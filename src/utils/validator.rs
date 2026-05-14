use dashmap::DashMap;
use lru::LruCache;
use pyo3::{PyObject, Python};
use tokio::{sync::Mutex, task};
use tracing::{debug, error};

use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
struct AuthEntry {
    authorized: bool,
    expires_at: Instant,
}

/// Default maximum number of distinct `(access_key, bucket, method)` entries
/// held in the authorization cache.  Once the limit is reached the
/// least-recently-used entry is evicted automatically — no background sweep
/// task needed.
pub const AUTH_CACHE_DEFAULT_CAPACITY: usize = 1024;

/// A time-bounded, capacity-limited LRU cache for authorization decisions.
///
/// Wraps arbitrary async validator functions so that the (potentially
/// expensive) Python callback is only invoked once per `(access_key, bucket,
/// method)` tuple within the configured TTL window.
///
/// Memory is bounded by `capacity`: when the limit is reached the
/// least-recently-used entry is evicted automatically (TODO perf-4: done).
///
/// Concurrent cache misses for the **same** key are serialised via a per-key
/// [`Mutex`] to avoid thundering-herd stampedes.  Misses for **different** keys
/// are fully concurrent — the per-key lock map is backed by a [`DashMap`] so
/// no single global lock is held (TODO perf-3: done).
#[derive(Clone, Debug)]
pub struct AuthCache {
    inner: Arc<RwLock<LruCache<String, AuthEntry>>>,
    /// Per-key mutex map — DashMap so concurrent misses for different keys
    /// never contend on a shared lock.
    locks: Arc<DashMap<String, Arc<Mutex<()>>>>,
}

impl Default for AuthCache {
    fn default() -> Self {
        Self::new(AUTH_CACHE_DEFAULT_CAPACITY)
    }
}

impl AuthCache {
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity)
            .unwrap_or(NonZeroUsize::new(AUTH_CACHE_DEFAULT_CAPACITY).unwrap());
        AuthCache {
            inner: Arc::new(RwLock::new(LruCache::new(cap))),
            locks: Arc::new(DashMap::new()),
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
            // LruCache::peek does not promote the entry, preserving LRU order
            // on a read-only hit — no write lock needed.
            let map = self.inner.read().unwrap();
            map.peek(key).cloned()
        } && Instant::now() < entry.expires_at
        {
            debug!("Cache hit for key.");
            return Ok(entry.authorized);
        }
        debug!("Cache miss for key. Validating authorization...");

        // Obtain (or lazily create) the per-key mutex without holding a global
        // lock across the async validation call.
        let key_lock = self
            .locks
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = key_lock.lock().await;

        // Double-checked locking: another task may have populated the entry
        // while we were waiting for the per-key mutex.
        if let Some(entry) = {
            let map = self.inner.read().expect("lock poisoned");
            map.peek(key).cloned()
        } && Instant::now() < entry.expires_at
        {
            return Ok(entry.authorized);
        }

        let decision = validator_fn().await?;

        {
            let mut map = self.inner.write().expect("lock poisoned");
            map.put(
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

    /// Pre-populate the cache with a known decision for `key`.
    pub fn insert(&self, key: String, authorized: bool, ttl: Duration) {
        let entry = AuthEntry {
            authorized,
            expires_at: Instant::now() + ttl,
        };
        let mut map = self.inner.write().expect("lock poisoned");
        map.put(key, entry);
    }

    /// Evict the cached entry for `key`, forcing re-validation on the next request.
    pub fn invalidate(&self, key: &str) {
        let mut map = self.inner.write().expect("lock poisoned");
        map.pop(key);
    }
}

/// Invoke the Python validator callback for a single request.
///
/// `takes_request` must be pre-computed once (e.g. at server startup via
/// [`callable_accepts_request`]) and passed here to avoid re-running
/// `inspect.signature` on every cache miss.
pub async fn validate_request(
    token: &str,
    bucket: &str,
    request: &HashMap<String, String>,
    callback: PyObject,
    takes_request: bool,
) -> Result<bool, String> {
    let token = token.to_string();
    let bucket = bucket.to_string();

    let req = request
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<HashMap<String, String>>();

    debug!("request details sent to Python callable: {:?}", &req);

    let authorized = if takes_request {
        task::spawn_blocking(move || {
            Python::with_gil(|py| {
                match callback.call1(py, (token.as_str(), bucket.as_str(), &req)) {
                    Ok(result_obj) => result_obj
                        .extract::<bool>(py)
                        .map_err(|_| "Failed to extract boolean".to_string()),
                    Err(e) => {
                        error!("Python callback error: {:?}", e);
                        Err("Inner Python exception".to_string())
                    }
                }
            })
        })
        .await
        .map_err(|e| format!("Join error: {:?}", e))??
    } else {
        task::spawn_blocking(move || {
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
        .map_err(|e| format!("Join error: {:?}", e))??
    };

    Ok(authorized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn auth_cache_get_or_validate_behaviors() {
        let cache = AuthCache::new(AUTH_CACHE_DEFAULT_CAPACITY);
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
        let res1 = cache
            .get_or_validate(key, Duration::from_secs(1), validator)
            .await
            .unwrap();
        assert!(res1);
        assert_eq!(*calls.lock().await, 1);

        // second call within TTL: cache hit, no new call
        let res2 = cache
            .get_or_validate(key, Duration::from_secs(1), {
                let calls = Arc::clone(&calls);
                move || {
                    let calls = Arc::clone(&calls);
                    async move {
                        let mut calls_lock = calls.lock().await;
                        *calls_lock += 1;
                        Ok::<bool, std::convert::Infallible>(false)
                    }
                }
            })
            .await
            .unwrap();
        assert!(res2);
        assert_eq!(*calls.lock().await, 1);

        // wait for expiry
        tokio::time::sleep(Duration::from_secs(2)).await;
        let res3 = cache
            .get_or_validate(key, Duration::from_secs(1), || async move {
                Ok::<bool, std::convert::Infallible>(false)
            })
            .await
            .unwrap();
        assert!(!res3);
    }
}
