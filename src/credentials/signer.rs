pub(crate) use chrono::{DateTime, NaiveDateTime, Utc};
use http::header::HeaderMap;
use pingora::{http::RequestHeader, proxy::Session};
use ring::hmac;
use sha256::digest;
use tracing::{debug, error};
use std::{collections::{HashMap, HashSet}, fmt, io};
use url::Url;

use bytes::{Bytes, BytesMut, BufMut};

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

/// Create a new AwsSign instance
/// 
/// # Arguments
/// 
/// * `method` - HTTP method (GET, POST, etc.)
/// * `url` - URL to sign
/// * `datetime` - Date and time of the request
/// * `headers` - HTTP headers
/// * `region` - AWS region
/// * `access_key` - AWS access key
/// * `secret_key` - AWS secret key
/// * `service` - AWS service code
/// * `body` - Request body
/// * `signed_headers` - Optional list of signed headers, used to check inbound request signature
/// 
/// # Returns
/// 
/// A new instance of `AwsSign`
/// 
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
        _signed_headers: Option<&'a Vec<String>>,
    ) -> Self {


        // let allowed: Vec<&str> = if let Some(sh) = signed_headers {
        //     sh.iter().map(String::as_str).collect()
        // } else {
        //     vec![
        //         "host",
        //         "x-amz-date",
        //         "range",
        //         "x-amz-content-sha256",
        //         "x-amz-security-token",
        //     ]
        // };

        let signed_allow: Option<HashSet<&str>> =
            _signed_headers.map(|v| v.iter().map(String::as_str).collect());

        let headers: HashMap<String, String> = headers
            .iter()
            .filter_map(|(key, value)| {
                let name = key.as_str().to_lowercase();

                // ─── decide whether to keep `name` ──────────────────────────
                let keep = if let Some(ref set) = signed_allow {
                    // verifier path → keep exactly what the client signed
                    set.contains(name.as_str())
                } else {
                    // re-signing path → keep the full streaming whitelist
                    name == "host"
                        || name.starts_with("x-amz-")
                        || matches!(
                            name.as_str(),
                            "content-length"
                                | "content-encoding"
                                | "transfer-encoding"
                                | "range"
                                | "expect"
                                | "x-amz-decoded-content-length"
                        )
                };
                if !keep {
                    return None;
                }
                value.to_str().ok().map(|v| (name, v.trim().to_owned()))
            })
            .collect();
        

        // let headers: HashMap<String, String> = headers
        //     .iter()
        //     .filter_map(|(key, value)| {
        //         let name = key.as_str().to_lowercase();
        //         let keep = name == "host"
        //             || name.starts_with("x-amz-")
        //             || name == "content-length"
        //             || name == "content-encoding"
        //             || name == "transfer-encoding"
        //             || name == "range";
        //         if !keep {
        //             return None;
        //         }
        //         value.to_str().ok().map(|v| (name, v.trim().to_owned()))
        //     })
        //     .collect();
        
        debug!("{:#?}", &url);
        let url: Url = url.parse().unwrap();
        // let headers: HashMap<String, String> = headers
        //     .iter()
        //     .filter_map(|(key, value)| {
        //         let name = key.as_str().to_lowercase();
        //         if !allowed.contains(&name.as_str()) {
        //             return None;
        //         }
        //         value
        //             .to_str()
        //             .ok()
        //             .map(|v| (name, v.trim().to_owned()))
        //     })
        //     .collect();
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
            .field("payload_override", &self.payload_override)
            .finish()
    }
}

