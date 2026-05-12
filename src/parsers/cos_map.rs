use std::collections::HashMap;

use pyo3::exceptions::PyKeyError;
use pyo3::prelude::*;
use pyo3::{PyResult, Python};

use crate::credentials::models::BucketCredential;

/// Represents a COS map item with its properties.
///
/// This struct is used to store the configuration for a COS bucket.
/// Each bucket is identified by its name, and the properties include:
/// - `host`: The host address of the COS service.
/// - `port`: The port number for the COS service.
/// - `region`: The region where the COS service is located.
/// - `api_key`: The API key for accessing the COS service (optional).
/// - `access_key`: The access key for the COS service (optional).
/// - `secret_key`: The secret key for the COS service (optional).
/// - `ttl`: The time-to-live for the COS bucket (optional).
///
#[pyclass]
#[derive(Debug, Clone)]
pub struct CosMapItem {
    pub host: String,
    pub port: u16,
    pub region: Option<String>,
    pub api_key: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub ttl: Option<u64>,
    pub tls: Option<bool>,
    pub addressing_style: Option<String>,
}

impl CosMapItem {
    /// Returns `true` if all three HMAC fields are present.
    pub fn has_hmac(&self) -> bool {
        self.access_key.is_some() && self.secret_key.is_some()
    }

