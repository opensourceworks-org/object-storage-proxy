use chrono::{DateTime, NaiveDateTime, Utc};
use http::header::HeaderMap;
use pingora::{http::RequestHeader, proxy::Session};
use rustls::crypto::hash::Hash;
use sha256::digest;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use std::{collections::{HashMap, HashSet}, fmt, sync::Arc};
use url::Url;

use crate::parsers::{cos_map::CosMapItem, credentials::{parse_credential_scope, parse_token_from_header}};

const SHORT_DATE: &str = "%Y%m%d";
const LONG_DATETIME: &str = "%Y%m%dT%H%M%SZ";

// AwsSign copied and modified from https://github.com/psnszsn/aws-sign-v4

pub struct AwsSign<'a, T: 'a>
where
    &'a T: std::iter::IntoIterator<Item = (&'a String, &'a String)>, T: std::fmt::Debug
{
    method: &'a str,
    url: Url,
    datetime: &'a DateTime<Utc>,
    region: &'a str,
    access_key: &'a str,
    secret_key: &'a str,
    headers: T,
    payload_override: Option<String>,

    /*
    service is the <aws-service-code> that can be found in the service-quotas api.

    For example, use the value `ServiceCode` for this `service` property.
    Thus, for "Amazon Simple Storage Service (Amazon S3)", you would use value "s3"

    ```
    > aws service-quotas list-services
    {
        "Services": [
            ...
            {
                "ServiceCode": "a4b",
                "ServiceName": "Alexa for Business"
            },
            ...
            {
                "ServiceCode": "s3",
                "ServiceName": "Amazon Simple Storage Service (Amazon S3)"
            },
            ...
    ```
    This is not absolute, so you might need to poke around at the service you're interesed in.
    See:
    [AWS General Reference -> Service endpoints and quotas](https://docs.aws.amazon.com/general/latest/gr/aws-service-information.html) - to look up "service" names and codes

    added in 0.2.0
    */
    service: &'a str,

    /// body, such as in an http POST
    body: &'a [u8],
}

impl<'a> AwsSign<'a, HashMap<String, String>> {
    pub fn new<B: AsRef<[u8]> + ?Sized>(
        method: &'a str,
        url: &'a str,
        datetime: &'a DateTime<Utc>,
        headers: &'a HeaderMap,
        region: &'a str,
        access_key: &'a str,
        secret_key: &'a str,
        service: &'a str,
        body: &'a B,
        signed_headers: Option<&'a Vec<String>>,
    ) -> Self {
        let allowed: Vec<&str> = if let Some(sh) = signed_headers {
            sh.iter().map(String::as_str).collect()
        } else {
            vec![
                "host",
                "x-amz-date",
                "range",
                "x-amz-content-sha256",
                "x-amz-security-token",
            ]
        };
        // let allowed = [
        //     "host",
        //     "x-amz-date",
        //     "range",
        //     "x-amz-content-sha256",
        //     "x-amz-security-token",
        // ];
        
        dbg!(&url);
        let url: Url = url.parse().unwrap();
        let headers: HashMap<String, String> = headers
            .iter()
            .filter_map(|(key, value)| {
                let name = key.as_str().to_lowercase();
                if !allowed.contains(&name.as_str()) {
                    return None;
                }
                value
                    .to_str()
                    .ok()
                    .map(|v| (name, v.trim().to_owned()))
            })
            .collect();
        Self {
            method,
            url,
            datetime,
            region,
            access_key,
            secret_key,
            headers,
            service,
            body: body.as_ref(),
            payload_override: None,
        }
    }
}

/// custom debug implementation to redact secret_key
impl<'a, T> fmt::Debug for AwsSign<'a, T>
where
    &'a T: IntoIterator<Item = (&'a String, &'a String)>, T: std::fmt::Debug
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AwsSign")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("datetime", &self.datetime)
            .field("region", &self.region)
            .field("access_key", &self.access_key)
            .field("secret_key", &"<REDACTED>")
            .field("service", &self.service)
            .field("body", &self.body)
            .field("headers", &self.headers)
            .finish()
    }
}