impl<'a, T> AwsSign<'a, T>
where
    &'a T: std::iter::IntoIterator<Item = (&'a String, &'a String)>, T: std::fmt::Debug
{
    /// for streaming uploads, we need to override the payload hash
    /// with the actual payload hash
    /// this is used for the `UNSIGNED-PAYLOAD` case
    /// and for the `payload_override` case
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
        } else if self.body == b"STREAMING-AWS4-HMAC-SHA256-PAYLOAD" {
            // NEW: forward the literal marker for chunk-signed streams
            "STREAMING-AWS4-HMAC-SHA256-PAYLOAD".into()
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

        let sign_string = format!(
            "AWS4-HMAC-SHA256 Credential={access_key}/{scope},\
             SignedHeaders={signed_headers},Signature={signature}",
            access_key = self.access_key,
            scope = scope_string(self.datetime, self.region, self.service),
            signed_headers = signed_headers,
            signature = signature
        );
        debug!("sign_string: {}", sign_string);
        sign_string
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


/// Sign the request with the AWS V4 signature
/// # Arguments
/// * `request` - The request to sign
/// * `cos_map` - The COS map item containing the credentials
/// # Returns
/// * `Ok(())` if the request was signed successfully
/// * `Err` if there was an error signing the request
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
    // let payload_hash = if method == "GET" || method == "HEAD" || method == "DELETE" {
    //     // spec uses empty‑body hash for reads
    //     &sha256::digest(b"")
    // } else {
    //     // for streaming uploads we sign UNSIGNED‑PAYLOAD
    //     "UNSIGNED-PAYLOAD"
    // };
    let payload_hdr = request
        .headers
        .get("x-amz-content-sha256")
        .and_then(|v| v.to_str().ok());

    let payload_hash = match payload_hdr {
        // client already supplied one → keep it verbatim
        Some(h) => h,                                       

        // empty-body requests (GET/HEAD/DELETE) → spec hash of “”
        None if matches!(method.as_str(), "GET" | "HEAD" | "DELETE") => 
            &sha256::digest(b""),

        // default for uploads over TLS
        _ => "UNSIGNED-PAYLOAD",
    };

    let payload_hash_value = payload_hash.to_string();
    request.insert_header("x-amz-content-sha256", payload_hash_value.clone())?;

    let body_bytes: &[u8] = match payload_hash_value.clone().as_str() {
        // empty body → empty slice
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" => &[], // sha256 hash of empty string
        "UNSIGNED-PAYLOAD" => b"UNSIGNED-PAYLOAD",
        "STREAMING-UNSIGNED-PAYLOAD-TRAILER" => b"STREAMING-UNSIGNED-PAYLOAD-TRAILER",
        "STREAMING-AWS4-HMAC-SHA256-PAYLOAD" => b"STREAMING-AWS4-HMAC-SHA256-PAYLOAD",
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

    let mut signer = auth_header;

    // if payload_hash_value.starts_with("STREAMING-") {
    //     // don’t hash the literal bytes – embed the magic string itself
    //     signer.set_payload_override(payload_hash_value.to_string());
    // }
    // if payload_hash_value != "UNSIGNED-PAYLOAD" {
    //     signer.set_payload_override(payload_hash_value.to_string());
    // }

    signer.set_payload_override(payload_hash_value.clone());

    let signature = signer.sign();
    debug!("{:#?}", signature);

    request.insert_header(
        "Authorization",
        http::header::HeaderValue::from_str(&signature)?,
    )?;

    Ok(())
}


/// Core signature validation: compares provided vs computed
async fn signature_is_valid_core(
    method: &str,
    provided_signature: &str,
    region: &str,
    service: &str,
    datetime: DateTime<Utc>,
    full_url: &str,
    headers: &HeaderMap,
    payload_override: Option<String>,
    access_key: &str,
    secret_key: &str,
    signed_headers: &Vec<String>,
    body_bytes: &[u8],
) -> Result<bool, Box<dyn std::error::Error>> {
    // Build AwsSign for authorization header style
    debug!("{:#?}", &headers);
    let mut signer = AwsSign::new(
        method,
        full_url,
        &datetime,
        headers,
        region,
        access_key,
        secret_key,
        service,
        body_bytes,
        Some(&signed_headers),
    );

    // if payload_hash.starts_with("STREAMING-") {
    //     // don’t hash the literal bytes – embed the magic string itself
    //     signer.set_payload_override(payload_hash.to_string());
    // }

    if let Some(ov) = payload_override {
        dbg!("payload_override: {}", &ov);
        signer.set_payload_override(ov);
    }

    let signature = signer.sign();
    let computed = signature.split("Signature=").nth(1).unwrap_or_default();
    debug!("Provided signature: {}", provided_signature);
    debug!("Computed signature: {}", computed);
    Ok(computed == provided_signature)
}

/// Validate standard S3 Authorization header
pub async fn signature_is_valid_for_request(
    auth_header: &str,
    session: &Session,
    secret_key: &str,
) -> Result<bool, Box<dyn std::error::Error>> {

    
    let (_, local_access_key) = parse_token_from_header(auth_header)
        .map_err(|_| pingora::Error::new_str("Failed to parse token"))?;
    let local_access_key = local_access_key.to_string();
    if local_access_key.is_empty() {
        error!("Missing access key");
        return Ok(false);
    }
    let provided_signature = auth_header
        .split("Signature=")
        .nth(1)
        .ok_or("Missing Signature")?;

    let (_, (region, service)) = parse_credential_scope(auth_header)
        .map_err(|_| pingora::Error::new_str("Invalid Credential scope"))?;

    let method = session.req_header().method.to_string();
    // Parse date header
    let dt_header = session.req_header().headers.get("x-amz-date").unwrap().to_str()?;
    let datetime = NaiveDateTime::parse_from_str(dt_header, LONG_DATETIME)?.and_utc();


    let content_sha256 = session
        .req_header()
        .headers
        .get("x-amz-content-sha256")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| pingora::Error::new_str("Missing x-amz-content-sha256 header"))?;

    let (body_bytes, payload_override) = if content_sha256 == "UNSIGNED-PAYLOAD" {
        (b"UNSIGNED-PAYLOAD" as &[u8], None)
    } else if content_sha256 == "STREAMING-AWS4-HMAC-SHA256-PAYLOAD" {
        (b"STREAMING-AWS4-HMAC-SHA256-PAYLOAD".as_ref(),
         Some("STREAMING-AWS4-HMAC-SHA256-PAYLOAD".to_string()))
    } else {
        // we don't have the raw body here, but we do have its hash:
        // tell AwsSign to use this string directly
        (&[] as &[u8], Some(content_sha256.to_owned()))
    };

    // Build full URL
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

    // Signed headers list
    let signed_headers_str = auth_header
        .split("SignedHeaders=")
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap();

    let signed_headers: Vec<String> = signed_headers_str.split(';').map(str::to_string).collect();

    signature_is_valid_core(
        &method,
        provided_signature,
        region,
        service,
        datetime,
        &full_url,
        &session.req_header().headers,
        payload_override,
        &local_access_key,
        secret_key,
        &signed_headers,
        body_bytes,
    )
    .await
}


