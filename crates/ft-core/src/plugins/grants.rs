use anyhow::{bail, ensure, Context, Result};
use ft_plugins::{Permissions, Sending};

use crate::{Core, Event};

impl Core {
    pub(crate) async fn granted_to(&self, id: &str) -> Result<Permissions> {
        let row = self
            .store
            .plugin(id)
            .await?
            .with_context(|| format!("{id} is not installed here"))?;
        Ok(serde_json::from_str(&row.granted).unwrap_or_default())
    }

    pub async fn grant_plugin(&self, id: &str, granted: Permissions) -> Result<()> {
        let manifest = self.plugin_manifest(id)?;
        allowed(&manifest.permissions, &granted)?;
        self.store
            .grant_plugin(id, &serde_json::to_string(&granted)?)
            .await?;
        let _ = self.events.send(Event::PluginsChanged);
        if !granted.remind && self.store.forget_reminders(id).await? {
            let _ = self.events.send(Event::RemindersChanged);
        }
        Ok(())
    }
}

// Refuse grants the plugin never asked for
pub(crate) fn allowed(asked: &Permissions, granted: &Permissions) -> Result<()> {
    for host in &granted.network {
        ensure!(
            asked.network.contains(host),
            "the plugin never asked to talk to {host}"
        );
    }
    ensure!(
        !granted.reads_given_messages || asked.reads_given_messages,
        "the plugin never asked to read what you hand it"
    );
    if reach(granted.send) > reach(asked.send) {
        bail!("the plugin never asked to write in the chat that way");
    }
    ensure!(
        !granted.print || asked.print,
        "the plugin never asked to print"
    );
    ensure!(
        !granted.live || asked.live,
        "the plugin never asked to talk to the other side"
    );
    ensure!(
        !granted.remind || asked.remind,
        "the plugin never asked to set reminders"
    );
    ensure!(
        !granted.drive || asked.drive,
        "the plugin never asked for your cloud"
    );
    ensure!(
        !granted.location || asked.location,
        "the plugin never asked where the phone is"
    );
    ensure!(
        !granted.translate || asked.translate,
        "the plugin never asked to translate"
    );
    ensure!(
        granted.storage == ft_plugins::Storage::Small
            || asked.storage == ft_plugins::Storage::Large,
        "the plugin never asked for that much room"
    );
    Ok(())
}

pub(crate) fn narrowed(granted: &Permissions, asked: &Permissions) -> Permissions {
    Permissions {
        network: granted
            .network
            .iter()
            .filter(|host| asked.network.contains(host))
            .cloned()
            .collect(),
        reads_given_messages: granted.reads_given_messages && asked.reads_given_messages,
        send: if reach(granted.send) <= reach(asked.send) {
            granted.send
        } else {
            asked.send
        },
        print: granted.print && asked.print,
        live: granted.live && asked.live,
        remind: granted.remind && asked.remind,
        drive: granted.drive && asked.drive,
        storage: if asked.storage == ft_plugins::Storage::Large {
            granted.storage
        } else {
            ft_plugins::Storage::Small
        },
        location: granted.location && asked.location,
        translate: granted.translate && asked.translate,
    }
}

fn reach(sending: Sending) -> u8 {
    match sending {
        Sending::Nothing => 0,
        Sending::Propose => 1,
        Sending::Auto => 2,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grant_may_never_go_beyond_what_was_asked() {
        let asked = Permissions {
            network: vec!["api.openai.com".to_owned()],
            reads_given_messages: true,
            send: Sending::Propose,
            ..Permissions::default()
        };
        assert!(
            allowed(&asked, &Permissions::default()).is_ok(),
            "granting nothing is always fine"
        );
        assert!(allowed(&asked, &asked).is_ok());
        assert!(
            allowed(&Permissions::default(), &asked).is_err(),
            "it asked for nothing"
        );
        assert!(allowed(
            &asked,
            &Permissions {
                send: Sending::Auto,
                ..asked.clone()
            }
        )
        .is_err());
        let elsewhere = Permissions {
            network: vec!["evil.example".to_owned()],
            ..Permissions::default()
        };
        assert!(allowed(&asked, &elsewhere).is_err());
        let printing = Permissions {
            print: true,
            ..asked.clone()
        };
        assert!(
            allowed(&asked, &printing).is_err(),
            "it never asked to print"
        );
        assert!(allowed(&printing, &printing).is_ok());
        for wanted in [
            Permissions {
                live: true,
                ..Permissions::default()
            },
            Permissions {
                remind: true,
                ..Permissions::default()
            },
            Permissions {
                drive: true,
                ..Permissions::default()
            },
            Permissions {
                storage: ft_plugins::Storage::Large,
                ..Permissions::default()
            },
            Permissions {
                location: true,
                ..Permissions::default()
            },
            Permissions {
                translate: true,
                ..Permissions::default()
            },
        ] {
            assert!(
                allowed(&asked, &wanted).is_err(),
                "{wanted:?} was never asked for"
            );
            assert!(allowed(&wanted, &wanted).is_ok());
        }
    }

    #[test]
    fn a_narrowed_grant_always_fits_what_is_asked() {
        let all = Permissions {
            network: vec!["a.example.com".to_owned(), "b.example.com".to_owned()],
            reads_given_messages: true,
            send: Sending::Auto,
            print: true,
            live: true,
            remind: true,
            drive: true,
            storage: ft_plugins::Storage::Large,
            location: true,
            translate: true,
        };
        for asked in [
            Permissions::default(),
            Permissions {
                network: vec!["b.example.com".to_owned()],
                send: Sending::Propose,
                ..Permissions::default()
            },
            all.clone(),
        ] {
            let left = narrowed(&all, &asked);
            assert!(allowed(&asked, &left).is_ok(), "{left:?} beyond {asked:?}");
            assert_eq!(narrowed(&left, &all), left, "and never more than before");
        }
        assert_eq!(narrowed(&all, &all), all);
        assert_eq!(
            narrowed(&Permissions::default(), &all),
            Permissions::default(),
            "nothing new is turned on"
        );
    }
}