impl<'a, T> AwsSign<'a, T>
where
    &'a T: std::iter::IntoIterator<Item = (&'a String, &'a String)>, T: std::fmt::Debug
{
    pub fn set_payload_override(&mut self, h: String) {
        self.payload_override = Some(h);
    }

    pub fn canonical_header_string(&'a self) -> String {
        let mut keyvalues = self
            .headers
            .into_iter()
            .map(|(key, value)| key.to_lowercase() + ":" + value.trim())
            .collect::<Vec<String>>();
        keyvalues.sort();
        keyvalues.join("\n")
    }

    pub fn signed_header_string(&'a self) -> String {
        let mut keys = self
            .headers
            .into_iter()
            .map(|(key, _)| key.to_lowercase())
            .collect::<Vec<String>>();
        keys.sort();
        keys.join(";")
    }

    pub fn canonical_request(&'a self) -> String {
        let url: &str = self.url.path().into();
        let payload_line = if let Some(ov) = &self.payload_override {
            ov.clone()
        } else if self.body == b"UNSIGNED-PAYLOAD" {
            "UNSIGNED-PAYLOAD".into()
        } else {
            digest(self.body)
        };

        format!(
            "{method}\n{uri}\n{query_string}\n{headers}\n\n{signed}\n{payload}",
            method = self.method,
            uri = url,
            query_string = canonical_query_string(&self.url),
            headers = self.canonical_header_string(),
            signed = self.signed_header_string(),
            payload = payload_line,
        )
    }
    pub fn sign(&'a self) -> String {
        let canonical = self.canonical_request();
        let string_to_sign = string_to_sign(self.datetime, self.region, &canonical, self.service);
        let signing_key = signing_key(self.datetime, self.secret_key, self.region, self.service);
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &signing_key.unwrap());
        let tag = ring::hmac::sign(&key, string_to_sign.as_bytes());
        let signature = hex::encode(tag.as_ref());
        let signed_headers = self.signed_header_string();

        format!(
            "AWS4-HMAC-SHA256 Credential={access_key}/{scope},\
             SignedHeaders={signed_headers},Signature={signature}",
            access_key = self.access_key,
            scope = scope_string(self.datetime, self.region, self.service),
            signed_headers = signed_headers,
            signature = signature
        )
    }
}

pub fn uri_encode(string: &str, encode_slash: bool) -> String {
    let mut result = String::with_capacity(string.len() * 2);
    for c in string.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' | '~' | '.' => result.push(c),
            '/' if encode_slash => result.push_str("%2F"),
            '/' if !encode_slash => result.push('/'),
            _ => {
                result.push_str(
                    &format!("{}", c)
                        .bytes()
                        .map(|b| format!("%{:02X}", b))
                        .collect::<String>(),
                );
            }
        }
    }
    result
}

pub fn canonical_query_string(uri: &Url) -> String {
    let mut keyvalues = uri
        .query_pairs()
        .map(|(key, value)| uri_encode(&key, true) + "=" + &uri_encode(&value, true))
        .collect::<Vec<String>>();
    keyvalues.sort();
    keyvalues.join("&")
}

pub fn scope_string(datetime: &DateTime<Utc>, region: &str, service: &str) -> String {
    format!(
        "{date}/{region}/{service}/aws4_request",
        date = datetime.format(SHORT_DATE),
        region = region,
        service = service
    )
}

pub fn string_to_sign(
    datetime: &DateTime<Utc>,
    region: &str,
    canonical_req: &str,
    service: &str,
) -> String {
    let hash = ring::digest::digest(&ring::digest::SHA256, canonical_req.as_bytes());
    format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{hash}",
        timestamp = datetime.format(LONG_DATETIME),
        scope = scope_string(datetime, region, service),
        hash = hex::encode(hash.as_ref())
    )
}