/// Validate presigned URL signature
pub async fn signature_is_valid_for_presigned(
    session: &Session,
    secret_key: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    // Extract query params from the URL

    let uri = session.req_header().uri.to_string();
    let full_uri = if uri.starts_with('/') {
        // build an absolute URL: scheme://host + path?query
        let host = session
            .req_header()
            .headers
            .get("host")
            .ok_or("Missing host header")?
            .to_str()?;
        format!("https://{}{}", host, uri)
    } else {
        uri
    };

    
    let mut url = Url::parse(&full_uri)?; 
    debug!("full_url: {}", url);
    let mut provided_signature = None;
    let mut qp: Vec<(String,String)> = vec![];
    for (k, v) in url.query_pairs() {
        if k == "X-Amz-Signature" {
            provided_signature = Some(v.into_owned());
        } else {
            qp.push((k.into_owned(), v.into_owned()));
        }
    }
    let provided_signature = provided_signature.ok_or("Missing X-Amz-Signature")?;
    
    // rebuild query string without the signature
    qp.sort();
    let new_query = qp.iter()
                      .map(|(k,v)| format!("{k}={v}"))
                      .collect::<Vec<_>>()
                      .join("&");
    url.set_query(Some(&new_query));
    
    // params map (also without the signature)
    let params: HashMap<_, _> = qp.into_iter().collect();
    debug!("params: {:?}", params);
    debug!("url: {:?}", url);

    debug!("provided signature: {}", provided_signature);
    let credential = params
        .get("X-Amz-Credential")
        .ok_or("Missing X-Amz-Credential")?;

    debug!("credential: {}", credential);

    // Parse credential: <access_key>/<date>/<region>/<service>/aws4_request
    let mut parts = credential.split('/');
    let access_key = parts.next().ok_or("Malformed Credential")?;
    let _credential_date = parts.next().ok_or("Malformed Credential")?;
    let region = parts.next().ok_or("Malformed Credential")?;
    let service = parts.next().ok_or("Malformed Credential")?;

    debug!("access_key: {}", access_key);
    debug!("region: {}", region);
    debug!("service: {}", service);

    // Parse date from query
    let date_str = params
        .get("X-Amz-Date")
        .ok_or("Missing X-Amz-Date")?;
    let datetime = NaiveDateTime::parse_from_str(date_str, LONG_DATETIME)?.and_utc();

    debug!("datetime: {}", datetime);

    let body_bytes: &[u8] = b"UNSIGNED-PAYLOAD";
    let payload_override = None;  

    debug!("body_bytes: {:?}", body_bytes);

    // collect signed headers list
    let signed_headers = params
        .get("X-Amz-SignedHeaders")
        .unwrap()
        .split(';')
        .map(str::to_string)
        .collect::<Vec<_>>();

    let mut signed_hdrs = HeaderMap::new();

    let host_header = match url.port_or_known_default() {
        Some(443) | Some(80) | None => url.host_str().unwrap().to_owned(),
        Some(p)                    => format!("{}:{}", url.host_str().unwrap(), p),
    };

    signed_hdrs.insert("host", host_header.parse()?);
    
    // copy any additional headers that appear in X-Amz-SignedHeaders (rare)
    for h in &["x-amz-date", "x-amz-content-sha256", "range", "x-amz-security-token"] {
        if signed_headers.contains(&h.to_string()) {
            if let Some(v) = session.req_header().headers.get(*h) {
                signed_hdrs.insert(*h, v.clone());
            }
        }
    }



    debug!("signed_headers: {:?}", signed_headers);
    // delegate to core validator
    signature_is_valid_core(
        session.req_header().method.as_str(),
        &provided_signature,
        region,
        service,
        datetime,
        url.as_str(),
        &signed_hdrs,
        payload_override,
        access_key,
        secret_key,
        &signed_headers,
        body_bytes,
    ).await
}



