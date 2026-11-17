//! Nothing is installed unless the catalogue signed it, and installing grants nothing.

use anyhow::{ensure, Result};

pub use ft_storage::ChatHook;

use crate::Core;

mod access;
mod catalogue;
mod conversations;
mod fetch;
mod grants;
mod hooks;
mod install;
mod live;
mod location;
mod reminders;
mod sending;
mod storage;
mod translate;

pub use hooks::NOTE_MAX_CHARS;
pub use install::{files_of, InstalledPlugin};
pub use live::LIVE_LIMIT;
pub use location::{Fix, Locator};
pub use storage::RECORD_VALUE;
pub use translate::{PairStatus, Translator, TRANSLATE_MAX_CHARS};

pub const CATALOGUE_HOME: &str = "https://flickertalk.com/plugins";

const RECORD_KEY: usize = 128;
pub const CORE_VERSION: &str = "1.8.0";

impl Core {
    // A closed session's data goes to nobody, even a caller that names it
    fn place<'a>(&self, session: Option<&'a str>) -> Result<Option<&'a str>> {
        if let Some(session) = session {
            ensure!(self.is_session_open(session), "that session is not open");
        }
        Ok(session)
    }
}
