use crate::cache;
use crate::config::Provider;
use crate::error::WeatherError;
use crate::weather::normalizer::WeatherNormalizer;
use crate::weather::provider::WeatherProvider;
use crate::weather::types::{WeatherData, WeatherLocation, WeatherUnits};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tundra_watchdog::AppWatchdog;

#[derive(Clone)]
pub struct WeatherClient {
    provider: Arc<dyn WeatherProvider>,
    cache: Arc<RwLock<Option<CachedWeather>>>,
    cache_duration: Duration,
    watchdog: Option<AppWatchdog>,
}

struct CachedWeather {
    data: WeatherData,
    fetched_at: Instant,
}

impl WeatherClient {
    pub fn new(provider: Arc<dyn WeatherProvider>, cache_duration: Duration) -> Self {
        Self {
            provider,
            cache: Arc::new(RwLock::new(None)),
            cache_duration,
            watchdog: None,
        }
    }

    pub fn new_managed(
        provider: Arc<dyn WeatherProvider>,
        cache_duration: Duration,
        watchdog: AppWatchdog,
    ) -> Self {
        Self {
            provider,
            cache: Arc::new(RwLock::new(None)),
            cache_duration,
            watchdog: Some(watchdog),
        }
    }

    pub async fn get_current_weather(
        &self,
        location: &WeatherLocation,
        units: &WeatherUnits,
        provider: Provider,
    ) -> Result<WeatherData, WeatherError> {
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.as_ref()
                && cached.fetched_at.elapsed() < self.cache_duration
            {
                return Ok(cached.data.clone());
            }
        }

        if let Some(cached_data) =
            cache::load_cached_weather(location.latitude, location.longitude, provider).await
            && std::env::var("CACHE_DISABLED").is_err()
        // Should've done this sooner
        {
            let mut cache = self.cache.write().await;
            *cache = Some(CachedWeather {
                data: cached_data.clone(),
                fetched_at: Instant::now(),
            });
            return Ok(cached_data);
        }

        self.fetch_remote(location, units, provider, false).await
    }

    pub(crate) async fn refresh_current_weather_for_startup(
        &self,
        location: &WeatherLocation,
        units: &WeatherUnits,
        provider: Provider,
    ) -> Result<WeatherData, WeatherError> {
        self.fetch_remote(location, units, provider, true).await
    }

    /// Fetches fresh provider data even when memory or disk cache entries are
    /// still valid.
    pub async fn refresh_current_weather(
        &self,
        location: &WeatherLocation,
        units: &WeatherUnits,
        provider: Provider,
    ) -> Result<WeatherData, WeatherError> {
        self.fetch_remote(location, units, provider, false).await
    }

    async fn fetch_remote(
        &self,
        location: &WeatherLocation,
        units: &WeatherUnits,
        provider: Provider,
        await_cache_write: bool,
    ) -> Result<WeatherData, WeatherError> {
        let response = self.provider.get_current_weather(location, units).await?;

        let data = WeatherNormalizer::normalize(response);

        {
            let mut cache = self.cache.write().await;
            *cache = Some(CachedWeather {
                data: data.clone(),
                fetched_at: Instant::now(),
            });
        }

        if await_cache_write {
            let _ = cache::save_weather_cache_now(
                &data,
                location.latitude,
                location.longitude,
                provider,
            )
            .await;
        } else {
            let _ = match self.watchdog.as_ref() {
                Some(watchdog) => cache::save_weather_cache_managed(
                    watchdog,
                    &data,
                    location.latitude,
                    location.longitude,
                    provider,
                ),
                None => cache::save_weather_cache(
                    &data,
                    location.latitude,
                    location.longitude,
                    provider,
                ),
            };
        }

        Ok(data)
    }

    #[allow(dead_code)]
    pub async fn invalidate_cache(&self) {
        let mut cache = self.cache.write().await;
        *cache = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::provider::WeatherProviderResponse;
    use crate::weather::provider::open_meteo::OpenMeteoProvider;
    use crate::weather::types::CelestialEvents;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct CountingProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl WeatherProvider for CountingProvider {
        async fn get_current_weather(
            &self,
            _location: &WeatherLocation,
            _units: &WeatherUnits,
        ) -> Result<WeatherProviderResponse, WeatherError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(WeatherProviderResponse {
                weather_code: 0,
                temperature: 10.0,
                precipitation: 0.0,
                wind_speed: 3.0,
                wind_direction: 90.0,
                sun: CelestialEvents::from_bool(true),
                moon_phase: Some(0.5),
                timestamp: "2026-07-18T00:00".to_string(),
                attribution: "test".to_string(),
            })
        }

        fn get_attribution(&self) -> &'static str {
            "test"
        }
    }

    #[tokio::test]
    async fn test_cache_invalidation() {
        let provider = Arc::new(OpenMeteoProvider::new());
        let client = WeatherClient::new(provider, Duration::from_secs(60));

        client.invalidate_cache().await;

        let cache = client.cache.read().await;
        assert!(cache.is_none());
    }

    #[tokio::test]
    async fn explicit_refresh_bypasses_a_fresh_memory_cache() {
        let calls = Arc::new(AtomicUsize::new(0));
        let client = WeatherClient::new(
            Arc::new(CountingProvider {
                calls: calls.clone(),
            }),
            Duration::from_secs(300),
        );
        let location = WeatherLocation {
            latitude: 31.23,
            longitude: 121.47,
            elevation: None,
        };

        client
            .refresh_current_weather(&location, &WeatherUnits::default(), Provider::OpenMeteo)
            .await
            .expect("first refresh");
        client
            .refresh_current_weather(&location, &WeatherUnits::default(), Provider::OpenMeteo)
            .await
            .expect("second refresh");

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