/// Build a stream whose items are *already* wrapped in
/// "AWS-chunk-signed" envelopes.
///
/// * `body`        - raw payload implementing `AsyncRead`  
/// * `signing_key` - result of the usual `signing_key()` step  
/// * `scope`       - e.g. `"20250501/eu-west-3/s3/aws4_request"`  
/// * `ts`          - the `X-Amz-Date` you put in the header (`YYYYMMDDThhmmssZ`)  
/// * `seed_sig`    - the `Signature=` value you computed for the
///                   *headers* (the one that goes into `Authorization:`)
///
/// ```text
/// ┌──── header chunk ────┐┌── data ─┐┌─ CRLF ─┐
/// <hex-len>;chunk-signature=<sig>\r\n<bytes>\r\n
/// ```
///
/// The very last frame is
/// ```text
/// 0;chunk-signature=<final-sig>\r\n\r\n
/// ```
pub async fn wrap_streaming_body(
    session: &mut Session,
    upstream_request: &mut RequestHeader,
    region: &str,
    access_key: &str,
    secret_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. pull the COMPLETE body from the client
    let body: Bytes = session.read_request_body().await.expect("Failed to read request body").unwrap();

    // 2. overwrite the x-amz-* headers so that we can sign UNSIGNED-PAYLOAD
    upstream_request.insert_header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")?;
    upstream_request.remove_header("x-amz-decoded-content-length");
    upstream_request.insert_header("content-length", body.len().to_string())?;

    // 3. resign
    let ts = chrono::Utc::now();
    let url = upstream_request.uri.to_string();
    upstream_request.insert_header("x-amz-date", ts.format("%Y%m%dT%H%M%SZ").to_string())?;
    let signer = AwsSign::new(
        upstream_request.method.as_str(),
        &url,
        &ts,
        &upstream_request.headers,
        region,
        access_key,
        secret_key,
        "s3",
        b"UNSIGNED-PAYLOAD",
        None,
    );
    let auth = signer.sign();
    upstream_request.insert_header("authorization", auth)?;

    let end_of_stream: bool = session.is_body_done();

    // 4. finally attach the body
    session.write_response_body(Some(body), end_of_stream).await?;

    Ok(())
}

