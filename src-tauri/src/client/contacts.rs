use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_ft_platform::PlatformExt;

use super::chat::{conversation_views, ConversationView};
use super::{failed, Client};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactView {
    id: String,
    name: String,
    fingerprint: String,
    mailbox: bool,
    blocked: bool,
    keep_for: i64,
    burn_after_read: i64,
    rules: RulesView,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct RulesView {
    muted: bool,
    accepts_chat: bool,
    accepts_calls: bool,
    receipts: bool,
    /// Older pages don't send this field.
    #[serde(default = "yes")]
    typing: bool,
}

fn yes() -> bool {
    true
}

impl From<ft_storage::ContactRules> for RulesView {
    fn from(rules: ft_storage::ContactRules) -> Self {
        Self {
            muted: rules.muted,
            accepts_chat: rules.accepts_chat,
            accepts_calls: rules.accepts_calls,
            receipts: rules.receipts,
            typing: rules.typing,
        }
    }
}

impl From<RulesView> for ft_storage::ContactRules {
    fn from(rules: RulesView) -> Self {
        Self {
            muted: rules.muted,
            accepts_chat: rules.accepts_chat,
            accepts_calls: rules.accepts_calls,
            receipts: rules.receipts,
            typing: rules.typing,
        }
    }
}

#[tauri::command]
pub async fn core_renew_link(
    session: Option<String>,
    client: State<'_, Client>,
) -> Result<String, String> {
    let online = client.online().await?;
    online
        .core
        .renew_link(session.as_deref())
        .await
        .map_err(failed)?;
    Ok(online
        .core
        .my_card_in(session.as_deref())
        .await
        .map_err(failed)?
        .to_link())
}

#[tauri::command]
pub async fn core_requests(
    session: Option<String>,
    client: State<'_, Client>,
) -> Result<Vec<ConversationView>, String> {
    let online = client.online().await?;
    let requests = match session.as_deref() {
        None => online.core.requests().await.map_err(failed)?,
        Some(session) => online
            .core
            .session_requests(session)
            .await
            .map_err(failed)?,
    };
    conversation_views(&online, requests).await
}

#[tauri::command]
pub async fn core_accept_contact(contact: String, client: State<'_, Client>) -> Result<(), String> {
    client
        .core()
        .await?
        .accept_contact(&contact)
        .await
        .map_err(failed)
}

#[tauri::command]
pub async fn core_decline_contact(
    contact: String,
    client: State<'_, Client>,
) -> Result<(), String> {
    client
        .core()
        .await?
        .decline_contact(&contact)
        .await
        .map_err(failed)
}

#[tauri::command]
pub async fn core_card(
    session: Option<String>,
    client: State<'_, Client>,
) -> Result<String, String> {
    Ok(client
        .core()
        .await?
        .my_card_in(session.as_deref())
        .await
        .map_err(failed)?
        .to_link())
}

#[tauri::command]
pub async fn core_add_contact(
    link: String,
    session: Option<String>,
    client: State<'_, Client>,
) -> Result<String, String> {
    let core = client.core().await?;
    let card = ft_contacts::ContactCard::from_link(&link).map_err(failed)?;
    if &card.device_id() == core.device_id() {
        return Err("that is your own contact card".to_owned());
    }
    let id = card.device_id().to_string();
    tauri::async_runtime::spawn(async move {
        let _ = core.add_contact_in(&link, None, session.as_deref()).await;
    });
    Ok(id)
}

#[tauri::command]
pub async fn core_add_support_contact(client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    core.add_bundled_contact(
        crate::support::support_link(),
        Some(crate::support::SUPPORT_NAME.to_owned()),
    )
    .await
    .map_err(failed)?;
    Ok(())
}

#[tauri::command]
pub fn core_support_id() -> Option<String> {
    crate::support::support_id()
}

#[tauri::command]
pub async fn core_share(text: String, app: AppHandle) -> Result<(), String> {
    app.platform().share_text(&text).map_err(failed)
}

#[derive(Serialize)]
pub struct PickedContactView {
    name: String,
    whatsapp: Option<String>,
}

fn picked_view(name: &str, number: &str, country: &str) -> PickedContactView {
    PickedContactView {
        name: ft_contacts::invite::greeting_name(name),
        whatsapp: ft_contacts::invite::whatsapp_number(number, country),
    }
}

fn deliver(
    whatsapp_link: Option<String>,
    open_whatsapp: impl FnOnce(&str) -> bool,
    share: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    match whatsapp_link {
        Some(link) if open_whatsapp(&link) => Ok(()),
        _ => share(),
    }
}

// the picked name and number go back to the page for this one message and are never kept
#[tauri::command]
pub async fn core_pick_contact(app: AppHandle) -> Result<Option<PickedContactView>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || app.platform().pick_contact())
        .await
        .map_err(failed)?
        .map_err(failed)?;
    Ok(picked.map(|contact| picked_view(&contact.name, &contact.number, &contact.country)))
}

