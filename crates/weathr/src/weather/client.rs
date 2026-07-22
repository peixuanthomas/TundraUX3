use crate::cache;
use crate::config::Provider;
use crate::error::WeatherError;
use crate::weather::normalizer::WeatherNormalizer;
use crate::weather::provider::WeatherProvider;
use crate::weather::types::{WeatherData, WeatherLocation, WeatherUnits};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use watchdog::AppWatchdog;

#[derive(Clone)]
pub struct WeatherClient {
    provider: Arc<dyn WeatherProvider>,
    cache: Arc<RwLock<Option<CachedWeather>>>,
    cache_duration: Duration,
    watchdog: Option<AppWatchdog>,
}

struct CachedWeather {
    request: WeatherRequest,
    data: WeatherData,
    fetched_at: Instant,
}

#[derive(Clone, Copy, PartialEq)]
struct WeatherRequest {
    location: WeatherLocation,
    units: WeatherUnits,
    provider: Provider,
}

impl WeatherRequest {
    fn new(location: &WeatherLocation, units: &WeatherUnits, provider: Provider) -> Self {
        Self {
            location: *location,
            units: *units,
            provider,
        }
    }
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
        let request = WeatherRequest::new(location, units, provider);
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.as_ref()
                && cached.request == request
                && cached.fetched_at.elapsed() < self.cache_duration
            {
                return Ok(cached.data.clone());
            }
        }

        if std::env::var_os("CACHE_DISABLED").is_none()
            && let Some(cached_data) = cache::load_cached_weather(*location, *units, provider).await
        {
            let mut cache = self.cache.write().await;
            *cache = Some(CachedWeather {
                request,
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
        let request = WeatherRequest::new(location, units, provider);

        {
            let mut cache = self.cache.write().await;
            *cache = Some(CachedWeather {
                request,
                data: data.clone(),
                fetched_at: Instant::now(),
            });
        }

        if await_cache_write {
            let _ = cache::save_weather_cache_now(&data, *location, *units, provider).await;
        } else {
            let _ = match self.watchdog.as_ref() {
                Some(watchdog) => {
                    cache::save_weather_cache_managed(watchdog, &data, *location, *units, provider)
                }
                None => cache::save_weather_cache(&data, *location, *units, provider),
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

    #[tokio::test]
    async fn memory_cache_is_scoped_to_the_complete_request() {
        let calls = Arc::new(AtomicUsize::new(0));
        let client = WeatherClient::new(
            Arc::new(CountingProvider {
                calls: calls.clone(),
            }),
            Duration::from_secs(300),
        );
        let first_location = WeatherLocation {
            latitude: 12.345_678,
            longitude: 98.765_432,
            elevation: Some(50.0),
        };
        let second_location = WeatherLocation {
            latitude: -23.456_789,
            longitude: -87.654_321,
            elevation: Some(500.0),
        };
        let metric = WeatherUnits::metric();
        let imperial = WeatherUnits::imperial();

        client
            .refresh_current_weather(&first_location, &metric, Provider::OpenMeteo)
            .await
            .expect("prime memory cache");
        client
            .get_current_weather(&first_location, &metric, Provider::OpenMeteo)
            .await
            .expect("matching request uses memory cache");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        client
            .get_current_weather(&second_location, &metric, Provider::OpenMeteo)
            .await
            .expect("different location bypasses memory cache");
        client
            .get_current_weather(&second_location, &imperial, Provider::OpenMeteo)
            .await
            .expect("different units bypass memory cache");
        client
            .get_current_weather(&second_location, &imperial, Provider::MetOffice)
            .await
            .expect("different provider bypasses memory cache");

        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }
}
