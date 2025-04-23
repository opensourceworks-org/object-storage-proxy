use std::{collections::HashMap, sync::Arc, time::Instant};
use pyo3::{PyObject, Python};
use tokio::sync::RwLock;

use crate::{get_credential_for_bucket, parsers::cos_map::CosMapItem};

use super::models::BucketCredential;

/// Manages per‑token credential fetching + caching,
/// while preserving the “generic” creds you seeded via CosMapItem.
// #[derive(Clone, Debug)]
pub struct BucketCredProxy {
    /// the original map of “generic” CosMapItems
    base_map: Arc<RwLock<HashMap<String, CosMapItem>>>,
    /// Python callback, if any
    bucket_creds_fetcher: Option<PyObject>,
    /// cache: key = “bucket:token” → (raw_credential_string, fetched_at)
    cache: Arc<RwLock<HashMap<String, (String, Instant)>>>,
}

impl BucketCredProxy {
    pub fn new(
        base_map: Arc<RwLock<HashMap<String, CosMapItem>>>,
        bucket_creds_fetcher: Option<PyObject>,
    ) -> Self {
        BucketCredProxy {
            base_map,
            bucket_creds_fetcher,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Returns a CosMapItem that has *either* its built‑in HMAC/API key,
    /// *or* a token‑scoped credential freshly fetched (and cached).
    pub async fn ensure_for(
        &self,
        bucket: &str,
        token: &str,
    ) -> Result<CosMapItem, Box<dyn std::error::Error + Send + Sync>> {
        let key = format!("{}:{}", bucket, token);

        // 1) Look up generic config
        let base_map = self.base_map.read().await;
        let mut item = base_map
            .get(bucket)
            .cloned()
            .ok_or("No COS config for bucket")?;
        let ttl_secs = item.ttl.unwrap_or(0);

        // If it already has creds, just return it
        if item.has_hmac() || item.has_api_key() {
            return Ok(item);
        }

        // 2) Try token‑scoped cache
        {
            let mut cache = self.cache.write().await;
            if let Some((raw, fetched)) = cache.get(&key) {
                if fetched.elapsed().as_secs() < ttl_secs {
                    // parse & apply
                    return Ok(Self::apply_raw(&mut item, raw.clone()));
                }
                // expired
                cache.remove(&key);
            }
        }

        // 3) Need to fetch
        let fetcher = self
            .bucket_creds_fetcher
            .as_ref()
            .ok_or("missing credentials and no fetcher provided")?;
        let cb = Python::with_gil(|py| fetcher.clone_ref(py));
        let raw = get_credential_for_bucket(&cb, bucket.to_owned(), token.to_owned()).await?;
        
        // 4) cache it
        {
            let mut cache = self.cache.write().await;
            cache.insert(key.clone(), (raw.clone(), Instant::now()));
        }

        // 5) parse & return
        Ok(Self::apply_raw(&mut item, raw))
    }

    /// Consume a raw JSON/string and apply it to a CosMapItem clone
    fn apply_raw(item: &mut CosMapItem, raw: String) -> CosMapItem {
        match BucketCredential::parse(&raw) {
            BucketCredential::Hmac { access_key, secret_key } => {
                item.access_key = Some(access_key);
                item.secret_key = Some(secret_key);
            }
            BucketCredential::ApiKey(k) => {
                item.api_key = Some(k);
            }
        }
        item.clone()
    }
}
