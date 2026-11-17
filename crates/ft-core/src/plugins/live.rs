use anyhow::{ensure, Result};
use ft_protocol::{Body, Packet};
use ft_storage::Contact;

use crate::{Core, Event};

pub const LIVE_LIMIT: usize = 48 * 1024;

impl Core {
    // Direct connection only, never the mailbox
    pub async fn plugin_live_send(&self, id: &str, contact: &str, data: Vec<u8>) -> Result<bool> {
        ensure!(self.granted_to(id).await?.live, "{id} may not talk to the other side");
        ensure!(data.len() <= LIVE_LIMIT, "a plugin may not say that much at once");
        let contact = self.contact(contact).await?;
        ensure!(contact.accepted && !contact.blocked, "that is not a contact of yours");
        self.transmit_direct(&contact, &Packet::new(Body::PluginEvent { plugin: id.to_owned(), data })).await
    }

    pub(crate) async fn plugin_event_received(&self, contact: &Contact, plugin: String, data: Vec<u8>) -> Result<()> {
        if !contact.accepted || data.len() > LIVE_LIMIT {
            return Ok(());
        }
        let granted = match self.granted_to(&plugin).await {
            Ok(granted) => granted.live,
            Err(_) => false,
        };
        if granted {
            let _ = self.events.send(Event::PluginEvent { plugin, contact: contact.device_id.clone(), data });
        }
        Ok(())
    }
}
