use pyo3::{PyObject, Python};
use tokio::{sync::Mutex, task};
use tracing::{debug, error};

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
struct AuthEntry {
    authorized: bool,
    expires_at: Instant,
}

#[derive(Clone, Debug)]
pub struct AuthCache {
    inner: Arc<RwLock<HashMap<String, AuthEntry>>>,
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl Default for AuthCache {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthCache {
    pub fn new() -> Self {
        AuthCache {
            inner: Arc::new(RwLock::new(HashMap::new())),
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn get_or_validate<F, Fut, E>(
        &self,
        key: &str,
        ttl: Duration,
        validator_fn: F,
    ) -> Result<bool, E>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<bool, E>> + Send,
        E: std::fmt::Debug,
    {
        if let Some(entry) = {
            let map = self.inner.read().unwrap();
            map.get(key).cloned()
        } {
            if Instant::now() < entry.expires_at {
                debug!("Cache hit for key.");
                return Ok(entry.authorized);
            }
        }
        debug!("Cache miss for key. Validating authorization...");
        let key_lock = {
            let mut locks_map = self.locks.lock().await;
            locks_map
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = key_lock.lock().await;

        if let Some(entry) = {
            let map = self.inner.read().unwrap();
            map.get(key).cloned()
        } {
            if Instant::now() < entry.expires_at {
                return Ok(entry.authorized);
            }
        }

        let decision = validator_fn().await?;

        {
            let mut map = self.inner.write().unwrap();
            map.insert(
                key.to_string(),
                AuthEntry {
                    authorized: decision,
                    expires_at: Instant::now() + ttl,
                },
            );
        }
        debug!("Authorization cache updated for key.");
        Ok(decision)
    }

    pub fn insert(&self, key: String, authorized: bool, ttl: Duration) {
        let entry = AuthEntry {
            authorized,
            expires_at: Instant::now() + ttl,
        };
        let mut map = self.inner.write().unwrap();
        map.insert(key, entry);
    }

    pub fn invalidate(&self, key: &str) {
        let mut map = self.inner.write().unwrap();
        map.remove(key);
    }
}

pub async fn validate_request(
    token: &str,
    bucket: &str,
    callback: PyObject,
) -> Result<bool, String> {
    let token = token.to_string();
    let bucket = bucket.to_string();

    let authorized = task::spawn_blocking(move || {
        Python::with_gil(
            |py| match callback.call1(py, (token.as_str(), bucket.as_str())) {
                Ok(result_obj) => result_obj
                    .extract::<bool>(py)
                    .map_err(|_| "Failed to extract boolean".to_string()),
                Err(e) => {
                    error!("Python callback error: {:?}", e);
                    Err("Inner Python exception".to_string())
                }
            },
        )
    })
    .await
    .map_err(|e| format!("Join error: {:?}", e))??;

    Ok(authorized)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn auth_cache_get_or_validate_behaviors() {
        let cache = AuthCache::new();
        let key = "auth_key";

        let calls = Arc::new(Mutex::new(0));
        let validator = {
            let calls = Arc::clone(&calls);
            move || {
                let calls = Arc::clone(&calls);
                async move {
                    let mut calls_lock = calls.lock().await;
                    *calls_lock += 1;
                    Ok::<bool, std::convert::Infallible>(true)
                }
            }
        };
        let res1 = cache.get_or_validate(key, Duration::from_secs(1), validator).await.unwrap();
        assert!(res1);
        assert_eq!(*calls.lock().await, 1);

        // second call within TTL: cache hit, no new call
        let res2 = cache.get_or_validate(key, Duration::from_secs(1), {
            let calls = Arc::clone(&calls);
            move || {
                let calls = Arc::clone(&calls);
                async move {
                    let mut calls_lock = calls.lock().await;
                    *calls_lock += 1;
                    Ok::<bool, std::convert::Infallible>(false)
                }
            }
        }).await.unwrap();
        assert!(res2);
        assert_eq!(*calls.lock().await, 1);

        // wait for expiry
        tokio::time::sleep(Duration::from_secs(2)).await;
        let res3 = cache.get_or_validate(key, Duration::from_secs(1), || async move { Ok::<bool, std::convert::Infallible>(false) }).await.unwrap();
        assert!(!res3);
    }
}