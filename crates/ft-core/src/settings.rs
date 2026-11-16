use anyhow::{ensure, Context, Result};
use ft_plugins::{Kind, Player};
use ft_protocol::Body;
use ft_storage::GamePlayer;

use crate::{clean_name, Core, NAME_LIMIT};

const NAME: &str = "name";
const MAILBOX: &str = "mailbox";
const RECEIPTS_DEFAULT: &str = "receipts_default";
const QUIET_HOURS: &str = "quiet_hours";

impl Core {
    pub async fn name(&self) -> Result<Option<String>> {
        self.store.setting(NAME).await
    }

    pub async fn set_name(&self, name: &str) -> Result<()> {
        self.store.set_setting(NAME, name.trim()).await
    }

    pub async fn game_player(&self, plugin: &str, session: Option<&str>) -> Result<Option<Player>> {
        let place = self.games_place(plugin, session)?;
        Ok(self.store.game_player(place).await?.map(|kept| Player {
            who: kept.who,
            hair: kept.hair,
            hair_colour: kept.hair_colour,
            name: kept.name,
        }))
    }

    pub async fn set_game_player(
        &self,
        plugin: &str,
        session: Option<&str>,
        player: &Player,
    ) -> Result<()> {
        let place = self.games_place(plugin, session)?;
        ensure!(player.in_the_cast(), "that look is not in the cast");
        // The message never says the name, because an error can end up in a log.
        ensure!(is_clean(&player.name), "that name is not clean");
        let kept = GamePlayer {
            who: player.who.clone(),
            hair: player.hair.clone(),
            hair_colour: player.hair_colour.clone(),
            name: player.name.clone(),
        };
        self.store.set_game_player(place, &kept).await
    }

    fn games_place<'a>(&self, plugin: &str, session: Option<&'a str>) -> Result<Option<&'a str>> {
        ensure!(
            self.plugin_manifest(plugin)?.kind == Kind::Game,
            "only a game shares the player"
        );
        if let Some(session) = session {
            ensure!(self.is_session_open(session), "that session is not open");
        }
        Ok(session)
    }

    pub async fn mailbox(&self) -> Result<bool> {
        Ok(self.store.setting(MAILBOX).await?.as_deref() != Some("0"))
    }

    #[doc(hidden)]
    pub fn set_tells_mailbox(&self, tells: bool) {
        self.tells_mailbox
            .store(tells, std::sync::atomic::Ordering::Relaxed);
    }

    pub async fn set_mailbox(&self, enabled: bool) -> Result<()> {
        self.store
            .set_setting(MAILBOX, if enabled { "1" } else { "0" })
            .await?;
        for contact in self.store.all_contacts().await? {
            if self.quiet_until_written(&contact).await {
                continue;
            }
            let _ = self
                .send_control(&contact, Body::MailboxPreference { enabled })
                .await;
        }
        Ok(())
    }

    pub async fn receipts_default(&self) -> Result<bool> {
        Ok(self.store.setting(RECEIPTS_DEFAULT).await?.as_deref() != Some("0"))
    }

    pub async fn set_receipts_default(&self, receipts: bool) -> Result<()> {
        self.store
            .set_setting(RECEIPTS_DEFAULT, if receipts { "1" } else { "0" })
            .await
    }

    pub async fn quiet_hours(&self) -> Result<Option<String>> {
        Ok(self
            .store
            .setting(QUIET_HOURS)
            .await?
            .filter(|hours| !hours.is_empty()))
    }

    pub async fn quiet_week(&self) -> Result<String> {
        Ok(match self.quiet_hours().await? {
            Some(json) => serde_json::from_str::<Week>(&json)?.compact(),
            None => String::new(),
        })
    }

    pub async fn set_quiet_hours(&self, hours: Option<&str>) -> Result<()> {
        let kept = match hours {
            Some(json) => {
                let week: Week = serde_json::from_str(json).context("unreadable hours")?;
                week.check()?;
                serde_json::to_string(&week)?
            }
            None => String::new(),
        };
        self.store.set_setting(QUIET_HOURS, &kept).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Week {
    pub days: Vec<Day>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Day {
    // "all" or "none"
    Whole(String),
    // "HH:MM", wraps past midnight when to < from
    Hours { from: String, to: String },
}

impl Week {
    // Parsed by the native side. Seven `;`-separated days from Monday, each `all`, `none` or `FROM-TO` in minutes.
    pub fn compact(&self) -> String {
        let day = |day: &Day| match day {
            Day::Whole(which) => which.clone(),
            Day::Hours { from, to } => format!(
                "{}-{}",
                minutes(from).unwrap_or(0),
                minutes(to).unwrap_or(0)
            ),
        };
        self.days.iter().map(day).collect::<Vec<_>>().join(";")
    }

    fn check(&self) -> Result<()> {
        ensure!(self.days.len() == 7, "a week has seven days");
        for day in &self.days {
            match day {
                Day::Whole(which) => ensure!(
                    which == "all" || which == "none",
                    "a day is all, none or hours"
                ),
                Day::Hours { from, to } => ensure!(
                    minutes(from).is_some() && minutes(to).is_some(),
                    "hours are HH:MM"
                ),
            }
        }
        Ok(())
    }
}

fn is_clean(name: &str) -> bool {
    name.chars().nth(NAME_LIMIT).is_none() && clean_name(name) == name
}

fn minutes(time: &str) -> Option<u32> {
    let (hours, minutes) = time.split_once(':')?;
    let (hours, minutes): (u32, u32) = (hours.parse().ok()?, minutes.parse().ok()?);
    (hours < 24 && minutes < 60 && time.len() == 5).then_some(hours * 60 + minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_week_is_handed_to_the_phone_in_minutes() {
        let week: Week = serde_json::from_str(
            r#"{"days":[{"from":"18:00","to":"22:00"},"none",{"from":"22:00","to":"02:00"},"all","all","all","all"]}"#,
        )
        .unwrap();
        assert_eq!(week.compact(), "1080-1320;none;1320-120;all;all;all;all");
    }
}
