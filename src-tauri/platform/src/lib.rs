use serde::Serialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

mod appearance;
mod authorize;
mod billing;
mod calls;
mod files;
mod location;
mod opened_links;
mod push;
mod reminders;
mod storage_key;
mod translate;
mod video;

pub use calls::{parse_call_event, CallRoute, NativeCallEvent};
pub use files::PickedFile;
pub use location::Location;
pub use translate::TranslateStatus;
pub use video::{VideoLayout, VideoRect};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not available on this platform")]
    Unsupported,
    #[error("the key store gave something that is not a key")]
    Corrupt,
    #[cfg(mobile)]
    #[error(transparent)]
    Invoke(#[from] tauri::plugin::mobile::PluginInvokeError),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Platform<R: Runtime> {
    #[cfg(mobile)]
    handle: tauri::plugin::PluginHandle<R>,
    #[cfg(not(mobile))]
    _runtime: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> Platform<R> {
    fn call(&self, command: &str, args: impl Serialize) -> Result<()> {
        #[cfg(mobile)]
        {
            self.run(command, args)
        }
        #[cfg(not(mobile))]
        {
            let _ = (command, args);
            Ok(())
        }
    }

    /// Tauri's own restart only exits on Android.
    pub fn restart_app(&self) -> Result<()> {
        self.run("restartApp", ())
    }

    #[cfg(mobile)]
    fn run(&self, command: &str, args: impl Serialize) -> Result<()> {
        self.handle.run_mobile_plugin::<()>(command, args)?;
        Ok(())
    }

    #[cfg(not(mobile))]
    fn run(&self, _command: &str, _args: impl Serialize) -> Result<()> {
        Err(Error::Unsupported)
    }
}

pub trait PlatformExt<R: Runtime> {
    fn platform(&self) -> &Platform<R>;
}

impl<R: Runtime, T: Manager<R>> PlatformExt<R> for T {
    fn platform(&self) -> &Platform<R> {
        self.state::<Platform<R>>().inner()
    }
}

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_ft_platform);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("ft-platform")
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            let platform = Platform {
                handle: _api
                    .register_android_plugin("com.flickertalk.platform", "PlatformPlugin")?,
            };
            #[cfg(target_os = "ios")]
            let platform = Platform {
                handle: _api.register_ios_plugin(init_plugin_ft_platform)?,
            };
            #[cfg(not(mobile))]
            let platform = Platform::<R> {
                _runtime: std::marker::PhantomData,
            };
            app.manage(platform);
            Ok(())
        })
        .build()
}
