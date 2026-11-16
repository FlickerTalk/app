use serde::{Deserialize, Serialize};
use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Subscription {
    until: i64,
}

#[cfg_attr(not(mobile), allow(dead_code))]
fn entitlement_until(body: tauri::ipc::InvokeResponseBody) -> Option<i64> {
    Some(body.deserialize::<Subscription>().ok()?.until)
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct EntitlementEvents<'a> {
    channel: &'a tauri::ipc::Channel<serde_json::Value>,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct SubscriptionPrice {
    #[serde(default)]
    price: Option<String>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
impl SubscriptionPrice {
    fn shown(self) -> Option<String> {
        self.price
            .map(|price| price.trim().to_owned())
            .filter(|price| !price.is_empty())
    }
}

impl<R: Runtime> Platform<R> {
    /// Returns until when the subscription runs (ms), or 0.
    pub fn subscribe(&self) -> Result<i64> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Subscription>("subscribe", ())?
                .until)
        }
        #[cfg(not(mobile))]
        {
            Err(crate::Error::Unsupported)
        }
    }

    pub fn restore_subscription(&self) -> Result<i64> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Subscription>("restoreSubscription", ())?
                .until)
        }
        #[cfg(not(mobile))]
        {
            Err(crate::Error::Unsupported)
        }
    }

    pub fn subscription(&self) -> Result<i64> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Subscription>("subscription", ())?
                .until)
        }
        #[cfg(not(mobile))]
        {
            Ok(0)
        }
    }

    /// The handler runs on a native thread, keep it short.
    pub fn listen_entitlements(&self, handler: impl Fn(i64) + Send + Sync + 'static) {
        #[cfg(mobile)]
        {
            let channel = tauri::ipc::Channel::new(move |body| {
                if let Some(until) = entitlement_until(body) {
                    handler(until);
                }
                Ok(())
            });
            let _ = self.run(
                "registerEntitlementEvents",
                EntitlementEvents { channel: &channel },
            );
        }
        #[cfg(not(mobile))]
        let _ = handler;
    }

    /// The Store's own formatted string. Don't convert, round or cache it.
    pub fn subscription_price(&self) -> Result<Option<String>> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<SubscriptionPrice>("subscriptionPrice", ())?
                .shown())
        }
        #[cfg(not(mobile))]
        {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kotlin_side_gets_the_subscription_it_expects() {
        let subscription: Subscription =
            serde_json::from_value(serde_json::json!({ "until": 1_800_000_000_000i64 })).unwrap();
        assert_eq!(subscription.until, 1_800_000_000_000);
    }

    #[test]
    fn the_store_price_comes_back_as_the_store_formats_it() {
        let said: SubscriptionPrice =
            serde_json::from_value(serde_json::json!({ "price": "0,99 €" })).unwrap();
        assert_eq!(said.shown().as_deref(), Some("0,99 €"));
        for silent in [
            serde_json::json!({}),
            serde_json::json!({ "price": null }),
            serde_json::json!({ "price": "  " }),
        ] {
            let answer: SubscriptionPrice = serde_json::from_value(silent).unwrap();
            assert_eq!(answer.shown(), None);
        }
    }

    #[test]
    fn a_live_entitlement_is_read_as_the_native_side_sends_it() {
        let read =
            |json: &str| entitlement_until(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"until":1800000000000}"#), Some(1_800_000_000_000));
        assert_eq!(read(r#"{"until":0}"#), Some(0));
        assert_eq!(read(r#"{"price":"0,99 €"}"#), None);
        assert_eq!(read("not json"), None);
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_has_no_store_price() {
        let platform = Platform::<tauri::Wry> {
            _runtime: std::marker::PhantomData,
        };
        assert_eq!(platform.subscription_price().ok(), Some(None));
    }
}
