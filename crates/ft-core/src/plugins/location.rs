use anyhow::{ensure, Result};
use async_trait::async_trait;
use serde::Serialize;

use crate::Core;

// degrees (WGS 84), accuracy in metres, `at` in ms
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fix {
    pub lat: f64,
    pub lon: f64,
    pub accuracy: f64,
    pub at: i64,
}

#[async_trait]
pub trait Locator: Send + Sync {
    async fn locate(&self) -> Result<Option<Fix>>;
}

const LOCATION_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

impl Fix {
    fn is_a_place(&self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
            && self.accuracy.is_finite()
            && self.accuracy >= 0.0
    }
}

async fn located(locator: &dyn Locator) -> Option<Fix> {
    match tokio::time::timeout(LOCATION_WAIT, locator.locate()).await {
        Ok(Ok(fix)) => fix.filter(Fix::is_a_place),
        _ => None,
    }
}

impl Core {
    // Pass the fix through, never keep or log it
    pub async fn plugin_location(&self, id: &str, locator: &dyn Locator) -> Result<Option<Fix>> {
        ensure!(
            self.granted_to(id).await?.location,
            "{id} may not ask where the phone is"
        );
        Ok(located(locator).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Silent;

    #[async_trait]
    impl Locator for Silent {
        async fn locate(&self) -> Result<Option<Fix>> {
            std::future::pending().await
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_phone_that_never_answers_is_nothing_after_a_while() {
        let started = tokio::time::Instant::now();
        assert_eq!(located(&Silent).await, None);
        let waited = started.elapsed();
        assert!(
            waited >= std::time::Duration::from_secs(15)
                && waited <= std::time::Duration::from_secs(30),
            "waited {waited:?}"
        );
    }
}
