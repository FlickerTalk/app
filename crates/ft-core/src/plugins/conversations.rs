use anyhow::{ensure, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ft_storage::PluginRef;

use crate::Core;

// Changing this changes every chat id plugins have stored
const PLUGIN_CHAT_CONTEXT: &str = "flickertalk 2026-10-02 plugin chat id";

impl Core {
    // Opaque, tells the plugin nothing about the contact
    pub async fn plugin_ref(&self, id: &str, message_id: &str) -> Result<String> {
        if let Some(reference) = self.store.plugin_ref_for(id, message_id).await? {
            return Ok(reference);
        }
        let conversation = match self.store.message(message_id).await? {
            Some(message) => message.contact,
            None => {
                self.store
                    .circle_message(message_id)
                    .await?
                    .context("that message is not here")?
                    .circle
            }
        };
        let reference = format!("ref_{}", ft_protocol::MessageId::new());
        self.store
            .add_plugin_ref(&reference, id, &conversation, message_id)
            .await?;
        Ok(reference)
    }

    pub async fn plugin_ref_target(
        &self,
        id: &str,
        session: Option<&str>,
        reference: &str,
    ) -> Result<Option<PluginRef>> {
        let session = self.place(session)?;
        let Some(target) = self.store.plugin_ref(reference).await? else {
            return Ok(None);
        };
        if target.plugin != id {
            return Ok(None);
        }
        if let Some(circle) = self.store.circle(&target.contact).await? {
            let here = circle.session.as_deref() == session
                && self
                    .store
                    .circle_message(&target.message_id)
                    .await?
                    .is_some();
            return Ok(here.then_some(target));
        }
        let contact = self.store.contact(&target.contact).await?;
        if contact.is_none_or(|contact| contact.blocked || contact.session.as_deref() != session) {
            return Ok(None);
        }
        if self.store.message(&target.message_id).await?.is_none() {
            return Ok(None);
        }
        Ok(Some(target))
    }

    // An unreachable conversation must fail exactly like one that doesn't exist
    pub async fn plugin_chat(&self, id: &str, contact: &str) -> Result<String> {
        ensure!(
            self.store.plugin(id).await?.is_some(),
            "{id} is not installed here"
        );
        let open =
            |session: Option<&str>| session.is_none_or(|session| self.is_session_open(session));
        let reachable = match self.store.circle(contact).await? {
            Some(circle) => open(circle.session.as_deref()),
            None => self.store.contact(contact).await?.is_some_and(|contact| {
                contact.accepted && !contact.blocked && open(contact.session.as_deref())
            }),
        };
        ensure!(reachable, "that is not a contact of yours");
        let mut hasher =
            blake3::Hasher::new_keyed(&blake3::derive_key(PLUGIN_CHAT_CONTEXT, &self.key));
        // ids can't contain NUL, so this keeps them apart
        hasher
            .update(id.as_bytes())
            .update(&[0])
            .update(contact.as_bytes());
        Ok(URL_SAFE_NO_PAD.encode(hasher.finalize().as_bytes()))
    }
}
