use anyhow::{ensure, Result};

use crate::native_calls::{BOARD, VIEWER};
use crate::Core;

impl Core {
    pub async fn may_use(&self, kind: ft_plugins::Kind) -> Result<bool> {
        Ok(kind == ft_plugins::Kind::Game || self.access().await?.may(ft_billing::Doing::UseTool))
    }

    pub async fn may_use_plugin(&self, id: &str, kind: ft_plugins::Kind) -> Result<bool> {
        if self.may_use(kind).await? {
            return Ok(true);
        }
        if ![BOARD, VIEWER].contains(&id) {
            return Ok(false);
        }
        Ok(self.following(id).await? && self.plugin_manifest(id).is_ok_and(|manifest| manifest.permissions.live))
    }

    pub(crate) async fn usable(&self, kind: ft_plugins::Kind) -> Result<()> {
        ensure!(self.may_use(kind).await?, crate::NEEDS_SUBSCRIPTION);
        Ok(())
    }

    pub async fn open_plugin(&self, id: &str) -> Result<()> {
        let kind = self.plugin_manifest(id)?.kind;
        ensure!(self.may_use_plugin(id, kind).await?, crate::NEEDS_SUBSCRIPTION);
        Ok(())
    }
}
