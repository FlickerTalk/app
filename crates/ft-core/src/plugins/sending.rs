use std::path::Path;

use anyhow::{ensure, Result};
use ft_plugins::Sending;

use crate::Core;

impl Core {
    pub async fn plugin_sending(&self, id: &str) -> Result<Sending> {
        Ok(self.granted_to(id).await?.send)
    }

    pub async fn plugin_may_propose(&self, id: &str) -> Result<bool> {
        Ok(matches!(
            self.plugin_sending(id).await?,
            Sending::Propose | Sending::Auto
        ))
    }

    // only with `auto`, which is never the default
    pub async fn plugin_send_file(
        &self,
        id: &str,
        contact: &str,
        path: &Path,
        name: &str,
        mime: &str,
    ) -> Result<String> {
        ensure!(
            self.plugin_sending(id).await? == Sending::Auto,
            "{id} may not send by itself"
        );
        self.send_file(contact, path, name, mime).await
    }
}
