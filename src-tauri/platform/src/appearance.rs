use serde::Serialize;
use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Serialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
struct SystemBars {
    dark: bool,
}

#[derive(Serialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
struct Appearance<'a> {
    appearance: &'a str,
}

impl<R: Runtime> Platform<R> {
    pub fn set_system_bars(&self, dark: bool) -> Result<()> {
        #[cfg(target_os = "android")]
        {
            self.run("setSystemBars", SystemBars { dark })
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = dark;
            Ok(())
        }
    }

    pub fn set_appearance(&self, appearance: &str) -> Result<()> {
        #[cfg(target_os = "android")]
        {
            self.run("setAppearance", Appearance { appearance })
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = appearance;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_bars_travel_as_kotlin_reads_them() {
        assert_eq!(
            serde_json::to_value(SystemBars { dark: true }).unwrap(),
            serde_json::json!({ "dark": true })
        );
        assert_eq!(
            serde_json::to_value(SystemBars { dark: false }).unwrap(),
            serde_json::json!({ "dark": false })
        );
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_has_no_system_bars_to_tell() {
        let platform = Platform::<tauri::Wry> {
            _runtime: std::marker::PhantomData,
        };
        assert!(platform.set_system_bars(true).is_ok());
        assert!(platform.set_system_bars(false).is_ok());
    }

    #[test]
    fn the_appearance_travels_as_kotlin_reads_it() {
        for appearance in ["system", "dark", "light"] {
            assert_eq!(
                serde_json::to_value(Appearance { appearance }).unwrap(),
                serde_json::json!({ "appearance": appearance })
            );
        }
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_keeps_no_appearance() {
        let platform = Platform::<tauri::Wry> {
            _runtime: std::marker::PhantomData,
        };
        assert!(platform.set_appearance("light").is_ok());
    }
}
