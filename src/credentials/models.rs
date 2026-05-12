pub enum BucketCredential {
    Hmac {
        access_key: String,
        secret_key: String,
    },
    ApiKey(String),
}

impl BucketCredential {
    pub fn parse(raw: &str) -> Self {
        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(raw) {
            if let (Some(ak), Some(sk)) = (json_val.get("access_key"), json_val.get("secret_key")) {
                return BucketCredential::Hmac {
                    access_key: ak.as_str().unwrap_or_default().to_owned(),
                    secret_key: sk.as_str().unwrap_or_default().to_owned(),
                };
            }
            if let Some(apikey) = json_val.get("api_key").or_else(|| json_val.get("apikey")) {
                return BucketCredential::ApiKey(apikey.as_str().unwrap_or_default().to_owned());
            }
        }

        BucketCredential::ApiKey(raw.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bucket_credential_variants() {
        let hmac_json = r#"{ "access_key": "AK", "secret_key": "SK" }"#;
        match BucketCredential::parse(hmac_json) {
            BucketCredential::Hmac {
                access_key,
                secret_key,
            } => {
                assert_eq!(access_key, "AK");
                assert_eq!(secret_key, "SK");
            }
            _ => panic!("Expected Hmac variant"),
        }

        let api_json = r#"{ "api_key": "APIKEY" }"#;
        if let BucketCredential::ApiKey(k) = BucketCredential::parse(api_json) {
            assert_eq!(k, "APIKEY");
        } else {
            panic!("Expected ApiKey variant");
        }

        let raw = "raw_token";
        if let BucketCredential::ApiKey(k) = BucketCredential::parse(raw) {
            assert_eq!(k, raw);
        } else {
            panic!("Expected fallback ApiKey variant");
        }
    }

    #[test]
    fn parse_apikey_alias() {
        // "apikey" (no underscore) must also be recognised
        let json = r#"{ "apikey": "MY_API_KEY" }"#;
        if let BucketCredential::ApiKey(k) = BucketCredential::parse(json) {
            assert_eq!(k, "MY_API_KEY");
        } else {
            panic!("Expected ApiKey variant for 'apikey' alias");
        }
    }

    #[test]
    fn parse_invalid_json_falls_back_to_api_key() {
        let raw = "not-json-at-all";
        if let BucketCredential::ApiKey(k) = BucketCredential::parse(raw) {
            assert_eq!(k, raw);
        } else {
            panic!("Expected ApiKey fallback for non-JSON input");
        }
    }

    #[test]
    fn parse_json_missing_keys_falls_back_to_api_key() {
        // Valid JSON but neither hmac nor api_key fields present
        let raw = r#"{"foo": "bar"}"#;
        if let BucketCredential::ApiKey(k) = BucketCredential::parse(raw) {
            assert_eq!(k, raw);
        } else {
            panic!("Expected ApiKey fallback when JSON has no recognised keys");
        }
    }

    #[test]
    fn parse_empty_string_falls_back_to_api_key() {
        if let BucketCredential::ApiKey(k) = BucketCredential::parse("") {
            assert_eq!(k, "");
        } else {
            panic!("Expected ApiKey fallback for empty string");
        }
    }
}
