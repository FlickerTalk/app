//! The support contact at work, over a running `Core`: it accepts whoever writes, answers what
//! comes in (`policy`), at most as often as `limit` allows, takes calls with the recording
//! (`devices`) and forgets what was said after `Settings::keep_for`.
//!
//! Everything about a contact runs under that contact's lock, so two events for one person never
//! answer twice; different people are answered side by side. Nothing here logs who wrote or what.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::Result;
use ft_core::{CallUpdate, Core, Event};
use ft_storage::OutboxEntry;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;

use crate::clips::{self, SAMPLE_FILE, SAMPLE_MIME, SAMPLE_NAME, VOICE_MESSAGE, VOICE_MIME};
use crate::devices::{audio_platform, video_platform};
use crate::limit::{Limits, RateLimiter, Verdict};
use crate::policy::{self, Incoming, Reply};

/// Up to how many bytes a file sent to the support contact is pulled (the rest wait, unasked).
pub const AUTO_DOWNLOAD: i64 = 5_000_000;

/// The settings key that says a contact was welcomed.
const WELCOMED: &str = "support_welcomed:";

#[derive(Debug, Clone)]
pub struct Settings {
    /// How long messages and call records stay.
    pub keep_for: Duration,
    /// How long a call lasts at most: the support contact hangs up then.
    pub max_call: Duration,
    /// How long a call rings before it is answered.
    pub answer_after: Duration,
    pub limits: Limits,
    /// What never reached its contact after this long is given up.
    pub give_up_after: Duration,
    /// How often everything is looked at again, in case an event was missed.
    pub scan_every: Duration,
    /// How often old messages, calls and undeliverable replies are cleared.
    pub tidy_every: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            keep_for: Duration::from_secs(10 * 60),
            max_call: Duration::from_secs(2 * 60),
            answer_after: Duration::from_secs(1),
            limits: Limits::default(),
            give_up_after: Duration::from_secs(24 * 60 * 60),
            scan_every: Duration::from_secs(10),
            tidy_every: Duration::from_secs(60),
        }
    }
}

pub struct Support {
    core: Arc<Core>,
    settings: Settings,
    limiter: Mutex<RateLimiter>,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Where the copies of the bundled files wait to be sent (a transit folder of the core's).
    outgoing: PathBuf,
}

impl Support {
    /// Makes the core the support contact: its name, the mailbox on, the files folder (clearing
    /// copies left by an earlier run), how much of a file it pulls, and the recording as the
    /// microphone and the camera of its calls.
    pub async fn prepare(core: &Arc<Core>, files: &Path) -> Result<()> {
        core.set_name(crate::NAME).await?;
        if !core.mailbox().await? {
            core.set_mailbox(true).await?;
        }
        crate::owner_only(files, true)?;
        let outgoing = files.join("outgoing");
        tokio::fs::create_dir_all(&outgoing).await?;
        core.set_files_dir(files.to_owned());
        ft_core::files::sweep_orphans(core.store(), files, vec![outgoing]).await?;
        core.set_auto_download_limit(AUTO_DOWNLOAD).await?;
        core.set_call_audio(Some(audio_platform(Arc::new(clips::call_voice()?))));
        core.set_call_video(Some(video_platform(Arc::new(clips::call_video()), clips::CALL_FPS)));
        Ok(())
    }

    pub fn new(core: Arc<Core>, settings: Settings, files: &Path) -> Arc<Self> {
        let limiter = Mutex::new(RateLimiter::new(settings.limits));
        Arc::new(Self { core, settings, limiter, locks: Mutex::default(), outgoing: files.join("outgoing") })
    }

    /// Starts answering: what waited, then every event, with a look at everything every
    /// `scan_every` and a tidy every `tidy_every`.
    pub fn spawn(self: &Arc<Self>) -> JoinHandle<()> {
        let support = self.clone();
        let mut events = self.core.events();
        tokio::spawn(async move {
            let mut scan = tokio::time::interval(support.settings.scan_every);
            let mut tidy = tokio::time::interval(support.settings.tidy_every);
            loop {
                tokio::select! {
                    event = events.recv() => match event {
                        Ok(event) => support.on_event(event),
                        Err(RecvError::Lagged(_)) => support.later(|support| async move { support.catch_up().await }),
                        Err(RecvError::Closed) => return,
                    },
                    _ = scan.tick() => support.later(|support| async move { support.catch_up().await }),
                    _ = tidy.tick() => support.later(|support| async move { support.tidy().await }),
                }
            }
        })
    }

