//! Keeps the support contact's TURN user fresh (2026-10-28). The router hands one out in its
//! welcome, and it lives ten minutes; the support contact stays connected for days. With an
//! expired user it gathered no relay, and a call whose direct path did not work could not connect
//! (half the calls from a tablet failed so). Every minute it asks the router for a new one
//! (`GET /v1/turn-credentials`): new connections and calls use it. A new user every minute also
//! keeps each one far below coturn's quota of allocations per user.

use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use ft_core::TurnGrant;

/// How often a new TURN user is asked for.
pub const REFRESH_EVERY: Duration = Duration::from_secs(60);

/// From now on, every `every`: asks `fetch` for a TURN user and gives it to `store`. One that
/// cannot be had is asked for again at the next tick. Ends when `stopped` resolves.
pub async fn keep_fresh<F, Fut, S>(every: Duration, fetch: F, store: S, stopped: impl Future<Output = ()>)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<TurnGrant>>,
    S: FnMut(TurnGrant),
{
    let (mut fetch, mut store) = (fetch, store);
    let mut tick = tokio::time::interval(every);
    tokio::pin!(stopped);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            () = &mut stopped => return,
        }
        let fetched = tokio::select! {
            fetched = fetch() => fetched,
            () = &mut stopped => return,
        };
        if let Ok(grant) = fetched {
            store(grant);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    fn grant(username: &str) -> TurnGrant {
        TurnGrant { urls: vec!["turn:t:3478".to_owned()], username: username.to_owned(), credential: "c".to_owned() }
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_user_now_and_then_every_minute_until_stopped() {
        let asked = Arc::new(AtomicUsize::new(0));
        let stored = Arc::new(Mutex::new(Vec::new()));
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let (counter, kept) = (asked.clone(), stored.clone());
        let running = tokio::spawn(keep_fresh(
            REFRESH_EVERY,
            move || {
                let n = counter.fetch_add(1, Ordering::SeqCst);
                async move { Ok(grant(&format!("user-{n}"))) }
            },
            move |grant| kept.lock().unwrap().push(grant.username),
            async move {
                let _ = stopped.await;
            },
        ));
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(*stored.lock().unwrap(), ["user-0"], "one at once");
        tokio::time::sleep(REFRESH_EVERY * 2).await;
        assert_eq!(*stored.lock().unwrap(), ["user-0", "user-1", "user-2"]);
        stop.send(()).unwrap();
        running.await.unwrap();
        tokio::time::sleep(REFRESH_EVERY * 3).await;
        assert_eq!(asked.load(Ordering::SeqCst), 3, "nothing more once stopped");
    }

    #[tokio::test(start_paused = true)]
    async fn a_user_that_cannot_be_had_is_asked_for_again_at_the_next_tick() {
        let asked = Arc::new(AtomicUsize::new(0));
        let stored = Arc::new(Mutex::new(Vec::new()));
        let (counter, kept) = (asked.clone(), stored.clone());
        let running = tokio::spawn(keep_fresh(
            REFRESH_EVERY,
            move || {
                let n = counter.fetch_add(1, Ordering::SeqCst);
                async move {
                    if n == 0 {
                        anyhow::bail!("the router is restarting")
                    }
                    Ok(grant("later"))
                }
            },
            move |grant| kept.lock().unwrap().push(grant.username),
            std::future::pending(),
        ));
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(stored.lock().unwrap().is_empty());
        tokio::time::sleep(REFRESH_EVERY).await;
        assert_eq!(*stored.lock().unwrap(), ["later"]);
        running.abort();
    }
}
