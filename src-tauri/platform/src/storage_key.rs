use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Serialize, Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct KeyBytes {
    value: String,
}

impl<R: Runtime> Platform<R> {
    pub fn seal_key(&self, key: &[u8; 32]) -> Result<Vec<u8>> {
        let sealed = self.ask(
            "sealKey",
            KeyBytes {
                value: BASE64.encode(key),
            },
        )?;
        BASE64
            .decode(sealed.value)
            .map_err(|_| crate::Error::Corrupt)
    }

    pub fn open_key(&self, sealed: &[u8]) -> Result<[u8; 32]> {
        let key = self.ask(
            "openKey",
            KeyBytes {
                value: BASE64.encode(sealed),
            },
        )?;
        BASE64
            .decode(key.value)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(crate::Error::Corrupt)
    }

    /// The iOS Keychain item outlives the app, so erasing has to delete it.
    pub fn forget_key(&self) -> Result<()> {
        #[cfg(mobile)]
        {
            self.run("forgetKey", ())
        }
        #[cfg(not(mobile))]
        {
            Ok(())
        }
    }

    #[cfg(mobile)]
    fn ask(&self, command: &str, args: KeyBytes) -> Result<KeyBytes> {
        Ok(self.handle.run_mobile_plugin::<KeyBytes>(command, args)?)
    }

    #[cfg(not(mobile))]
    fn ask(&self, _command: &str, _args: KeyBytes) -> Result<KeyBytes> {
        Err(crate::Error::Unsupported)
    }
}