const EMPTY_HASH: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Handles running signature state for AWS-chunked uploads
pub struct ChunkSigner {
    signing_key: Vec<u8>,
    scope: String,
    ts: String,
    prev_sig: String,
}

impl ChunkSigner {
    /// create a new signer.  
    /// *`seed_signature`* is the **signature you put in the `Authorization` header**
    pub fn new(
        signing_key: Vec<u8>,
        scope: String,
        ts: String,
        seed_signature: String,
    ) -> Self {
        Self {
            signing_key,
            scope,
            ts,
            prev_sig: seed_signature,
        }
    }

    /// sign one payload chunk and return the wire bytes **and** update internal state.
    pub fn sign_chunk(
        &mut self,
        chunk: Bytes,
    ) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        // 1. Hash of the chunk payload
        let chunk_hash = hex::encode(sha256::digest(chunk.as_ref()));

        // 2. String to sign for this chunk (AWS spec §4)
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256-PAYLOAD\n{}\n{}\n{}\n{}\n{}",
            self.ts,
            self.scope,
            self.prev_sig,
            EMPTY_HASH, // hash of empty string
            chunk_hash
        );

        // 3. HMAC with the derived signing key
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.signing_key);
        let sig = hex::encode(hmac::sign(&key, string_to_sign.as_bytes()));

        // 4. Build the chunk header:  "<hexlen>;chunk-signature=<sig>\r\n"
        let mut buf = BytesMut::with_capacity(chunk.len() + 128);
        buf.put(format!("{:x};chunk-signature={}\r\n", chunk.len(), sig).as_bytes());
        buf.put(chunk);
        buf.put_slice(b"\r\n");

        // 5. advance running signature
        self.prev_sig = sig;

        Ok(buf.freeze())
    }

    /// Final 0-length chunk (must be sent after the body)
    pub fn final_chunk(
        &mut self,
    ) -> Bytes {
        // sign an empty payload chunk (same steps, len = 0)
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256-PAYLOAD\n{}\n{}\n{}\n{}\n{}",
            self.ts,
            self.scope,
            self.prev_sig,
            EMPTY_HASH,
            EMPTY_HASH
        );
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.signing_key);
        let sig = hex::encode(hmac::sign(&key, string_to_sign.as_bytes()));

        Bytes::from(format!("0;chunk-signature={}\r\n\r\n", sig))
    }
}

