use anyhow::{ensure, Result};
use ft_storage::Reminder;

use super::RECORD_KEY;
use crate::{Core, Event};

const REMINDER_TEXT: usize = 200;

impl Core {
    // A reminder set in a hidden session only rings while that session is open
    pub async fn set_reminder(&self, id: &str, session: Option<&str>, reminder: &str, at: i64, text: &str) -> Result<()> {
        let session = self.place(session)?.map(str::to_owned);
        ensure!(self.granted_to(id).await?.remind, "{id} may not set reminders");
        ensure!(!reminder.is_empty() && reminder.len() <= RECORD_KEY, "that reminder id is too long");
        ensure!(at > 0, "a reminder needs a time");
        let text: String = text.chars().take(REMINDER_TEXT).collect();
        self.store.set_reminder(&Reminder { plugin: id.to_owned(), session, id: reminder.to_owned(), at, text }).await?;
        let _ = self.events.send(Event::RemindersChanged);
        Ok(())
    }

    pub async fn cancel_reminder(&self, id: &str, session: Option<&str>, reminder: &str) -> Result<bool> {
        let gone = self.store.cancel_reminder(id, self.place(session)?, reminder).await?;
        if gone {
            let _ = self.events.send(Event::RemindersChanged);
        }
        Ok(gone)
    }

    pub async fn plugin_reminders(&self, id: &str, session: Option<&str>) -> Result<Vec<Reminder>> {
        let session = self.place(session)?;
        Ok(self.store.reminders(Some(id)).await?.into_iter().filter(|reminder| reminder.session.as_deref() == session).collect())
    }

    pub async fn ringing_reminder(&self, id: &str, reminder: &str) -> Result<Option<Reminder>> {
        Ok(self.reminders().await?.into_iter().find(|ringing| ringing.plugin == id && ringing.id == reminder))
    }

    pub async fn reminders(&self) -> Result<Vec<Reminder>> {
        Ok(self
            .store
            .reminders(None)
            .await?
            .into_iter()
            .filter(|reminder| reminder.session.as_deref().is_none_or(|session| self.is_session_open(session)))
            .collect())
    }

    pub async fn due_reminders(&self, at: i64) -> Result<Vec<Reminder>> {
        Ok(self.reminders().await?.into_iter().filter(|reminder| reminder.at <= at).collect())
    }
}