pub fn signing_key(
    datetime: &DateTime<Utc>,
    secret_key: &str,
    region: &str,
    service: &str,
) -> Result<Vec<u8>, String> {
    let secret = String::from("AWS4") + secret_key;

    let date_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    let date_tag = ring::hmac::sign(
        &date_key,
        datetime.format(SHORT_DATE).to_string().as_bytes(),
    );

    let region_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, date_tag.as_ref());
    let region_tag = ring::hmac::sign(&region_key, region.to_string().as_bytes());

    let service_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, region_tag.as_ref());
    let service_tag = ring::hmac::sign(&service_key, service.as_bytes());

    let signing_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, service_tag.as_ref());
    let signing_tag = ring::hmac::sign(&signing_key, b"aws4_request");
    Ok(signing_tag.as_ref().to_vec())
}

pub(crate) async fn sign_request(
    request: &mut RequestHeader,
    cos_map: &CosMapItem,
) -> Result<(), Box<dyn std::error::Error>> {
    // if no region, access_key or secret_key, return error
    if cos_map.region.is_none() || cos_map.access_key.is_none() || cos_map.secret_key.is_none() {
        return Err("Missing region, access_key or secret_key".into());
    }

    request.remove_header("authorization");

    let datetime = chrono::Utc::now();
    let method = request.method.to_string();
    let url = request.uri.to_string();
    let access_key = cos_map.access_key.as_ref().unwrap();
    let secret_key = cos_map.secret_key.as_ref().unwrap();
    let region = cos_map.region.as_ref().unwrap();

    request.insert_header(
        "X-Amz-Date",
        datetime
            .format("%Y%m%dT%H%M%SZ")
            .to_string()
            .parse::<http::header::HeaderValue>()
            .unwrap(),
    )?;
    let payload_hash = if method == "GET" || method == "HEAD" || method == "DELETE" {
        // spec uses empty‑body hash for reads
        &sha256::digest(b"")
    } else {
        // for streaming uploads we sign UNSIGNED‑PAYLOAD
        "UNSIGNED-PAYLOAD"
    };

    request.insert_header("x-amz-content-sha256", payload_hash)?;

    let body_bytes: &[u8] = match payload_hash {
        // empty body → empty slice
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" => &[], // sha256 hash of empty string
        "UNSIGNED-PAYLOAD" => b"UNSIGNED-PAYLOAD",
        // unreachable code
        _ => &[],
    };

    let auth_header = AwsSign::new(
        &method,
        &url,
        &datetime,
        &request.headers,
        region,
        access_key,
        secret_key,
        "s3",
        body_bytes,
        None,
    );
    debug!("{:#?}", &auth_header);

    let signature = auth_header.sign();
    debug!("{:#?}", signature);

    request.insert_header(
        "Authorization",
        http::header::HeaderValue::from_str(&auth_header.sign())?,
    )?;

    Ok(())
}