    /// Clears what is old: replies that never arrived, messages and calls past `keep_for`.
    pub async fn tidy(&self) {
        let now = ft_core::now();
        if let Ok(outbox) = self.core.store().outbox().await {
            for message in stale(&outbox, now - millis(self.settings.give_up_after)) {
                let _ = self.core.forget_message(&message).await;
            }
        }
        if let Ok(contacts) = self.core.store().contacts().await {
            for contact in contacts {
                let _ = self.keep_short(&contact).await;
            }
        }
        let _ = self.core.sweep_history().await;
        let _ = self.core.store().forget_calls_before(now - millis(self.settings.keep_for)).await;
    }

    /// Runs `work` in the background, so that the event loop never waits.
    fn later<F, Fut>(self: &Arc<Self>, work: F)
    where
        F: FnOnce(Arc<Self>) -> Fut,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(work(self.clone()));
    }

    fn on_event(self: &Arc<Self>, event: Event) {
        match event {
            // A stranger wrote, or someone added this card: they get in at once (A5 would keep
            // them waiting, and a call of theirs busy).
            Event::ContactsChanged => self.later(|support| async move { support.accept_requests().await }),
            Event::MessagesChanged { contact } => self.later(|support| async move { support.answer(&contact).await }),
            Event::Call { contact, call, update } => match update {
                CallUpdate::Incoming { .. } => self.later(|support| async move { support.take_call(&contact, &call).await }),
                CallUpdate::Connected => self.later(|support| async move { support.hang_up_later(&call).await }),
                CallUpdate::MissedWhileBusy => self.later(|support| async move { support.say(&contact, policy::BUSY).await }),
                _ => {}
            },
            _ => {}
        }
    }

    /// Everyone waiting is let in, and whoever has unread messages is answered.
    async fn catch_up(self: Arc<Self>) {
        self.accept_requests().await;
        let Ok(conversations) = self.core.store().conversations().await else { return };
        for conversation in conversations.into_iter().filter(|conversation| conversation.unread > 0) {
            let contact = conversation.contact.device_id;
            self.later(|support| async move { support.answer(&contact).await });
        }
    }

    async fn accept_requests(self: &Arc<Self>) {
        let Ok(requests) = self.core.requests().await else { return };
        for request in requests.into_iter().filter(|request| !request.contact.blocked) {
            let contact = request.contact.device_id;
            let lock = self.lock_for(&contact);
            let _one_at_a_time = lock.lock().await;
            if self.core.accept_contact(&contact).await.is_ok() {
                if let Ok(Some(accepted)) = self.core.store().contact(&contact).await {
                    let _ = self.keep_short(&accepted).await;
                }
            }
        }
    }

    /// Messages and calls of this contact go after `keep_for`.
    async fn keep_short(&self, contact: &ft_storage::Contact) -> Result<()> {
        let keep_for = i64::try_from(self.settings.keep_for.as_secs().max(1)).unwrap_or(i64::MAX);
        if contact.keep_for != keep_for || contact.burn_after_read != 0 {
            self.core.set_history(&contact.device_id, keep_for, 0).await?;
        }
        Ok(())
    }

    /// Answers what the contact sent since the last answer, if the limits let it.
    async fn answer(self: &Arc<Self>, contact: &str) {
        let lock = self.lock_for(contact);
        let _one_at_a_time = lock.lock().await;
        let _ = self.answer_holding(contact).await;
    }

    async fn answer_holding(&self, contact: &str) -> Result<()> {
        let store = self.core.store();
        let Some(stored) = store.contact(contact).await? else { return Ok(()) };
        if stored.blocked {
            return Ok(());
        }
        if !stored.accepted {
            self.core.accept_contact(contact).await?;
        }
        self.keep_short(&stored).await?;
        // What comes in while it waits for the gap gets the same answer.
        loop {
            if store.unread(contact).await?.is_empty() {
                return Ok(());
            }
            let verdict = self.limiter.lock().unwrap_or_else(PoisonError::into_inner).check(contact, now_ms());
            match verdict {
                Verdict::Go => break,
                Verdict::Wait(wait) => tokio::time::sleep(wait).await,
                Verdict::Spent { notice } => {
                    self.core.mark_read(contact).await?;
                    if notice {
                        self.core.send_text(contact, policy::DAILY_CAP).await?;
                    }
                    return Ok(());
                }
            }
        }

        let unread = store.unread(contact).await?;
        let mut batch = Vec::with_capacity(unread.len());
        for id in &unread {
            let Some(message) = store.message(id).await? else { continue };
            batch.push(match store.file(id).await? {
                Some(file) => Incoming::file(&file.mime),
                None => Incoming::Text(message.body),
            });
        }
        let first = store.setting(&format!("{WELCOMED}{contact}")).await?.is_none();
        let replies = policy::replies(first, &batch);
        // Read before the answer, as a person would. A message landing between the two reads is
        // marked read without an answer of its own: the next one it sends is answered.
        self.core.mark_read(contact).await?;
        self.limiter.lock().unwrap_or_else(PoisonError::into_inner).record(contact, now_ms());
        if first {
            store.set_setting(&format!("{WELCOMED}{contact}"), "1").await?;
        }
        for reply in replies {
            match reply {
                Reply::ThumbsUp => {
                    if let Some(message) = unread.first() {
                        let _ = self.core.react(contact, message, Some("👍")).await;
                    }
                }
                Reply::Text(text) => {
                    self.core.send_text(contact, &text).await?;
                }
                Reply::SampleFile => self.send_copy(contact, SAMPLE_FILE, SAMPLE_NAME, SAMPLE_MIME).await?,
                Reply::VoiceMessage => {
                    let name = clips::voice_name(now_ms() / 1_000);
                    self.send_copy(contact, VOICE_MESSAGE, &name, VOICE_MIME).await?;
                }
            }
        }
        Ok(())
    }

