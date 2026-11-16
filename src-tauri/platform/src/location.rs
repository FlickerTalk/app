use serde::Deserialize;
use tauri::Runtime;

use crate::{Platform, Result};

/// degrees, accuracy in metres, `at` in ms since the epoch
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    pub accuracy: f64,
    pub at: i64,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Located {
    found: bool,
    #[serde(default)]
    lat: f64,
    #[serde(default)]
    lon: f64,
    #[serde(default)]
    accuracy: f64,
    #[serde(default)]
    at: f64,
}

#[cfg_attr(not(mobile), allow(dead_code))]
impl Located {
    fn location(self) -> Option<Location> {
        self.found.then_some(Location {
            lat: self.lat,
            lon: self.lon,
            accuracy: self.accuracy,
            at: self.at as i64,
        })
    }
}

impl<R: Runtime> Platform<R> {
    /// Blocks until the phone answers (up to ~15 s), don't call it on the main thread.
    pub fn current_location(&self) -> Result<Option<Location>> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Located>("currentLocation", ())?
                .location())
        }
        #[cfg(not(mobile))]
        {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_location_comes_back_as_swift_and_kotlin_resolve_it() {
        let found: Located = serde_json::from_value(serde_json::json!({
            "found": true, "lat": 40.41678, "lon": -3.70379, "accuracy": 35.0, "at": 1_790_000_000_000.0
        }))
        .unwrap();
        assert_eq!(
            found.location(),
            Some(Location {
                lat: 40.41678,
                lon: -3.70379,
                accuracy: 35.0,
                at: 1_790_000_000_000
            })
        );
        let nothing: Located =
            serde_json::from_value(serde_json::json!({ "found": false })).unwrap();
        assert_eq!(nothing.location(), None);
    }
}