#[tauri::command]
pub async fn core_invite(
    whatsapp: Option<String>,
    text: String,
    app: AppHandle,
) -> Result<(), String> {
    let link = whatsapp.and_then(|number| ft_contacts::invite::whatsapp_link(&number, &text));
    let platform = app.platform();
    deliver(
        link,
        |link| platform.open_whatsapp(link).unwrap_or(false),
        || platform.share_text(&text).map_err(failed),
    )
}

#[tauri::command]
pub async fn core_contact(
    contact: String,
    client: State<'_, Client>,
) -> Result<ContactView, String> {
    let core = client.core().await?;
    let stored = core
        .store()
        .contact(&contact)
        .await
        .map_err(failed)?
        .ok_or("unknown contact")?;
    Ok(ContactView {
        fingerprint: core.fingerprint(&contact).await.map_err(failed)?,
        id: stored.device_id,
        name: stored.name,
        mailbox: stored.mailbox,
        blocked: stored.blocked,
        keep_for: stored.keep_for,
        burn_after_read: stored.burn_after_read,
        rules: stored.rules.into(),
    })
}

#[tauri::command]
pub async fn core_set_rules(
    contact: String,
    rules: RulesView,
    client: State<'_, Client>,
) -> Result<(), String> {
    client
        .core()
        .await?
        .set_rules(&contact, rules.into())
        .await
        .map_err(failed)
}

/// seconds, 0 = forever / never
#[tauri::command]
pub async fn core_set_history(
    contact: String,
    keep_for: i64,
    burn_after_read: i64,
    client: State<'_, Client>,
) -> Result<(), String> {
    client
        .core()
        .await?
        .set_history(&contact, keep_for, burn_after_read)
        .await
        .map_err(failed)
}

#[tauri::command]
pub async fn core_block(
    contact: String,
    blocked: bool,
    client: State<'_, Client>,
) -> Result<(), String> {
    client
        .core()
        .await?
        .block(&contact, blocked)
        .await
        .map_err(failed)
}

#[tauri::command]
pub async fn core_rename(
    contact: String,
    name: String,
    client: State<'_, Client>,
) -> Result<(), String> {
    client
        .core()
        .await?
        .rename_contact(&contact, &name)
        .await
        .map_err(failed)
}

#[tauri::command]
pub async fn core_remove_contact(contact: String, client: State<'_, Client>) -> Result<(), String> {
    client
        .core()
        .await?
        .remove_contact(&contact)
        .await
        .map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picked_contact_reaches_the_page_as_a_first_name_and_a_whatsapp_number() {
        let view = serde_json::to_value(picked_view("María García", "612 345 678", "ES")).unwrap();
        assert_eq!(
            view,
            serde_json::json!({ "name": "María", "whatsapp": "34612345678" })
        );
        let unusable = serde_json::to_value(picked_view("+34 612", "call me", "ES")).unwrap();
        assert_eq!(
            unusable,
            serde_json::json!({ "name": "", "whatsapp": null })
        );
    }

    #[test]
    fn an_invitation_goes_to_whatsapp_when_it_opens() {
        let shared = std::cell::Cell::new(false);
        let opened = std::cell::RefCell::new(Vec::new());
        let sent = deliver(
            Some("https://wa.me/34612345678?text=Hi".to_owned()),
            |link| {
                opened.borrow_mut().push(link.to_owned());
                true
            },
            || {
                shared.set(true);
                Ok(())
            },
        );
        assert_eq!(sent, Ok(()));
        assert_eq!(*opened.borrow(), ["https://wa.me/34612345678?text=Hi"]);
        assert!(!shared.get());
    }

    #[test]
    fn an_invitation_goes_to_the_share_sheet_without_whatsapp_or_without_a_number() {
        for (link, whatsapp_opens) in [
            (Some("https://wa.me/34612345678?text=Hi".to_owned()), false),
            (None, true),
        ] {
            let shared = std::cell::Cell::new(false);
            let sent = deliver(
                link,
                |_| whatsapp_opens,
                || {
                    shared.set(true);
                    Ok(())
                },
            );
            assert_eq!(sent, Ok(()));
            assert!(shared.get());
        }
        let refused = deliver(
            None,
            |_| true,
            || Err("not available on this platform".to_owned()),
        );
        assert_eq!(refused, Err("not available on this platform".to_owned()));
    }

    #[test]
    fn a_contact_view_carries_its_rules() {
        let rules = ft_storage::ContactRules {
            muted: true,
            accepts_chat: false,
            accepts_calls: true,
            receipts: false,
            typing: false,
        };
        let view = serde_json::to_value(RulesView::from(rules)).unwrap();
        assert_eq!(
            view,
            serde_json::json!({ "muted": true, "acceptsChat": false, "acceptsCalls": true, "receipts": false, "typing": false })
        );
    }

    #[test]
    fn rules_without_the_typing_switch_keep_it_on() {
        let view: RulesView = serde_json::from_value(serde_json::json!({ "muted": false, "acceptsChat": true, "acceptsCalls": true, "receipts": true })).unwrap();
        assert!(ft_storage::ContactRules::from(view).typing);
        let view: RulesView = serde_json::from_value(serde_json::json!({ "muted": false, "acceptsChat": true, "acceptsCalls": true, "receipts": true, "typing": false })).unwrap();
        assert!(!ft_storage::ContactRules::from(view).typing);
    }
}