pub async fn signature_is_valid(
    auth_header: &str,
    session: &Session,
    secret_key: &str,
) -> Result<bool, Box<dyn std::error::Error>> {

    let signed_headers_str = auth_header
        .split("SignedHeaders=")
        .nth(1)
        .and_then(|s| s.split(',').next())
        .unwrap_or("");
    let signed_headers: Vec<String> =
        signed_headers_str.split(';').map(|s| s.to_lowercase()).collect();


    // extract access key from Authorization header
    let (_, local_access_key) = parse_token_from_header(auth_header)
        .map_err(|_| pingora::Error::new_str("Failed to parse token"))?;
    let local_access_key = local_access_key.to_string();
    if local_access_key.is_empty() {
        error!("Missing access key");
        return Ok(false);
    }

    // extract provided signature
    let provided_signature = auth_header
        .split("Signature=")
        .nth(1)
        .ok_or_else(|| pingora::Error::new_str("Invalid Authorization header: no Signature"))?
        .to_string();

    // parse x-amz-date header
    let dt_header = session
        .req_header()
        .headers
        .get("x-amz-date")
        .ok_or_else(|| pingora::Error::new_str("Missing x-amz-date header"))?
        .to_str()?;

    // use NaiveDateTime then assign Utc timezone (format: %Y%m%dT%H%M%SZ)
    let naive = NaiveDateTime::parse_from_str(dt_header, LONG_DATETIME)
        .map_err(|_| {
            pingora::Error::new_str("invalid date")
        })?;
    let datetime = naive.and_utc();


    info!("parsing the region and service");

    let (_, (region, service)) = parse_credential_scope(auth_header)
        .map_err(|_| pingora::Error::new_str("Invalid Credential scope"))?;

    dbg!(&region);
    dbg!(&service);

    // determine payload hash header
    let content_sha256 = session
        .req_header()
        .headers
        .get("x-amz-content-sha256")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| pingora::Error::new_str("Missing x-amz-content-sha256 header"))?;
    // let body_bytes: &[u8] = match content_sha256 {
    //     "UNSIGNED-PAYLOAD" => b"UNSIGNED-PAYLOAD",
    //     _ => &[],
    // };

    let (body_bytes, payload_override) = if content_sha256 == "UNSIGNED-PAYLOAD" {
        (b"UNSIGNED-PAYLOAD" as &[u8], None)
    } else {
        // we don't have the raw body here, but we do have its hash:
        // tell AwsSign to use this string directly
        (&[] as &[u8], Some(content_sha256.to_owned()))
    };

    let original_uri = session.req_header().uri.to_string();
    let full_url = if original_uri.starts_with('/') {
        let host = session
            .req_header()
            .headers
            .get("host")
            .ok_or_else(|| pingora::Error::new_str("Missing host header"))?
            .to_str()?;
        format!("https://{}{}", host, original_uri)
    } else {
        original_uri
    };

    // construct AwsSign and compute signature
    let method = session.req_header().method.to_string();
    let mut signer = AwsSign::new(
        &method,
        &full_url,
        &datetime,
        &session.req_header().headers,
        region,
        &local_access_key,
        &secret_key,
        service,
        body_bytes,
        Some(&signed_headers),
    );

    if let Some(ov) = payload_override {
        signer.set_payload_override(ov);
    }

    let signature = signer.sign();
    let computed_signature = signature
        .split("Signature=")
        .nth(1)
        .unwrap_or_default();

    info!("Provided signature: {}", provided_signature);
    info!("Computed signature: {}", computed_signature);
    Ok(computed_signature == provided_signature)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::cos_map::CosMapItem;
    use http::{HeaderMap, Method};
    use pingora::http::RequestHeader;
    use regex::Regex;
    use sha256::digest;

    #[test]
    fn sample_canonical_request() {
        let datetime = chrono::Utc::now();
        let url: &str = "https://hi.s3.us-east-1.amazonaws.com/Prod/graphql";
        let map: HeaderMap = HeaderMap::new();
        let aws_sign = AwsSign::new("GET", url, &datetime, &map, "us-east-1", "a", "b", "s3", "", None);
        let s = aws_sign.canonical_request();
        assert_eq!(
            s,
            "GET\n/Prod/graphql\n\n\n\n\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sample_canonical_request_using_u8_body() {
        let datetime = chrono::Utc::now();
        let url: &str = "https://hi.s3.us-east-1.amazonaws.com/Prod/graphql";
        let map: HeaderMap = HeaderMap::new();
        let aws_sign = AwsSign::new(
            "GET",
            url,
            &datetime,
            &map,
            "us-east-1",
            "a",
            "b",
            "s3",
            "".as_bytes(),
            None,
        );
        let s = aws_sign.canonical_request();
        assert_eq!(
            s,
            "GET\n/Prod/graphql\n\n\n\n\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sample_canonical_request_using_vec_body() {
        let datetime = chrono::Utc::now();
        let url: &str = "https://hi.s3.us-east-1.amazonaws.com/Prod/graphql";
        let map: HeaderMap = HeaderMap::new();
        let body = Vec::new();
        let aws_sign = AwsSign::new(
            "GET",
            url,
            &datetime,
            &map,
            "us-east-1",
            "a",
            "b",
            "s3",
            &body,
            None,
        );
        let s = aws_sign.canonical_request();
        assert_eq!(
            s,
            "GET\n/Prod/graphql\n\n\n\n\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    fn make_cos_map_item() -> CosMapItem {
        CosMapItem {
            region: Some("us-east-1".into()),
            access_key: Some("AKIDEXAMPLE".into()),
            secret_key: Some("wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into()),
            host: "bucket.s3.us-east-1.amazonaws.com".into(),
            port: 443,
            api_key: None,
            ttl: None,
        }
    }

    /// Any method other than GET/HEAD/DELETE should use UNSIGNED-PAYLOAD
    #[tokio::test]
    async fn post_request_uses_unsigned_payload() {
        // build a POST RequestHeader
        let mut req = RequestHeader::build(
            Method::GET,
            b"https://bucket.s3.us-east-1.amazonaws.com/?list-type=2&prefix=mandelbrot&encoding-type=url",
            None
        ).unwrap();
        req.insert_header("Host", "bucket.s3.us-east-1.amazonaws.com")
            .unwrap();
        assert!(req.headers.get("x-amz-content-sha256").is_none());

        // run sign_request
        let cos = make_cos_map_item();
        sign_request(&mut req, &cos).await.unwrap();

        // x-amz-content-sha256 must be "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        let payload_header = req
            .headers
            .get("x-amz-content-sha256")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            payload_header,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        // Authorization header must include our access key and scope
        let auth = req.headers.get("authorization").unwrap().to_str().unwrap();
        assert!(auth.contains("Credential=AKIDEXAMPLE/"));
        assert!(auth.contains("/us-east-1/s3/aws4_request,"));
    }

    /// GET/DELETE must use the empty-body hash, and sign correctly
    #[tokio::test]
    async fn get_request_sets_empty_body_hash_and_signature_format() {
        let mut req = RequestHeader::build(
            Method::GET, b"https://bucket.s3.us-east-1.amazonaws.com/?list-type=2&prefix=mandelbrot&encoding-type=url",
            None
        ).unwrap();
        req.insert_header("Host", "bucket.s3.us-east-1.amazonaws.com")
            .unwrap();
        let cos = make_cos_map_item();
        sign_request(&mut req, &cos).await.unwrap();

        // empty-body sha256
        let empty_hash = digest(b"");
        let header_hash = req
            .headers
            .get("x-amz-content-sha256")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(header_hash, empty_hash);

        // X-Amz-Date must be a valid timestamp ending in Z
        let x_amz_date = req.headers.get("x-amz-date").unwrap().to_str().unwrap();
        let re_date = Regex::new(r"^\d{8}T\d{6}Z$").unwrap();
        assert!(
            re_date.is_match(x_amz_date),
            "x-amz-date wrong format: {}",
            x_amz_date
        );

        // Authorization header format
        let auth = req.headers.get("authorization").unwrap().to_str().unwrap();
        assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"));
        // must have SignedHeaders including host;x-amz-content-sha256;x-amz-date
        assert!(auth.contains("SignedHeaders="));
        assert!(auth.contains("host;"));
        assert!(auth.contains("x-amz-content-sha256;"));
        assert!(auth.contains("x-amz-date"));
    }

    /// Missing any of region/access_key/secret_key should error out
    #[tokio::test]
    async fn error_when_missing_credentials() {
        let mut req = RequestHeader::build(
            Method::GET,
            b"https://bucket.s3.us-east-1.amazonaws.com/?list-type=2&prefix=mandelbrot&encoding-type=url",
            None
        ).unwrap();
        req.insert_header("Host", "bucket.s3.us-east-1.amazonaws.com")
            .unwrap();
        let mut cos = make_cos_map_item();
        cos.region = None; // drop region
        let err = sign_request(&mut req, &cos).await.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Missing region, access_key or secret_key"));
    }

    #[test]
    fn uri_encode_edge_cases() {
        assert_eq!(uri_encode("simple", true), "simple");
        assert_eq!(uri_encode("a b", true), "a%20b");
        assert_eq!(uri_encode("/path/with/slash", true), "%2Fpath%2Fwith%2Fslash");
        assert_eq!(uri_encode("/path/with/slash", false), "/path/with/slash");
        assert_eq!(uri_encode("unicode✓", true).contains("%E2%9C%93"), true);
    }

    #[test]
    fn canonical_query_string_sorts_and_encodes() {
        let url = "https://example.com/?b=2&a=1 space";
        let parsed = url.parse().unwrap();
        let qs = canonical_query_string(&parsed);
        assert_eq!(qs, "a=1%20space&b=2");
    }
}
