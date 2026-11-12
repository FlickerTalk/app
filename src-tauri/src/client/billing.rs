use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_ft_platform::PlatformExt;

use super::{failed, now_ms, Client};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    state: String,
    until: i64,
    /// Google Play never gives the phone an expiry date, so `until` is only how long the last check
    /// holds. The screen shows "renews automatically" instead.
    #[serde(default)]
    renews: bool,
}

fn plan_view(access: ft_billing::Access, play: bool) -> PlanView {
    let renews = play && matches!(access, ft_billing::Access::Subscribed { .. });
    let (state, until) = match access {
        ft_billing::Access::Trial { until } => ("trial", until),
        ft_billing::Access::Subscribed { until } => ("subscribed", until),
        ft_billing::Access::Limited => ("limited", 0),
    };
    PlanView { state: state.to_owned(), until, renews }
}

#[tauri::command]
pub async fn core_plan(client: State<'_, Client>) -> Result<PlanView, String> {
    let core = client.core().await?;
    let plan = core.plan().await.map_err(failed)?;
    Ok(plan_view(ft_billing::Access::of(now_ms(), plan), cfg!(target_os = "android")))
}

#[derive(Serialize)]
pub struct PriceView {
    price: Option<String>,
}

fn price_view(answer: Result<Option<String>, String>) -> PriceView {
    PriceView { price: answer.ok().flatten() }
}

/// Never cache the price, it could end up shown from another store or country.
#[tauri::command]
pub async fn core_subscription_price(app: AppHandle) -> Result<PriceView, String> {
    let answer = tauri::async_runtime::spawn_blocking(move || app.platform().subscription_price().map_err(failed))
        .await
        .map_err(failed)
        .and_then(|answer| answer);
    Ok(price_view(answer))
}

#[tauri::command]
pub async fn core_subscribe(app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let until = tauri::async_runtime::spawn_blocking(move || buy_from(app.platform(), now_ms()))
        .await
        .map_err(failed)??;
    client.core().await?.set_entitlement(until).await.map_err(failed)
}

/// Paid-up-until in ms (0 = not paid), or a `plan.trouble.*` key the screen translates.
pub(crate) trait Shop {
    fn subscribe(&self) -> Result<i64, String>;
    fn restore(&self) -> Result<i64, String>;
    fn current(&self) -> Result<i64, String>;
}

impl<R: tauri::Runtime> Shop for tauri_plugin_ft_platform::Platform<R> {
    fn subscribe(&self) -> Result<i64, String> {
        tauri_plugin_ft_platform::Platform::subscribe(self).map_err(failed)
    }

    fn restore(&self) -> Result<i64, String> {
        self.restore_subscription().map_err(failed)
    }

    fn current(&self) -> Result<i64, String> {
        self.subscription().map_err(failed)
    }
}

pub(super) struct AppShop(pub(super) AppHandle);

impl Shop for AppShop {
    fn subscribe(&self) -> Result<i64, String> {
        Shop::subscribe(self.0.platform())
    }

    fn restore(&self) -> Result<i64, String> {
        Shop::restore(self.0.platform())
    }

    fn current(&self) -> Result<i64, String> {
        Shop::current(self.0.platform())
    }
}

/// The bridge calls block, so the plan watch runs them off the async runtime.
pub(super) struct ShopWord<S>(pub(super) Arc<S>);

#[async_trait::async_trait]
impl<S: Shop + Send + Sync + 'static> ft_core::Entitlements for ShopWord<S> {
    async fn until(&self) -> anyhow::Result<i64> {
        let shop = self.0.clone();
        tauri::async_runtime::spawn_blocking(move || shop.current()).await?.map_err(anyhow::Error::msg)
    }
}

fn buy_from(shop: &impl Shop, now: i64) -> Result<i64, String> {
    let until = shop.subscribe()?;
    if until > now {
        Ok(until)
    } else {
        Err("payment_failed".to_owned())
    }
}

fn restore_from(shop: &impl Shop, now: i64) -> Result<(i64, &'static str), String> {
    let until = shop.restore()?;
    Ok((until, if until > now { "restored" } else { "nothing" }))
}