pub fn resign_streaming_request(
    req: &mut RequestHeader,
    region: &str,
    access_key: &str,
    secret_key: &str,
    ts: DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {

    // let ts = chrono::Utc::now();
    req.insert_header("x-amz-date", ts.format("%Y%m%dT%H%M%SZ").to_string())?;


    let url = req.uri.to_string();
    let signer = AwsSign::new(
        req.method.as_str(),
        &url,
        &ts,
        &req.headers,
        region,
        access_key,
        secret_key,
        "s3",
        b"STREAMING-AWS4-HMAC-SHA256-PAYLOAD",
        None,
    );


    let auth = signer.sign();
    req.insert_header("authorization", auth)?;

    Ok(())
}


/// Maximum chunk payload we read from the client before signing.
/// (4 MiB is what the Java/AWS SDKs use, but any size works.)
// const DEFAULT_CHUNK: usize = 4 * 1024 * 1024;

// pub fn wrap_streaming_body_reader<R>(
//     body_reader: R,
//     signing_key: Vec<u8>,
//     scope: String,
//     ts: String,
//     seed_signature: String,
// ) -> impl Stream<Item = Result<bytes::Bytes, std::io::Error>>
// where
//     R: Stream<Item = Result<bytes::Bytes, pingora::Error>> + Unpin + Send + 'static,
// {
//     // sign_chunk prepends the S3 chunk header & signature
//     body_reader
//         .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
//         .and_then(move |chunk| async move {
//             Ok(ChunkSigner::sign_chunk(
//                 chunk,
//                 &signing_key,
//                 &scope,
//                 &ts,
//                 &seed_signature,
//             )?)
//         })
// }

// pin_project! {
//     pub struct StreamingSignerReader<R> {
//         #[pin] inner: R,
//         buf: BytesMut,
//         region: String,
//         access_key: String,
//         secret_key: String,
//         ts: chrono::DateTime<chrono::Utc>,
//         prior_signature: String,
//         eof: bool,
//     }
// }

// impl<R> StreamingSignerReader<R>
// where
//     R: AsyncRead + Unpin,
// {
//     pub fn new(
//         inner: R,
//         region: String,
//         access_key: String,
//         secret_key: String,
//         ts: chrono::DateTime<chrono::Utc>,
//         seed_sig: String,
//     ) -> Self {
//         Self {
//             inner,
//             buf: BytesMut::new(),
//             region,
//             access_key,
//             secret_key,
//             ts,
//             prior_signature: seed_sig,
//             eof: false,
//         }
//     }

//     /// Read *one* chunk payload from `inner`, sign it and stage it in `buf`.
//     async fn fill_buf(mut self: Pin<&mut Self>) -> std::io::Result<()> {
//         if self.eof {
//             return Ok(());
//         }

//         let mut payload = vec![0u8; DEFAULT_CHUNK];
//         let n = self.inner.read(&mut payload).await?;
//         if n == 0 {
//             // Final 0-length chunk
//             self.stage_chunk(Bytes::new());
//             self.eof = true;
//             return Ok(());
//         }
//         payload.truncate(n);
//         self.stage_chunk(Bytes::from(payload));
//         Ok(())
//     }

//     fn stage_chunk(&mut self, payload: Bytes) {
//         // 1/ compute SHA-256 hash of the chunk payload
//         let chunk_hash = sha256::digest(payload.as_ref());

//         // 2/ build canonical string for the *chunk*
//         //    https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-streaming.html
//         let string_to_sign = format!(
//             "AWS4-HMAC-SHA256-PAYLOAD\n{}\n{}/s3/aws4_request\n{}\n{}\n{}",
//             self.ts.format("%Y%m%dT%H%M%SZ"),
//             self.ts.format("%Y%m%d"),
//             self.prior_signature,
//             hex::encode([0u8; 32]), // empty-string hash for headers
//             chunk_hash
//         );

//         // 3/ get signing key for the day and region
//         let sig_key = signing_key(&self.ts, &self.secret_key, &self.region, "s3")
//             .expect("streaming key");

//         let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &sig_key);
//         let tag = ring::hmac::sign(&key, string_to_sign.as_bytes());
//         let signature_hex = hex::encode(tag);

//         // 4/ assemble chunk: <hexlen>;<sig>\r\n<payload>\r\n
//         let mut chunk = BytesMut::new();
//         let len_hex = format!("{:x}", payload.len());
//         chunk.extend_from_slice(len_hex.as_bytes());
//         chunk.extend_from_slice(b";chunk-signature=");
//         chunk.extend_from_slice(signature_hex.as_bytes());
//         chunk.extend_from_slice(b"\r\n");
//         chunk.extend_from_slice(&payload);
//         chunk.extend_from_slice(b"\r\n");

//         self.prior_signature = signature_hex;
//         self.buf.extend_from_slice(&chunk);
//     }
// }

// impl<R: AsyncRead + Unpin> AsyncRead for StreamingSignerReader<R> {
//     fn poll_read(
//         self: Pin<&mut Self>,
//         cx: &mut Context<'_>,
//         out: &mut tokio::io::ReadBuf<'_>,
//     ) -> Poll<std::io::Result<()>> {
//         // 1. fast-path: drain any buffered bytes
//         {
//             let this = self.as_ref().project();
//             if !this.buf.is_empty() {
//                 let n = std::cmp::min(out.remaining(), this.buf.len());
//                 out.put_slice(&this.buf.split_to(n));
//                 return Poll::Ready(Ok(()));
//             }
//         }

//         // 2. refill the buffer
//         let mut fut = Box::pin(self.as_mut().fill_buf());      // <-- changed line
//         futures::ready!(fut.as_mut().poll(cx))?;

//         // 3. project again to copy freshly-staged data to `out`
//         let this = self.project();
//         if this.buf.is_empty() {
//             return Poll::Ready(Ok(())); // EOF
//         }
//         let n = std::cmp::min(out.remaining(), this.buf.len());
//         out.put_slice(&this.buf.split_to(n));
//         Poll::Ready(Ok(()))
//     }
// }


#[derive(Debug)]
pub struct StreamingState {
    region: String,
    _access_key: String,
    _secret_key: String,
    ts: chrono::DateTime<chrono::Utc>,
    prior_sig: String,
    signing_key: Vec<u8>,
}

impl StreamingState {
    pub fn new(
        region: String,
        access_key: String,
        secret_key: String,
        ts: chrono::DateTime<chrono::Utc>,
        seed_signature: String,
    ) -> Self {
        let signing_key = signing_key(&ts, &secret_key, &region, "s3")
            .expect("signing key");
        Self {
            region,
            _access_key: access_key,
            _secret_key: secret_key,
            ts,
            prior_sig: seed_signature,
            signing_key,
        }
    }

    pub fn sign_chunk(&mut self, payload: &[u8]) -> io::Result<Bytes> {
        let sig = compute_chunk_signature(
            payload,
            &self.signing_key,
            &self.prior_sig,
            &self.ts,
            &self.region,
        )?;
        self.prior_sig = sig.clone();
        Ok(build_chunk_frame(payload, &sig))
    }
    
    pub fn final_chunk(&mut self) -> io::Result<Bytes> {
        let sig = compute_chunk_signature(
            &[],
            &self.signing_key,
            &self.prior_sig,
            &self.ts,
            &self.region,
        )?;
        Ok(build_chunk_frame(&[], &sig))
    }
}


// fn sha256_hex(data: &[u8]) -> String {
//     let mut hasher = Sha256::new();
//     hasher.update(data);
//     hex::encode(hasher.finalize())
// }

fn sha256_hex(data: &[u8]) -> String {
    sha256::digest(data)
}

/// Pre‑computed because it is constant for every chunk.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Calculate the per‑chunk signature (section *Defining the Chunk Body* of the
/// AWS doc).  
/// * `signing_key` is the value you built once from the secret key  
/// * `prior_sig`  is the *seed* (first chunk) or the previous chunk’s signature
fn compute_chunk_signature(
    payload: &[u8],
    signing_key: &[u8],
    prior_sig: &str,
    ts: &DateTime<Utc>,
    region: &str,
) -> io::Result<String> {
    // 1️⃣  Build the String‑To‑Sign
    let time = ts.format("%Y%m%dT%H%M%SZ").to_string();
    let scope = format!("{}/{}/s3/aws4_request", ts.format("%Y%m%d"), region);

    let string_to_sign = format!(
        "AWS4-HMAC-SHA256-PAYLOAD\n{}\n{}\n{}\n{}\n{}",
        time,
        scope,
        prior_sig,
        EMPTY_SHA256,             // SHA256("")
        sha256_hex(payload),
    );

    // 2️⃣  HMAC it
    let key = hmac::Key::new(hmac::HMAC_SHA256, signing_key);
    let sig = hmac::sign(&key, string_to_sign.as_bytes());
    Ok(hex::encode(sig.as_ref()))
    
}

/// Wrap a signed payload frame into the final on‑the‑wire representation.
fn build_chunk_frame(payload: &[u8], sig: &str) -> Bytes {
    let mut buf = BytesMut::with_capacity(payload.len() + sig.len() + 64);
    // <hexlen>;chunk-signature=<sig>\r\n
    use std::fmt::Write;
    write!(&mut buf, "{:x};chunk-signature={}\r\n", payload.len(), sig).unwrap();
    buf.extend_from_slice(payload);
    buf.extend_from_slice(b"\r\n");
    buf.freeze()
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
