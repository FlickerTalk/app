use serde::{Deserialize, Serialize};
use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Serialize)]
struct OpenSlots<'a> {
    slots: &'a [u8],
}

#[derive(Serialize)]
struct QuietHours<'a> {
    week: &'a str,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct FullScreen {
    allowed: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct PushToken {
    token: String,
}

impl<R: Runtime> Platform<R> {
    pub fn set_open_slots(&self, slots: &[u8]) -> Result<()> {
        self.run("setOpenSlots", OpenSlots { slots })
    }

    pub fn set_quiet_hours(&self, week: &str) -> Result<()> {
        self.run("setQuietHours", QuietHours { week })
    }

    /// FCM token on Android, `gateway:bundle:token` for APNs on iOS
    pub fn push_token(&self) -> Result<String> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<PushToken>("pushToken", ())?
                .token)
        }
        #[cfg(not(mobile))]
        {
            Err(crate::Error::Unsupported)
        }
    }

    pub fn can_show_full_screen(&self) -> Result<bool> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<FullScreen>("canShowFullScreen", ())?
                .allowed)
        }
        #[cfg(not(mobile))]
        {
            Ok(false)
        }
    }

    pub fn ask_full_screen(&self) -> Result<()> {
        self.run("askFullScreen", ())
    }

    pub fn request_notifications(&self) -> Result<()> {
        self.run("requestNotifications", ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_push_token_comes_back_from_kotlin() {
        let answer: PushToken =
            serde_json::from_value(serde_json::json!({ "token": "fcm-abc" })).unwrap();
        assert_eq!(answer.token, "fcm-abc");
    }

    #[test]
    fn the_kotlin_side_gets_the_open_slots_and_quiet_hours_it_expects() {
        let slots = serde_json::to_value(OpenSlots { slots: &[1, 3] }).unwrap();
        assert_eq!(slots, serde_json::json!({ "slots": [1, 3] }));
        let hours = serde_json::to_value(QuietHours {
            week: "all;all;all;all;all;none;none",
        })
        .unwrap();
        assert_eq!(
            hours,
            serde_json::json!({ "week": "all;all;all;all;all;none;none" })
        );
    }
}