    /// Sends a bundled file from a copy of its own, which goes with the message (`tidy`) or,
    /// left over, at the next start (`prepare`).
    async fn send_copy(&self, contact: &str, bytes: &[u8], name: &str, mime: &str) -> Result<()> {
        let folder = self.outgoing.join(hex::encode(rand::random::<[u8; 8]>()));
        tokio::fs::create_dir_all(&folder).await?;
        let path = folder.join(name);
        tokio::fs::write(&path, bytes).await?;
        self.core.send_file(contact, &path, name, mime).await?;
        Ok(())
    }

    /// One of the fixed notices, within the limits.
    async fn say(self: &Arc<Self>, contact: &str, text: &str) {
        let lock = self.lock_for(contact);
        let _one_at_a_time = lock.lock().await;
        let verdict = self.limiter.lock().unwrap_or_else(PoisonError::into_inner).check(contact, now_ms());
        if let Verdict::Wait(wait) = verdict {
            tokio::time::sleep(wait).await;
        } else if verdict != Verdict::Go {
            return;
        }
        if self.core.send_text(contact, text).await.is_ok() {
            self.limiter.lock().unwrap_or_else(PoisonError::into_inner).record(contact, now_ms());
        }
    }

    /// A call rings: answered with the recording after `answer_after`, or refused with a word.
    async fn take_call(self: &Arc<Self>, contact: &str, call: &str) {
        if !self.core.native_calls() {
            let _ = self.core.decline_ringing_call().await;
            self.say(contact, policy::CALLS_OFF).await;
            return;
        }
        tokio::time::sleep(self.settings.answer_after).await;
        let routing = self.core.call_routing().await;
        if self.core.answer_native_call(call, routing).await.is_err() {
            self.say(contact, policy::CALLS_OFF).await;
        }
    }

    /// The call connected: the support contact hangs up after `max_call`, if it still goes on.
    async fn hang_up_later(self: &Arc<Self>, call: &str) {
        tokio::time::sleep(self.settings.max_call).await;
        if self.core.current_call().await.ok().flatten().is_some_and(|current| current.call == call) {
            let _ = self.core.end_call(call, false).await;
        }
    }

    fn lock_for(&self, contact: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.locks.lock().unwrap_or_else(PoisonError::into_inner).entry(contact.to_owned()).or_default().clone()
    }
}

fn now_ms() -> u64 {
    u64::try_from(ft_core::now()).unwrap_or(0)
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

/// The replies waiting in the outbox since before `before` (ms): never delivered, given up.
pub fn stale(outbox: &[OutboxEntry], before: i64) -> Vec<String> {
    outbox.iter().filter(|entry| entry.created_at < before).map(|entry| entry.message_id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, created_at: i64) -> OutboxEntry {
        OutboxEntry {
            message_id: id.to_owned(),
            contact: "ft_a".to_owned(),
            created_at,
            attempts: 3,
            next_attempt: created_at + 1,
            in_mailbox: false,
            mailed_at: None,
        }
    }

    #[test]
    fn replies_older_than_the_limit_are_given_up() {
        let outbox = [entry("old", 10), entry("edge", 50), entry("new", 90)];
        assert_eq!(stale(&outbox, 50), vec!["old".to_owned()]);
        assert!(stale(&[], 50).is_empty());
    }

    #[test]
    fn by_default_it_keeps_ten_minutes_and_calls_last_two() {
        let settings = Settings::default();
        assert_eq!(settings.keep_for, Duration::from_secs(600));
        assert_eq!(settings.max_call, Duration::from_secs(120));
        assert_eq!(settings.limits, Limits::default());
        assert!(settings.give_up_after >= Duration::from_secs(3_600));
    }

    #[test]
    fn the_welcome_mark_is_one_key_per_contact() {
        assert_eq!(format!("{WELCOMED}ft_a"), "support_welcomed:ft_a");
    }
}
