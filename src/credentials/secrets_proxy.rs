use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use pyo3::{PyObject, PyResult, Python};
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, error};

/// A cached IAM bearer token together with its UNIX expiry timestamp.
#[derive(Clone, Debug)]
pub struct SecretValue {
    value: String,
    expiration: u64,
}

impl SecretValue {
    /// Create a new secret with `value` that expires at the given UNIX timestamp.
    pub fn new(value: String, expiration: u64) -> Self {
        SecretValue { value, expiration }
    }

    /// Return the raw token string.
    pub fn get_value(&self) -> &str {
        &self.value
    }

    /// Return the UNIX expiry timestamp.
    pub fn get_expiration(&self) -> u64 {
        self.expiration
    }

    /// Returns `true` if the token has expired (with a 5-minute safety buffer).
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_secs();
        now >= self.expiration - 300 // 5 minute buffer
    }
}

/// A thread-safe, in-memory cache for IBM IAM bearer tokens.
///
/// Tokens are automatically refreshed when they are within 5 minutes of
/// expiry.  The cache is keyed by bucket name and shared across all Pingora
/// worker threads via an [`Arc`].
#[derive(Clone, Debug)]
pub struct SecretsCache {
    inner: Arc<RwLock<HashMap<String, SecretValue>>>,
}

impl Default for SecretsCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretsCache {
    /// Create a new, empty secrets cache.
    pub fn new() -> Self {
        SecretsCache {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Insert a token `value` for `key` that expires at the given UNIX timestamp.
    pub fn insert(&self, key: String, value: String, expiration: u64) {
        let secret = SecretValue { value, expiration };

        let mut map = self.inner.write().expect("lock poisoned");
        map.insert(key, secret);
    }

    /// Return a valid token for `key`, fetching a new one via `bearer_fetcher` if
    /// the cache is empty or the stored token is near expiry.
    pub async fn get<F, Fut>(&self, key: &str, bearer_fetcher: F) -> Option<String>
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<IamResponse, Box<dyn std::error::Error>>> + Send,
    {
        let maybe_secret = {
            let map = self.inner.read().expect("lock poisoned");
            map.get(key).cloned()
        };

        match maybe_secret {
            Some(secret) => {
                if secret.is_expired() {
                    debug!("Token for {} is expired, renewing ...", key);
                    match bearer_fetcher().await {
                        Ok(iam_response) => {
                            self.insert(
                                key.to_string(),
                                iam_response.access_token.clone(),
                                iam_response.expiration,
                            );
                            debug!("Renewed token for {}", key);
                            Some(iam_response.access_token)
                        }
                        Err(e) => {
                            error!("Failed to renew token for {}: {}", key, e);
                            None
                        }
                    }
                } else {
                    debug!("Using cached token for {}", key);
                    Some(secret.get_value().to_string())
                }
            }
            None => {
                debug!("No cached token found for {}, fetching ...", key);
                match bearer_fetcher().await {
                    Ok(iam_response) => {
                        self.insert(
                            key.to_string(),
                            iam_response.access_token.clone(),
                            iam_response.expiration,
                        );
                        debug!("Fetched new token for {}", key);
                        Some(iam_response.access_token)
                    }
                    Err(e) => {
                        error!("Failed to fetch token for {}: {}", key, e);
                        None
                    }
                }
            }
        }
    }

    /// Remove the cached token for `key`, forcing a fresh fetch on the next call to [`get`](Self::get).
    pub fn invalidate(&self, key: &str) {
        let mut map = self.inner.write().expect("lock poisoned");
        map.remove(key);
    }
}

/// Deserialized response from the IBM IAM `/identity/token` endpoint.
#[derive(Deserialize, Debug)]
pub struct IamResponse {
    /// The short-lived OAuth2 access token.
    pub access_token: String,
    /// Token lifetime in seconds (typically 3600).
    pub expires_in: u32,
    /// Absolute UNIX expiry timestamp.
    pub expiration: u64,
}

/// Exchange an IBM COS API key for an IAM bearer token.
///
/// Posts to `https://iam.cloud.ibm.com/identity/token` and returns the
/// deserialized [`IamResponse`] on success.
pub(crate) async fn get_bearer(api_key: String) -> Result<IamResponse, Box<dyn std::error::Error>> {
    debug!("Fetching bearer token for the API key");
    let client = Client::new();

    let params = [
        ("grant_type", "urn:ibm:params:oauth:grant-type:apikey"),
        ("apikey", &api_key),
    ];

    // todo: move url to config
    let resp = client
        .post("https://iam.cloud.ibm.com/identity/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .await?;

    if resp.status().is_success() {
        let iam_response: IamResponse = resp.json().await?;
        debug!("Received access token");
        Ok(iam_response)
    } else {
        let err_text = resp.text().await?;
        error!("Failed to get token: {}", err_text);
        Err(format!("Failed to get token: {}", err_text).into())
    }
}

/// Call the Python `bucket_creds_fetcher` callback and return the raw
/// credential string it produces.
///
/// The callback receives `(token, bucket)` as positional arguments and must
/// return a `str`.
pub(crate) async fn get_credential_for_bucket(
    callback: &PyObject,
    bucket: String,
    token: String,
) -> PyResult<String> {
    Python::with_gil(|py| {
        let s = callback.call1(py, (token, bucket))?;
        s.extract::<String>(py)
    })
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_response(json_body: &str, status_code: u16) -> ResponseTemplate {
        ResponseTemplate::new(status_code).set_body_raw(json_body, "application/json")
    }

    #[tokio::test]
    async fn test_get_bearer_success() {
        let mock_server = MockServer::start().await;

        let response_body = r#"{
            "access_token": "mock_access_token",
            "expires_in": 3600,
            "expiration": 9999999999
        }"#;

        Mock::given(method("POST"))
            .and(path("/identity/token"))
            .respond_with(make_response(response_body, 200))
            .mount(&mock_server)
            .await;

        let result = get_bearer_with_url("mock_api_key".to_string(), &mock_server.uri()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "mock_access_token");
    }

    #[tokio::test]
    async fn test_get_bearer_failure() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/identity/token"))
            .respond_with(ResponseTemplate::new(400).set_body_string("Invalid API key"))
            .mount(&mock_server)
            .await;

        let result = get_bearer_with_url("invalid_api_key".to_string(), &mock_server.uri()).await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Failed to get token: Invalid API key"
        );
    }

