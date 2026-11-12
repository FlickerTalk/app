use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    Answered,
    Missed,
    Declined,
    Busy,
    Cancelled,
    Unreachable,
    Failed,
}

impl CallOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::Missed => "missed",
            Self::Declined => "declined",
            Self::Busy => "busy",
            Self::Cancelled => "cancelled",
            Self::Unreachable => "unreachable",
            Self::Failed => "failed",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Self::Answered, Self::Missed, Self::Declined, Self::Busy, Self::Cancelled, Self::Unreachable, Self::Failed]
            .into_iter()
            .find(|outcome| outcome.as_str() == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
    pub call_id: String,
    pub contact: String,
    pub outgoing: bool,
    pub video: bool,
    pub started_at: i64,
    pub answered_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub outcome: Option<CallOutcome>,
}

impl Store {
    pub async fn insert_call(&self, call: &CallRecord) -> Result<bool> {
        let result = sqlx::query(
            "INSERT INTO calls (call_id, contact, outgoing, video, started_at, answered_at, ended_at, outcome)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (call_id) DO NOTHING",
        )
        .bind(&call.call_id)
        .bind(&call.contact)
        .bind(call.outgoing)
        .bind(call.video)
        .bind(call.started_at)
        .bind(call.answered_at)
        .bind(call.ended_at)
        .bind(call.outcome.map(CallOutcome::as_str))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn call(&self, call_id: &str) -> Result<Option<CallRecord>> {
        let row = sqlx::query("SELECT * FROM calls WHERE call_id = ?").bind(call_id).fetch_optional(&self.pool).await?;
        Ok(row.as_ref().map(call_from))
    }

    pub async fn calls(&self, limit: i64) -> Result<Vec<CallRecord>> {
        let rows = sqlx::query("SELECT * FROM calls ORDER BY started_at DESC, call_id DESC LIMIT ?").bind(limit).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(call_from).collect())
    }

    pub async fn answer_call(&self, call_id: &str, at: i64) -> Result<()> {
        sqlx::query("UPDATE calls SET answered_at = ? WHERE call_id = ? AND answered_at IS NULL")
            .bind(at)
            .bind(call_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn finish_call(&self, call_id: &str, at: i64, outcome: CallOutcome) -> Result<()> {
        sqlx::query("UPDATE calls SET ended_at = ?, outcome = ? WHERE call_id = ? AND ended_at IS NULL")
            .bind(at)
            .bind(outcome.as_str())
            .bind(call_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn finish_open_calls(&self, at: i64) -> Result<()> {
        sqlx::query(
            "UPDATE calls SET ended_at = ?, outcome = CASE
                 WHEN answered_at IS NOT NULL THEN 'answered'
                 WHEN outgoing = 1 THEN 'cancelled'
                 ELSE 'missed' END
             WHERE ended_at IS NULL",
        )
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn forget_calls_before(&self, at: i64) -> Result<u64> {
        let gone = sqlx::query("DELETE FROM calls WHERE ended_at IS NOT NULL AND ended_at < ?").bind(at).execute(&self.pool).await?;
        Ok(gone.rows_affected())
    }
}

fn call_from(row: &SqliteRow) -> CallRecord {
    let outcome: Option<String> = row.get("outcome");
    CallRecord {
        call_id: row.get("call_id"),
        contact: row.get("contact"),
        outgoing: row.get("outgoing"),
        video: row.get("video"),
        started_at: row.get("started_at"),
        answered_at: row.get("answered_at"),
        ended_at: row.get("ended_at"),
        outcome: outcome.as_deref().and_then(CallOutcome::parse),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{contact, store};

    fn call(id: &str, contact: &str, outgoing: bool, started_at: i64) -> CallRecord {
        CallRecord {
            call_id: id.to_owned(),
            contact: contact.to_owned(),
            outgoing,
            video: true,
            started_at,
            answered_at: None,
            ended_at: None,
            outcome: None,
        }
    }

    #[tokio::test]
    async fn calls_are_logged_from_ringing_to_their_end() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        assert!(store.insert_call(&call("c1", "ft_bob", true, 10)).await.expect("logs"));
        assert!(!store.insert_call(&call("c1", "ft_bob", true, 10)).await.expect("once"), "a repeated offer is logged once");
        store.answer_call("c1", 12).await.expect("answers");
        store.finish_call("c1", 70, CallOutcome::Answered).await.expect("ends");

        let logged = store.call("c1").await.expect("reads").expect("present");
        assert_eq!((logged.answered_at, logged.ended_at, logged.outcome), (Some(12), Some(70), Some(CallOutcome::Answered)));
        assert!(store.call("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn the_call_history_is_newest_first() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        for (id, at) in [("old", 1), ("new", 3), ("mid", 2)] {
            store.insert_call(&call(id, "ft_bob", false, at)).await.unwrap();
        }
        let ids: Vec<_> = store.calls(2).await.expect("lists").into_iter().map(|c| c.call_id).collect();
        assert_eq!(ids, ["new", "mid"]);
    }

    #[tokio::test]
    async fn calls_left_open_are_closed() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_call(&call("talking", "ft_bob", true, 1)).await.unwrap();
        store.answer_call("talking", 2).await.unwrap();
        store.insert_call(&call("calling", "ft_bob", true, 3)).await.unwrap();
        store.insert_call(&call("ringing", "ft_bob", false, 4)).await.unwrap();
        store.insert_call(&call("done", "ft_bob", false, 5)).await.unwrap();
        store.finish_call("done", 6, CallOutcome::Declined).await.unwrap();

        store.finish_open_calls(9).await.expect("closes");
        let outcome = |id: &'static str| {
            let store = &store;
            async move { store.call(id).await.unwrap().unwrap().outcome }
        };
        assert_eq!(outcome("talking").await, Some(CallOutcome::Answered));
        assert_eq!(outcome("calling").await, Some(CallOutcome::Cancelled));
        assert_eq!(outcome("ringing").await, Some(CallOutcome::Missed));
        assert_eq!(outcome("done").await, Some(CallOutcome::Declined), "finished calls are left alone");
    }

    #[tokio::test]
    async fn ended_calls_before_a_time_can_be_forgotten() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        for (id, at) in [("old", 1), ("going", 2), ("new", 9)] {
            store.insert_call(&call(id, "ft_bob", false, at)).await.unwrap();
        }
        store.finish_call("old", 3, CallOutcome::Answered).await.unwrap();
        store.finish_call("new", 10, CallOutcome::Missed).await.unwrap();

        assert_eq!(store.forget_calls_before(5).await.expect("forgets"), 1);
        let left: Vec<_> = store.calls(10).await.unwrap().into_iter().map(|c| c.call_id).collect();
        assert_eq!(left, ["new", "going"]);
    }

    #[test]
    fn call_outcomes_have_stable_names() {
        for outcome in [
            CallOutcome::Answered,
            CallOutcome::Missed,
            CallOutcome::Declined,
            CallOutcome::Busy,
            CallOutcome::Cancelled,
            CallOutcome::Unreachable,
            CallOutcome::Failed,
        ] {
            assert_eq!(CallOutcome::parse(outcome.as_str()), Some(outcome));
        }
        assert_eq!(CallOutcome::Unreachable.as_str(), "unreachable");
    }
}
