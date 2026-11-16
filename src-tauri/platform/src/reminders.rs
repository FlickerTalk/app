use serde::{Deserialize, Serialize};
use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Serialize)]
struct Reminders<'a> {
    reminders: &'a str,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct PendingReminder {
    reminder: String,
}

#[derive(Deserialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
struct ExactAlarms {
    allowed: bool,
}

impl<R: Runtime> Platform<R> {
    pub fn set_reminders(&self, reminders: &str) -> Result<()> {
        self.run("setReminders", Reminders { reminders })
    }

    /// "plugin\nid", empty if none
    pub fn pending_reminder(&self) -> Result<String> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<PendingReminder>("pendingReminder", ())?
                .reminder)
        }
        #[cfg(not(mobile))]
        {
            Ok(String::new())
        }
    }

    pub fn exact_alarms(&self) -> Result<bool> {
        #[cfg(target_os = "android")]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<ExactAlarms>("exactAlarms", ())?
                .allowed)
        }
        #[cfg(not(target_os = "android"))]
        {
            Ok(true)
        }
    }

    /// The user decides after this returns, so the result is only the current state.
    pub fn request_exact_alarms(&self) -> Result<bool> {
        #[cfg(target_os = "android")]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<ExactAlarms>("requestExactAlarms", ())?
                .allowed)
        }
        #[cfg(not(target_os = "android"))]
        {
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kotlin_side_gets_the_reminder_names_it_expects() {
        let reminders = serde_json::to_value(Reminders {
            reminders: r#"[{"plugin":"p","id":"r1","at":5,"text":""}]"#,
        })
        .unwrap();
        assert_eq!(
            reminders["reminders"],
            r#"[{"plugin":"p","id":"r1","at":5,"text":""}]"#
        );
        let tapped: PendingReminder =
            serde_json::from_value(serde_json::json!({ "reminder": "p\nr1" })).unwrap();
        assert_eq!(tapped.reminder, "p\nr1");
    }

    #[test]
    fn whether_exact_alarms_are_allowed_comes_back_as_kotlin_resolves_it() {
        let allowed: ExactAlarms =
            serde_json::from_value(serde_json::json!({ "allowed": true })).unwrap();
        assert!(allowed.allowed);
        let refused: ExactAlarms =
            serde_json::from_value(serde_json::json!({ "allowed": false })).unwrap();
        assert!(!refused.allowed);
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_rings_reminders_on_time_without_asking() {
        let platform = Platform::<tauri::Wry> {
            _runtime: std::marker::PhantomData,
        };
        assert_eq!(platform.exact_alarms().ok(), Some(true));
        assert_eq!(platform.request_exact_alarms().ok(), Some(true));
    }
}
