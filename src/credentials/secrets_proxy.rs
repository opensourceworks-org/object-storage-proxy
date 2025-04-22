use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use pyo3::{PyObject, PyResult, Python};
use reqwest::Client;
use serde::Deserialize;
use tracing::{error, info};

#[derive(Clone, Debug)]
pub struct SecretValue {
    value: String,
    expiration: u64,
}

impl SecretValue {
    pub fn new(value: String, expiration: u64) -> Self {
        SecretValue { value, expiration }
    }

    pub fn get_value(&self) -> &str {
        &self.value
    }

    pub fn get_expiration(&self) -> u64 {
        self.expiration
    }

    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now >= self.expiration - 300 // 5 minute buffer
    }
}

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
    pub fn new() -> Self {
        SecretsCache {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn insert(&self, key: String, value: String, expiration: u64) {
        let secret = SecretValue { value, expiration };

        let mut map = self.inner.write().unwrap();
        map.insert(key, secret);
    }

    pub async fn get<F, Fut>(&self, key: &str, bearer_fetcher: F) -> Option<String>
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<IamResponse, Box<dyn std::error::Error>>> + Send,
    {
        let maybe_secret = {
            let map = self.inner.read().unwrap();
            map.get(key).cloned()
        };

        match maybe_secret {
            Some(secret) => {
                if secret.is_expired() {
                    info!("Token for {} is expired, renewing ...", key);
                    match bearer_fetcher().await {
                        Ok(iam_response) => {
                            self.insert(
                                key.to_string(),
                                iam_response.access_token.clone(),
                                iam_response.expiration,
                            );
                            println!("Renewed token for {}", key);
                            Some(iam_response.access_token)
                        }
                        Err(e) => {
                            error!("Failed to renew token for {}: {}", key, e);
                            None
                        }
                    }
                } else {
                    info!("Using cached token for {}", key);
                    Some(secret.get_value().to_string())
                }
            }
            None => {
                info!("No cached token found for {}, fetching ...", key);
                match bearer_fetcher().await {
                    Ok(iam_response) => {
                        self.insert(
                            key.to_string(),
                            iam_response.access_token.clone(),
                            iam_response.expiration,
                        );
                        info!("Fetched new token for {}", key);
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

    pub fn invalidate(&self, key: &str) {
        let mut map = self.inner.write().unwrap();
        map.remove(key);
    }
}

#[derive(Deserialize, Debug)]
pub struct IamResponse {
    pub access_token: String,
    pub expires_in: u32,
    pub expiration: u64,
}

pub(crate) async fn get_bearer(api_key: String) -> Result<IamResponse, Box<dyn std::error::Error>> {
    info!("Fetching bearer token for the API key");
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
        info!("Received access token");
        Ok(iam_response)
    } else {
        let err_text = resp.text().await?;
        error!("Failed to get token: {}", err_text);
        Err(format!("Failed to get token: {}", err_text).into())
    }
}

pub(crate) async fn get_credential_for_bucket(
    callback: &PyObject,
    bucket: String,
    token: String
) -> PyResult<String> {
    Python::with_gil(|py| {
        let s = callback.call1(py, (token, bucket,))?;
        s.extract::<String>(py)
    })
}

#[cfg(test)]
mod tests {
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
}