    #[tokio::test]
    async fn test_get_bearer_invalid_json() {
        let mock_server = MockServer::start().await;

        let invalid_json = r#"{
            "invalid_field": "value"
        }"#;

        Mock::given(method("POST"))
            .and(path("/identity/token"))
            .respond_with(make_response(invalid_json, 200))
            .mount(&mock_server)
            .await;

        let result = get_bearer_with_url("mock_api_key".to_string(), &mock_server.uri()).await;
        assert!(result.is_err());

        let err_message = result.unwrap_err().to_string();
        assert!(
            err_message.contains("missing field `access_token`")
                || err_message.contains("error decoding response body"),
            "Unexpected error message: {}",
            err_message
        );
    }

    async fn get_bearer_with_url(
        api_key: String,
        base_url: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        let params = [
            ("grant_type", "urn:ibm:params:oauth:grant-type:apikey"),
            ("apikey", &api_key),
        ];
        let resp = client
            .post(&format!("{}/identity/token", base_url))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&params)
            .send()
            .await?;

        if resp.status().is_success() {
            let iam_response: IamResponse = resp.json().await?;
            Ok(iam_response.access_token)
        } else {
            let err_text = resp.text().await?;
            Err(format!("Failed to get token: {}", err_text).into())
        }
    }

    #[tokio::test]
    async fn secrets_cache_hit_returns_cached_value() {
        let cache = SecretsCache::new();
        let key = "test".to_string();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        cache.insert(key.clone(), "cached_token".to_string(), now + 3600);

        let fetcher = || async { panic!("Should not be called on cache hit") };

        let result = cache.get(&key, fetcher).await;
        assert_eq!(result, Some("cached_token".to_string()));
    }

    #[tokio::test]
    async fn secrets_cache_expired_renews_token() {
        let cache = SecretsCache::new();
        let key = "test2".to_string();
        // expired token
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        cache.insert(key.clone(), "old_token".to_string(), now);

        // fetcher returns new token
        let fetcher = move || async {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            Ok(IamResponse {
                access_token: "new_token".into(),
                expires_in: 3600,
                expiration: now + 7200,
            })
        };

        let result = cache.get(&key, fetcher).await;
        assert_eq!(result, Some("new_token".to_string()));
    }

    #[tokio::test]
    async fn secrets_cache_invalidate_works() {
        let cache = SecretsCache::new();
        let key = "test3".to_string();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        cache.insert(key.clone(), "token".to_string(), now + 3600);

        cache.invalidate(&key);

        // now fetcher must be called
        let fetcher = move || async {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            Ok(IamResponse {
                access_token: "fresh_token".into(),
                expires_in: 3600,
                expiration: now + 3600,
            })
        };
        let result = cache.get(&key, fetcher).await;
        assert_eq!(result, Some("fresh_token".to_string()));
    }
}