    /// Returns `true` if an API‑key is present.
    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// Ensure that **some** credential (HMAC or API key) is populated.
    ///
    /// * If HMAC pair exists → OK
    /// * Else if api_key exists → OK
    /// * Else it calls the supplied async `fetcher(bucket)` which
    ///   should return one of the accepted formats (see [`BucketCredential`]).
    ///   The struct is then updated in‑place.
    pub async fn ensure_credentials<F, Fut>(
        &mut self,
        bucket: &str,
        fetcher: Option<F>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnOnce(String) -> Fut + Send,
        Fut: std::future::Future<Output = Result<String, Box<dyn std::error::Error + Send + Sync>>>
            + Send,
    {
        if self.has_hmac() || self.has_api_key() {
            return Ok(());
        }

        let Some(fetch) = fetcher else {
            return Err("missing credentials and no fetcher provided".into());
        };

        let raw_creds = fetch(bucket.to_owned()).await?;
        match BucketCredential::parse(&raw_creds) {
            BucketCredential::Hmac {
                access_key,
                secret_key,
            } => {
                self.access_key = Some(access_key);
                self.secret_key = Some(secret_key);
            }
            BucketCredential::ApiKey(k) => {
                self.api_key = Some(k);
            }
        }
        Ok(())
    }
}

pub(crate) fn parse_cos_map(
    py: Python,
    cos_dict: &PyObject,
) -> PyResult<HashMap<String, CosMapItem>> {
    let raw_map: HashMap<String, HashMap<String, PyObject>> = cos_dict.extract(py)?;
    let mut map = HashMap::new();

    for (bucket, inner_map) in raw_map {
        let host_obj = inner_map
            .get("host")
            .ok_or_else(|| PyKeyError::new_err("Missing 'host' in COS map entry"))?;
        let host: String = host_obj.extract(py)?;

        let port_obj = inner_map
            .get("port")
            .ok_or_else(|| PyKeyError::new_err("Missing 'port' in COS map entry"))?;
        let port: u16 = port_obj.extract(py)?;

        let region = inner_map.get("region").map(|v| v.extract(py)).transpose()?;

        // Optional: api_key (allow 'api_key' or 'apikey')
        let api_key =
            if let Some(val) = inner_map.get("api_key").or_else(|| inner_map.get("apikey")) {
                Some(val.extract(py)?)
            } else {
                None
            };
        let ttl = if let Some(val) = inner_map
            .get("ttl")
            .or_else(|| inner_map.get("time-to-live"))
        {
            Some(val.extract(py)?)
        } else {
            None
        };

        let tls = inner_map
            .get("tls")
            .or_else(|| inner_map.get("is_tls_enabled"))
            .map(|v| v.extract(py))
            .transpose()?;

        let access_key = inner_map
            .get("access_key")
            .or_else(|| inner_map.get("accessKey"))
            .map(|v| v.extract(py))
            .transpose()?;

        let secret_key = inner_map
            .get("secret_key")
            .or_else(|| inner_map.get("secretKey"))
            .map(|v| v.extract(py))
            .transpose()?;

        let addressing_style = inner_map
            .get("addressing_style")
            .or_else(|| inner_map.get("addressingStyle"))
            .map(|v| v.extract(py))
            .transpose()?;

        map.insert(
            bucket.clone(),
            CosMapItem {
                host,
                port,
                region,
                api_key,
                access_key,
                secret_key,
                ttl,
                tls,
                addressing_style,
            },
        );
    }

    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_item() -> CosMapItem {
        CosMapItem {
            host: "s3.example.com".into(),
            port: 443,
            region: Some("us-east-1".into()),
            api_key: None,
            access_key: None,
            secret_key: None,
            ttl: None,
            tls: Some(true),
            addressing_style: None,
        }
    }

    #[test]
    fn has_hmac_requires_both_keys() {
        let mut item = base_item();
        assert!(!item.has_hmac());

        item.access_key = Some("AK".into());
        assert!(!item.has_hmac(), "only access_key should not satisfy has_hmac");

        item.secret_key = Some("SK".into());
        assert!(item.has_hmac());
    }

    #[test]
    fn has_api_key_reflects_presence() {
        let mut item = base_item();
        assert!(!item.has_api_key());
        item.api_key = Some("my-api-key".into());
        assert!(item.has_api_key());
    }

    #[tokio::test]
    async fn ensure_credentials_noop_when_hmac_present() {
        let mut item = base_item();
        item.access_key = Some("AK".into());
        item.secret_key = Some("SK".into());

        // fetcher should NOT be called
        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called2 = called.clone();
        let fetcher = move |_bucket: String| {
            let c = called2.clone();
            async move {
                c.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok::<String, Box<dyn std::error::Error + Send + Sync>>("unused".into())
            }
        };

        item.ensure_credentials("my-bucket", Some(fetcher))
            .await
            .unwrap();
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn ensure_credentials_noop_when_api_key_present() {
        let mut item = base_item();
        item.api_key = Some("apikey123".into());

        item.ensure_credentials("my-bucket", None::<fn(String) -> std::future::Ready<Result<String, Box<dyn std::error::Error + Send + Sync>>>>)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ensure_credentials_fetches_hmac_when_missing() {
        let mut item = base_item();
        let creds = r#"{"access_key": "FETCHED_AK", "secret_key": "FETCHED_SK"}"#;
        let creds_str = creds.to_string();
        let fetcher = move |_bucket: String| {
            let c = creds_str.clone();
            async move { Ok::<String, Box<dyn std::error::Error + Send + Sync>>(c) }
        };

        item.ensure_credentials("my-bucket", Some(fetcher))
            .await
            .unwrap();

        assert_eq!(item.access_key.as_deref(), Some("FETCHED_AK"));
        assert_eq!(item.secret_key.as_deref(), Some("FETCHED_SK"));
    }

    #[tokio::test]
    async fn ensure_credentials_fetches_api_key_when_missing() {
        let mut item = base_item();
        let fetcher = |_bucket: String| async move {
            Ok::<String, Box<dyn std::error::Error + Send + Sync>>("my-fetched-api-key".into())
        };

        item.ensure_credentials("my-bucket", Some(fetcher))
            .await
            .unwrap();

        assert_eq!(item.api_key.as_deref(), Some("my-fetched-api-key"));
    }

    #[tokio::test]
    async fn ensure_credentials_errors_without_fetcher() {
        let mut item = base_item();
        let result = item
            .ensure_credentials("my-bucket", None::<fn(String) -> std::future::Ready<Result<String, Box<dyn std::error::Error + Send + Sync>>>>)
            .await;
        assert!(result.is_err());
    }
}