#[tauri::command]
pub async fn core_restore_subscription(app: AppHandle, client: State<'_, Client>) -> Result<&'static str, String> {
    let (until, word) = tauri::async_runtime::spawn_blocking(move || restore_from(app.platform(), now_ms()))
        .await
        .map_err(failed)??;
    client.core().await?.set_entitlement(until).await.map_err(failed)?;
    Ok(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeShop {
        bought: Result<i64, String>,
        restored: Result<i64, String>,
        current: Result<i64, String>,
    }

    impl FakeShop {
        fn restoring(restored: Result<i64, String>) -> Self {
            Self { bought: Err("not asked".into()), restored, current: Err("not asked".into()) }
        }

        fn selling(bought: Result<i64, String>) -> Self {
            Self { bought, restored: Err("not asked".into()), current: Err("not asked".into()) }
        }

        fn saying(current: Result<i64, String>) -> Self {
            Self { bought: Err("not asked".into()), restored: Err("not asked".into()), current }
        }
    }

    impl Shop for FakeShop {
        fn subscribe(&self) -> Result<i64, String> {
            self.bought.clone()
        }

        fn restore(&self) -> Result<i64, String> {
            self.restored.clone()
        }

        fn current(&self) -> Result<i64, String> {
            self.current.clone()
        }
    }

    #[tokio::test]
    async fn the_plans_watch_hears_what_the_store_says_now() {
        use ft_core::Entitlements;
        assert_eq!(ShopWord(Arc::new(FakeShop::saying(Ok(42)))).until().await.unwrap(), 42);
        assert_eq!(ShopWord(Arc::new(FakeShop::saying(Ok(0)))).until().await.unwrap(), 0);
        assert!(ShopWord(Arc::new(FakeShop::saying(Err("store_unavailable".into())))).until().await.is_err());
    }

    const DAY_MS: i64 = 24 * 60 * 60 * 1000;

    #[test]
    fn restoring_says_whether_the_store_found_a_subscription() {
        let now = 1_800_000_000_000;
        assert_eq!(restore_from(&FakeShop::restoring(Ok(now + 300 * DAY_MS)), now), Ok((now + 300 * DAY_MS, "restored")));
        assert_eq!(restore_from(&FakeShop::restoring(Ok(0)), now), Ok((0, "nothing")));
        assert_eq!(restore_from(&FakeShop::restoring(Ok(now - DAY_MS)), now), Ok((now - DAY_MS, "nothing")));
        assert_eq!(restore_from(&FakeShop::restoring(Err("store_unavailable".into())), now), Err("store_unavailable".to_owned()));
    }

    #[test]
    fn a_purchase_answered_with_a_refunded_transaction_is_a_failed_payment() {
        let now = 1_800_000_000_000;
        assert_eq!(buy_from(&FakeShop::selling(Ok(0)), now), Err("payment_failed".to_owned()));
        assert_eq!(buy_from(&FakeShop::selling(Ok(now - DAY_MS)), now), Err("payment_failed".to_owned()));
        assert_eq!(buy_from(&FakeShop::selling(Ok(now + 365 * DAY_MS)), now), Ok(now + 365 * DAY_MS));
        assert_eq!(buy_from(&FakeShop::selling(Err("cancelled".into())), now), Err("cancelled".to_owned()));
        assert_eq!(buy_from(&FakeShop::selling(Err("pending_approval".into())), now), Err("pending_approval".to_owned()));
    }

    #[test]
    fn a_play_subscription_is_shown_as_renewing_and_an_apple_one_with_its_expiry() {
        let until = 1_830_000_000_000;
        let play = plan_view(ft_billing::Access::Subscribed { until }, true);
        assert_eq!(serde_json::to_value(&play).unwrap(), serde_json::json!({ "state": "subscribed", "until": until, "renews": true }));
        let apple = plan_view(ft_billing::Access::Subscribed { until }, false);
        assert_eq!(serde_json::to_value(&apple).unwrap()["renews"], serde_json::json!(false));
        for access in [ft_billing::Access::Trial { until }, ft_billing::Access::Limited] {
            assert_eq!(serde_json::to_value(plan_view(access, true)).unwrap()["renews"], serde_json::json!(false));
        }
    }

    #[test]
    fn the_plan_has_three_states_and_no_age() {
        let states: Vec<serde_json::Value> = [ft_billing::Access::Trial { until: 1 }, ft_billing::Access::Subscribed { until: 1 }, ft_billing::Access::Limited]
            .into_iter()
            .map(|access| serde_json::to_value(plan_view(access, false)).unwrap())
            .collect();
        assert_eq!(states.iter().map(|view| view["state"].as_str().unwrap()).collect::<Vec<_>>(), ["trial", "subscribed", "limited"]);
        assert!(states.iter().all(|view| view.get("age").is_none()));
    }

    #[test]
    fn the_plan_screen_gets_the_store_price_or_none() {
        let price = |answer| serde_json::to_value(price_view(answer)).unwrap();
        assert_eq!(price(Ok(Some("0,99 €".to_owned()))), serde_json::json!({ "price": "0,99 €" }));
        assert_eq!(price(Ok(None)), serde_json::json!({ "price": null }));
        assert_eq!(price(Err("store_unavailable".to_owned())), serde_json::json!({ "price": null }));
    }
}
