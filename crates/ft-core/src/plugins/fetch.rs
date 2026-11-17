use anyhow::{ensure, Result};

use crate::web::{host_of, Fetch, WebAnswer, WebRequest};
use crate::Core;

const FETCH_LIMIT: u64 = 8 * 1024 * 1024;

impl Core {
    // This host check is the real one, the frame's CSP is only a second lock. No identity, key or cookie goes along.
    pub async fn plugin_fetch(
        &self,
        id: &str,
        request: WebRequest,
        fetch: &dyn Fetch,
    ) -> Result<WebAnswer> {
        let host = host_of(&request.url)?;
        let granted = self.granted_to(id).await?;
        ensure!(
            granted
                .network
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(&host)),
            "{id} may not reach {host}"
        );
        fetch.call(&request, FETCH_LIMIT).await
    }
}
