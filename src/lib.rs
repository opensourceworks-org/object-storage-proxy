//! # object-storage-proxy
//!
//! A fast, in-process reverse proxy for **AWS S3**, **IBM Cloud Object Storage (COS)** and other S3-compatible object storage services,
//! with a Python interface for custom authentication and credential management.
//!
//! The proxy is built on top of [Pingora](https://github.com/cloudflare/pingora) and exposed
//! to Python via [PyO3](https://pyo3.rs). It handles:
//!
//! * **AWS Signature Version 4** re-signing — incoming requests are validated and
//!   then re-signed with backend credentials before being forwarded.
//! * **Presigned URL enforcement** — optional per-URL usage limits prevent replay abuse.
//! * **IBM IAM bearer-token exchange** — API keys are automatically exchanged for
//!   short-lived IAM tokens and cached.
//! * **Pluggable Python callbacks** — supply an async validator and/or a credential
//!   fetcher callable from Python to integrate with any auth backend.
//!
//! ## Quick start (Python)
//!
//! ```python
//! from object_storage_proxy import ProxyServerConfig, start_server
//!
//! config = ProxyServerConfig(
//!     cos_map={
//!         "my-bucket": {
//!             "host": "s3.eu-west-3.amazonaws.com",
//!             "port": 443,
//!             "access_key": "AKIAIOSFODNN7EXAMPLE",
//!             "secret_key": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
//!             "region": "eu-west-3",
//!         }
//!     },
//!     http_port=6190,
//! )
//! start_server(config)
//! ```

#![warn(clippy::all)]
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use credentials::signer::{
    self, resign_streaming_request, signature_is_valid_for_presigned,
    signature_is_valid_for_request,
};
use dashmap::DashMap;
use dotenv::dotenv;
use http::uri::Authority;
use http::{Method, StatusCode, Uri};
use parsers::cos_map::{CosMapItem, parse_cos_map};
use parsers::keystore::parse_hmac_list;
use pingora::Result;
use pingora::http::ResponseHeader;
use pingora::proxy::{ProxyHttp, Session};
use pingora::server::Server;
use pingora::upstreams::peer::HttpPeer;
use pyo3::prelude::*;
use pyo3::types::{PyModule, PyModuleMethods};
use pyo3::{Bound, PyResult, Python, pyclass, pyfunction, pymodule, wrap_pyfunction};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

// use utils::functions::inspect_callable_signature;

use std::collections::HashMap;
use std::fmt::Debug;

use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::time::ChronoLocal;

pub mod parsers;
use parsers::credentials::{parse_presigned_params, parse_token_from_header};
use parsers::path::{parse_path, parse_query};

pub mod credentials;
use credentials::{
    secrets_proxy::{SecretsCache, get_bearer, get_credential_for_bucket},
    signer::sign_request,
};

pub mod utils;
use utils::banner::print_banner;
use utils::response::write_error_response_with_header;
use utils::validator::{AuthCache, validate_request};

static REQ_COUNTER: AtomicUsize = AtomicUsize::new(0);
static REQ_COUNTER_ENABLED: AtomicBool = AtomicBool::new(false);
const DEFAULT_SERVER_NAME: &str = "<osp⚡>";

/// Thread-safe hit counter for presigned URLs.
///
/// Tracks how many times each presigned URL has been used so that a configurable
/// maximum can be enforced.  Regular (re-signed) requests are **not** tracked —
/// the aws-cli issues parallel range-GET sub-requests for the same object, which
/// would exhaust a small limit instantly.
///
/// Internally backed by a [`DashMap`] so that concurrent access from multiple
/// Pingora worker threads never requires a global lock.
#[derive(Clone)]
pub struct UrlTracker {
    /// Per-URL hit counters.
    pub counts: Arc<DashMap<String, usize>>,
}

impl Default for UrlTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl UrlTracker {
    /// Create a new, empty tracker.
    pub fn new() -> Self {
        UrlTracker {
            counts: Arc::new(DashMap::new()),
        }
    }

    /// Increment the hit counter for `url` by one.
    pub fn track(&self, url: &str) {
        let mut entry = self.counts.entry(url.to_string()).or_insert(0);
        *entry += 1;
        debug!(url, count = *entry, "tracking presigned URL");
    }

    /// Return the current hit count for `url`, or `None` if it has never been tracked.
    pub fn get(&self, url: &str) -> Option<usize> {
        self.counts.get(url).map(|v| *v)
    }

    /// Return a snapshot of all tracked URLs and their counts.
    pub fn get_all(&self) -> Vec<(String, usize)> {
        self.counts
            .iter()
            .map(|e| (e.key().clone(), *e.value()))
            .collect()
    }
}

/// Configuration object for :pyfunc:`object_storage_proxy.start_server`.
///
/// Parameters
/// ----------
/// cos_map:
///    A dictionary mapping bucket names to their respective COS configuration.
///   Each entry should contain the following
///   keys:
///   - host: The COS endpoint (e.g., "s3.eu-de.cloud-object-storage.appdomain.cloud")
///   - port: The port number (e.g., 443)
///   - api_key/apikey: The API key for the bucket (optional)
///   - ttl/time-to-live: The time-to-live for the API key in seconds (optional)
///
/// bucket_creds_fetcher:
///     Optional Python async callable that fetches the API key for a bucket.
///     The callable should accept a single argument, the bucket name.
///     It should return a string containing the API key.
/// http_port:
///     The HTTP port to listen on.
/// https_port:
///     The HTTPS port to listen on.
/// validator:
///     Optional Python async callable that validates the request.
///     The callable should accept two arguments, the token and the bucket name.
///     It should return a boolean indicating whether the request is valid.
/// threads:
///     Optional number of threads to use for the server.
///     If not specified, the server will use a single thread.
///
#[pyclass]
#[pyo3(name = "ProxyServerConfig")]
#[derive(Debug)]
pub struct ProxyServerConfig {
    #[pyo3(get, set)]
    pub bucket_creds_fetcher: Option<Py<PyAny>>,

    #[pyo3(get, set)]
    pub cos_map: PyObject,

    #[pyo3(get, set)]
    pub http_port: Option<u16>,

    #[pyo3(get, set)]
    pub https_port: Option<u16>,

    #[pyo3(get, set)]
    pub validator: Option<Py<PyAny>>,

    #[pyo3(get, set)]
    pub threads: Option<usize>,

    #[pyo3(get, set)]
    pub verify: Option<bool>,

    #[pyo3(get, set)]
    pub hmac_keystore: PyObject,

    #[pyo3(get, set)]
    pub skip_signature_validation: Option<bool>,

    #[pyo3(get, set)]
    pub hmac_fetcher: Option<Py<PyAny>>,

    #[pyo3(get, set)]
    pub max_presign_url_usage_attempts: Option<usize>,

    #[pyo3(get, set)]
    pub server_name: String,

    /// Port to expose the Prometheus `/metrics` scrape endpoint on.
    ///
    /// Only effective when the `metrics` Cargo feature is enabled.
    /// When `None` (the default) no metrics endpoint is started.
    #[pyo3(get, set)]
    pub metrics_port: Option<u16>,
}

impl Default for ProxyServerConfig {
    fn default() -> Self {
        ProxyServerConfig {
            cos_map: Python::with_gil(|py| py.None()),
            bucket_creds_fetcher: None,
            http_port: None,
            https_port: None,
            validator: None,
            threads: Some(1),
            verify: None,
            hmac_keystore: Python::with_gil(|py| py.None()),
            skip_signature_validation: Some(false),
            hmac_fetcher: None,
            max_presign_url_usage_attempts: Some(3),
            server_name: "<osp⚡>".to_string(),
            metrics_port: None,
        }
    }
}

#[pymethods]
impl ProxyServerConfig {
    #[new]
    #[pyo3(
        signature = (
            cos_map,
            hmac_keystore = None,
            bucket_creds_fetcher = None,
            http_port = None,
            https_port = None,
            validator = None,
            threads = Some(1),
            verify = None,
            skip_signature_validation = Some(false),
            hmac_fetcher = None,
            max_presign_url_usage_attempts = Some(3),
            server_name = "<osp⚡>".to_string(),
            metrics_port = None,
        )
    )]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cos_map: PyObject,
        hmac_keystore: Option<PyObject>,
        bucket_creds_fetcher: Option<PyObject>,
        http_port: Option<u16>,
        https_port: Option<u16>,
        validator: Option<PyObject>,
        threads: Option<usize>,
        verify: Option<bool>,
        skip_signature_validation: Option<bool>,
        hmac_fetcher: Option<PyObject>,
        max_presign_url_usage_attempts: Option<usize>,
        server_name: String,
        metrics_port: Option<u16>,
    ) -> Self {
        ProxyServerConfig {
            cos_map,
            hmac_keystore: hmac_keystore.unwrap_or_else(|| Python::with_gil(|py| py.None())),
            bucket_creds_fetcher,
            http_port,
            https_port,
            validator,
            threads,
            verify,
            skip_signature_validation,
            hmac_fetcher,
            max_presign_url_usage_attempts,
            server_name,
            metrics_port,
        }
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!(
            "ProxyServerConfig(http_port={}, https_port={}, threads={:?})",
            self.http_port.unwrap_or(0),
            self.https_port.unwrap_or(0),
            self.threads
        ))
    }
}

/// The core Pingora proxy handler.
///
/// One instance is created per server and shared (via [`Arc`]) across all worker
/// threads.  It implements [`ProxyHttp`] and drives the full request lifecycle:
/// signature validation -> authorization -> credential injection -> upstream routing.
pub struct MyProxy {
    cos_endpoint: String,
    cos_mapping: Arc<RwLock<HashMap<String, CosMapItem>>>,
    hmac_keystore: Arc<RwLock<HashMap<String, String>>>,
    secrets_cache: SecretsCache,
    auth_cache: AuthCache,
    validator: Option<PyObject>,
    bucket_creds_fetcher: Option<PyObject>,
    verify: Option<bool>,
    skip_signature_validation: Option<bool>,
    hmac_fetcher: Option<PyObject>,
    tracker: UrlTracker,
    max_presign_url_usage_attempts: Option<usize>,
    #[allow(dead_code)]
    server_name: String,
    /// Cached result of `inspect.signature` on the validator callable:
    /// `true`  = validator accepts a third `request: dict` argument.
    /// `false` = validator only takes `(token, bucket)`.
    /// `None`  = no validator configured.
    validator_takes_request: Option<bool>,
}

/// Per-request context threaded through the Pingora middleware chain.
///
/// A fresh `MyCtx` is created by [`MyProxy::new_ctx`] for every incoming
/// connection and is discarded when the request completes.
pub struct MyCtx {
    cos_mapping: Arc<RwLock<HashMap<String, CosMapItem>>>,
    hmac_keystore: Arc<RwLock<HashMap<String, String>>>,
    secrets_cache: SecretsCache,
    auth_cache: AuthCache,
    validator: Option<PyObject>,
    bucket_creds_fetcher: Option<PyObject>,
    hmac_fetcher: Option<PyObject>,
    is_presigned: Option<bool>,
    stream_state: Option<signer::StreamingState>,
    /// Bucket name parsed from the request path in `request_filter`, reused by
    /// later stages to avoid redundant `parse_path` calls and map lock acquires.
    cached_bucket: Option<String>,
    /// CosMapItem resolved in `request_filter` and reused by `upstream_peer`
    /// to avoid a second `cos_mapping` RwLock read on every request.
    /// TODO(perf-2): done — see upstream_peer
    cached_bucket_config: Option<CosMapItem>,
}

// impl MyCtx {
//     fn streaming(&mut self) -> &mut signer::StreamingState {
//         self.stream_state.as_mut().expect("stream_state not initialised")
//     }
// }

#[async_trait]
impl ProxyHttp for MyProxy {
    type CTX = MyCtx;
    fn new_ctx(&self) -> Self::CTX {
        MyCtx {
            cos_mapping: Arc::clone(&self.cos_mapping),
            hmac_keystore: Arc::clone(&self.hmac_keystore),
            secrets_cache: self.secrets_cache.clone(),
            auth_cache: self.auth_cache.clone(),
            validator: self
                .validator
                .as_ref()
                .map(|v| Python::with_gil(|py| v.clone_ref(py))),
            bucket_creds_fetcher: self
                .bucket_creds_fetcher
                .as_ref()
                .map(|v| Python::with_gil(|py| v.clone_ref(py))),
            hmac_fetcher: self
                .hmac_fetcher
                .as_ref()
                .map(|v| Python::with_gil(|py| v.clone_ref(py))),
            is_presigned: None,
            stream_state: None,
            cached_bucket: None,
            cached_bucket_config: None,
        }
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        debug!("upstream_peer::start");
        #[cfg(feature = "metrics")]
        utils::metrics::ACTIVE_CONNECTIONS.inc();
        if REQ_COUNTER_ENABLED.load(Ordering::Relaxed) {
            let new_val = REQ_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
            debug!("Request count: {}", new_val);
        }

        let hdr_bucket = ctx.cached_bucket.clone().unwrap_or_else(|| {
            let path = session.req_header().uri.path();
            parse_path(path)
                .map(|(_, (b, _))| b.to_owned())
                .unwrap_or_default()
        });

        // Use the config cached by request_filter; fall back to a fresh lock
        // read only for the rare case where upstream_peer is called without a
        // preceding request_filter (e.g. direct Pingora internal calls).
        let bucket_config = if ctx.cached_bucket_config.is_some() {
            ctx.cached_bucket_config.clone()
        } else {
            let map = ctx.cos_mapping.read().await;
            map.get(&hdr_bucket).cloned()
        };

        let addressing_style = bucket_config
            .as_ref()
            .and_then(|c| c.addressing_style.as_deref())
            .unwrap_or("virtual");

        let endpoint = match &bucket_config {
            Some(config) => {
                if addressing_style == "path" {
                    config.host.clone()
                } else {
                    format!("{}.{}", hdr_bucket, config.host)
                }
            }
            None => format!("{}.{}", hdr_bucket, self.cos_endpoint),
        };

        let port = bucket_config.as_ref().map(|c| c.port).unwrap_or(443);

        let addr = (endpoint.clone(), port);

        let endpoint_is_tls = bucket_config.as_ref().and_then(|c| c.tls).unwrap_or(true);

        debug!(endpoint_is_tls, endpoint, "resolved upstream peer");

        let mut peer = Box::new(HttpPeer::new(addr, endpoint_is_tls, endpoint.clone()));
        debug!(?peer, "upstream peer created");

        // todo: make ths configurable

        peer.options.max_h2_streams = 128;
        peer.options.h2_ping_interval = Some(Duration::from_secs(30));

        // peer.options.idle_timeout          = Some(Duration::from_secs(300));
        // peer.options.connection_timeout    = Some(Duration::from_secs(30));
        // peer.options.read_timeout          = Some(Duration::from_secs(300));
        // peer.options.write_timeout         = Some(Duration::from_secs(300));

        debug!("peer: {:#?}", &peer);

        if let Some(verify) = self.verify {
            info!("Verify peer (upstream) certificates disabled!");
            peer.options.verify_cert = verify;
            peer.options.verify_hostname = verify;
        } else {
            peer.options.verify_cert = true;
        }

        debug!("peer: {:#?}", &peer);

        debug!("upstream_peer::end");
        Ok(peer)
    }

    async fn logging(
        &self,
        _session: &mut Session,
        _e: Option<&pingora::Error>,
        ctx: &mut Self::CTX,
    ) {
        #[cfg(feature = "metrics")]
        utils::metrics::ACTIVE_CONNECTIONS.dec();
        let _ = ctx;
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        debug!("request_filter::start");

        // Tracking the request count for presigned URLs only.
        // Regular (re-signed) requests must not be counted — aws-cli issues multiple
        // parallel range-GET requests for the same object (multipart download), so
        // counting every request would exhaust the limit almost immediately.
        let url = session.req_header().uri.to_string();
        let path = session.req_header().uri.path().to_string();
        let is_presigned_url = session
            .req_header()
            .uri
            .query()
            .is_some_and(|q| q.contains("X-Amz-Signature"));
        if is_presigned_url {
            self.tracker.track(&url);
        }
        let tracked_count = self.tracker.get(&url).unwrap_or(0);
        if is_presigned_url && tracked_count > self.max_presign_url_usage_attempts.unwrap_or(3) {
            #[cfg(feature = "metrics")]
            {
                let bucket_label = session
                    .req_header()
                    .uri
                    .path()
                    .split('/')
                    .nth(1)
                    .unwrap_or("-");
                utils::metrics::PRESIGNED_URL_REJECTED_TOTAL
                    .with_label_values(&[bucket_label])
                    .inc();
            }
            warn!(
                url,
                tracked_count,
                max = self.max_presign_url_usage_attempts.unwrap_or(3),
                "presigned URL usage limit exceeded, denying"
            );
            let msg = format!(
                "URL ({}) has been tracked too many times: {} (max={}).  Access Denied!",
                path,
                tracked_count,
                self.max_presign_url_usage_attempts.unwrap_or(3)
            );

            // let mut hdr = ResponseHeader::build(StatusCode::FORBIDDEN, Some(msg.len()))?;
            // hdr.insert_header("content-type", "text/plain")?;
            // hdr.insert_header("server", self.server_name.clone())?;
            // hdr.insert_header("x-content-type-options", "nosniff")?;

            // // Send it
            // session.write_response_header(Box::new(hdr), false).await?;
            // // session
            // //     .write_response_body(Some(msg.into()), true)
            // //     .await?;

            // session.respond_error_with_body(403, msg.into()).await?;
            write_error_response_with_header(session, StatusCode::FORBIDDEN, msg).await?;
            return Ok(true);
        }

        debug!(summary = ?session.request_summary(), "request summary");
        debug!(uri = ?session.req_header().uri, "incoming request URI");
        debug!("request path: {}", session.req_header().uri.path());
        debug!("request method: {}", session.req_header().method);

        if session
            .req_header()
            .headers
            .get("expect")
            .map(|v| {
                v.to_str()
                    .unwrap_or("")
                    .eq_ignore_ascii_case("100-continue")
            })
            .unwrap_or(false)
        {
            return Ok(false);
        };

        let path = session.req_header().uri.path().to_owned();

        // ── ListBuckets short-circuit ────────────────────────────────────────────
        // GET / has no bucket component; parse_path would error.  Return the list
        // of buckets that are configured in the cos_mapping.
        if path == "/" && session.req_header().method == Method::GET {
            let bucket_names: Vec<String> = {
                let map = ctx.cos_mapping.read().await;
                let mut names: Vec<String> = map.keys().cloned().collect();
                names.sort();
                names
            };
            let entries: String = bucket_names
                .iter()
                .map(|n| {
                    format!(
                        "<Bucket><Name>{n}</Name>\
<CreationDate>2000-01-01T00:00:00.000Z</CreationDate></Bucket>"
                    )
                })
                .collect();
            let body = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListAllMyBucketsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Owner><ID>proxy</ID><DisplayName>proxy</DisplayName></Owner>\
<Buckets>{entries}</Buckets>\
</ListAllMyBucketsResult>"
            );
            let body_bytes = Bytes::copy_from_slice(body.as_bytes());
            let mut hdr = ResponseHeader::build(StatusCode::OK, None)?;
            hdr.insert_header("Content-Type", "application/xml")?;
            hdr.insert_header("Content-Length", body_bytes.len().to_string())?;
            hdr.insert_header("Server", DEFAULT_SERVER_NAME)?;
            session.write_response_header(Box::new(hdr), false).await?;
            session.write_response_body(Some(body_bytes), true).await?;
            return Ok(true);
        }

        let parse_path_result = parse_path(&path);
        if parse_path_result.is_err() {
            error!("Failed to parse path: {:?}", parse_path_result);
            return Err(pingora::Error::new_str("Failed to parse path"));
        }

        let (_, (bucket, _uri_path)) = parse_path_result.expect("checked above");

        let hdr_bucket = bucket.to_owned();
        ctx.cached_bucket = Some(hdr_bucket.clone());

        #[cfg(feature = "metrics")]
        {
            let method_label = session.req_header().method.as_str();
            utils::metrics::REQUESTS_TOTAL
                .with_label_values(&[method_label, &hdr_bucket, "received"])
                .inc();
            if is_presigned_url {
                utils::metrics::PRESIGNED_URL_HITS_TOTAL
                    .with_label_values(&[&hdr_bucket])
                    .inc();
            }
        }

        let auth_header = session
            .req_header()
            .headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .map(ToString::to_string)
            .unwrap_or_default();

        let (ttl, bucket_config_init) = {
            let map = ctx.cos_mapping.read().await;
            let cfg = map.get(bucket).cloned();
            let ttl = cfg.as_ref().and_then(|c| c.ttl).unwrap_or(0);
            (ttl, cfg)
        };
        // Cache the resolved config so upstream_peer can skip a second lock read.
        ctx.cached_bucket_config = bucket_config_init.clone();
        let mut access_key: String = String::new();

        if auth_header.is_empty() {
            if let Some(q) = session.req_header().uri.query()
                && q.contains("X-Amz-Credential")
            {
                let (_, p) = parse_presigned_params(&format!("?{q}"))
                    .map_err(|_| pingora::Error::new_str("Failed to parse presigned params"))?;
                access_key = p.access_key.clone();
            }
        } else {
            access_key = parse_token_from_header(&auth_header)
                .map_err(|_| pingora::Error::new_str("Failed to parse access_key"))?
                .1
                .to_string();
        }

        let is_authorized = if let Some(py_cb) = &ctx.validator {
            let is_multipart = session
                .req_header()
                .uri
                .query()
                .is_some_and(|q| q.contains("uploadId="));

            debug!("checking signature");
            if let Some(skip) = self.skip_signature_validation {
                if skip || is_multipart {
                    debug!("Skipping local signature check");
                    // continue
                } else {
                    // presigned
                    debug!("Checking presigned signature");
                    let uri_q = session.req_header().uri.query().unwrap_or("");

                    if auth_header.is_empty() && uri_q.contains("X-Amz-Signature") {
                        ctx.is_presigned = Some(true);

                        // ensure we have the secret_key in the keystore
                        if !ctx.hmac_keystore.read().await.contains_key(&access_key) {
                            debug!(
                                "No key in keystore, trying to fetch via hmac_fetcher for ->{}<-",
                                access_key
                            );
                            // fetch via hmac_fetcher exactly as you do below…
                            if let Some(py_fetcher) = &ctx.hmac_fetcher {
                                // call Python callback
                                let cb = py_fetcher;
                                let secret: PyResult<String> = Python::with_gil(|py| {
                                    cb.call1(py, (&access_key,)).and_then(|r| r.extract(py))
                                });
                                debug!("Got secret: {:#?}", secret);
                                match secret {
                                    Ok(secret_key) => {
                                        debug!("got key and inserting into keystore");
                                        ctx.hmac_keystore
                                            .write()
                                            .await
                                            .insert(access_key.clone(), secret_key);
                                    }
                                    Err(_) => {
                                        // no key -> unauthorized
                                        write_error_response_with_header(
                                            session,
                                            StatusCode::UNAUTHORIZED,
                                            "No key found for presigned URL".to_string(),
                                        )
                                        .await?;
                                        // session.respond_error(401).await?;
                                        return Ok(true);
                                    }
                                }
                            } else {
                                // session.respond_error(401).await?;
                                write_error_response_with_header(
                                    session,
                                    StatusCode::UNAUTHORIZED,
                                    "No key found for presigned URL".to_string(),
                                )
                                .await?;
                                return Ok(true);
                            }
                        }
                        debug!("now checking if the signature is valid for presigned...");
                        let sk = ctx
                            .hmac_keystore
                            .read()
                            .await
                            .get(&access_key)
                            .expect("key was just inserted")
                            .clone();
                        debug!("got secret {} from keystore", sk);
                        debug!("RAW_PATH       = {}", &session.req_header().uri);
                        debug!(
                            "RAW_HOST_HDR   = {:?}",
                            &session.req_header().headers.get("host")
                        );
                        let presigned_result = signature_is_valid_for_presigned(session, &sk)
                            .await
                            .map_err(|e| e.to_string());
                        let ok = match presigned_result {
                            Ok(b) => b,
                            Err(msg) => {
                                error!("presigned-URL validation error: {msg}");
                                if msg.contains("expired") {
                                    write_error_response_with_header(
                                        session,
                                        StatusCode::FORBIDDEN,
                                        format!(
                                            "Presigned URL has expired: {}",
                                            session.req_header().uri.path()
                                        ),
                                    )
                                    .await?;
                                    return Ok(true);
                                }
                                return Err(pingora::Error::new_str("Failed to check signature"));
                            }
                        };
                        debug!("is signature valid?: {}", ok);
                        if !ok {
                            let msg = format!(
                                "Signature invalid for presigned URL: {}",
                                &session.req_header().uri.path()
                            );
                            session.respond_error_with_body(401, msg.into()).await?;
                            return Ok(true);
                        }
                    } else {
                        debug!("processing a regular request");

                        let has_key = {
                            let map = ctx.hmac_keystore.read().await;
                            map.contains_key(&access_key)
                        };
                        if !has_key {
                            if let Some(py_fetcher) = &ctx.hmac_fetcher {
                                // call Python callback
                                let cb = py_fetcher;
                                let secret: PyResult<String> = Python::with_gil(|py| {
                                    cb.call1(py, (&access_key,)).and_then(|r| r.extract(py))
                                });
                                match secret {
                                    Ok(secret_key) => {
                                        ctx.hmac_keystore
                                            .write()
                                            .await
                                            .insert(access_key.clone(), secret_key);
                                    }
                                    Err(_) => {
                                        // no key -> unauthorized
                                        // session.respond_error(401).await?;
                                        write_error_response_with_header(
                                            session,
                                            StatusCode::UNAUTHORIZED,
                                            "No key found for request".to_string(),
                                        )
                                        .await?;
                                        return Ok(true);
                                    }
                                }
                            } else {
                                // session.respond_error(401).await?;
                                write_error_response_with_header(
                                    session,
                                    StatusCode::UNAUTHORIZED,
                                    "No key found for request".to_string(),
                                )
                                .await?;
                                return Ok(true);
                            }
                        }
                        let secret_key = {
                            let map = ctx.hmac_keystore.read().await;
                            map.get(&access_key).cloned()
                        };

                        debug!("checking signature");
                        let sig_ok = match signature_is_valid_for_request(
                            &auth_header,
                            session,
                            &secret_key.expect("key was just inserted"),
                        )
                        .await
                        {
                            Ok(true) => true,
                            Ok(false) => {
                                debug!("Signature invalid");
                                false
                            }
                            Err(err) => {
                                error!("Signature check error: {}", err);
                                false
                            }
                        };

                        // if signature failed, skip further validation
                        if !sig_ok {
                            //  session.respond_error(401).await?;
                            write_error_response_with_header(
                                session,
                                StatusCode::UNAUTHORIZED,
                                "Signature invalid".to_string(),
                            )
                            .await?;
                            return Ok(true);
                        }
                    }
                }
            }
            debug!("Signature check passed, continuing now onto the bespoke validation");
            // Build the query dict here — deferred so requests without a validator
            // pay no parsing cost at all.
            let request_query = session.req_header().uri.query().unwrap_or("");
            let (_, mut query_dict) = parse_query(request_query).map_err(|e| {
                error!("Failed to parse query: {:?}", e);
                pingora::Error::new_str("Failed to parse query")
            })?;
            query_dict.insert(
                "method".to_string(),
                session.req_header().method.to_string(),
            );
            query_dict.insert(
                "path".to_string(),
                session.req_header().uri.path().to_string(),
            );
            query_dict.insert(
                "source".to_string(),
                session
                    .req_header()
                    .headers
                    .get("x-forwarded-for")
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or_default()
                    .to_string(),
            );
            debug!("Parsed query: {:#?}", query_dict);
            // Cache key: access_key + bucket + HTTP method only.
            // Volatile query params (uploadId, X-Amz-Date, etc.) must NOT be
            // included — they differ on every request and would make the cache
            // useless.
            let method_str = session.req_header().method.as_str();
            let cache_key = format!("{}:{}:{}", &access_key, bucket, method_str);
            debug!("Cache key: {}", cache_key);

            // Default 300-second TTL so the cache is always effective.
            // ttl=0 in the bucket config means "use default", not "disable caching".
            // Set ttl to u64::MAX in the bucket config to opt out of expiry.
            let effective_ttl = Duration::from_secs(if ttl == 0 { 300 } else { ttl });

            let bucket_clone = bucket.to_string();
            let callback_clone: PyObject = Python::with_gil(|py| py_cb.clone_ref(py));

            let move_access_key = access_key.clone();
            let req = query_dict.clone();
            let takes_request = self.validator_takes_request.unwrap_or(false);

            ctx.auth_cache
                .get_or_validate(&cache_key, effective_ttl, move || {
                    let tk = move_access_key.clone();
                    let bu = bucket_clone.clone();
                    let cb = Python::with_gil(|py| callback_clone.clone_ref(py));
                    {
                        let req_value = req.clone();
                        async move {
                            validate_request(&tk, &bu, &req_value, cb, takes_request)
                                .await
                                .map_err(|_| pingora::Error::new_str("Validator error"))
                        }
                    }
                })
                .await?
        } else {
            true
        };

        if !is_authorized {
            warn!("Access denied for bucket: {}.  End of request.", bucket);
            // session.respond_error(401).await?;
            write_error_response_with_header(
                session,
                StatusCode::UNAUTHORIZED,
                format!("Access denied for bucket: {}", bucket),
            )
            .await?;
            return Ok(true);
        }

        let bucket_config = bucket_config_init;

        debug!("Access key: {}", &access_key);

        // we have to check for some available credentials here to be able to return unauthorized already if not
        match bucket_config.clone() {
            Some(mut config) => {
                let fetcher_opt = ctx.bucket_creds_fetcher.as_ref().map(|py_cb| {
                    // clone the PyObject so the async block is 'static
                    let cb = Python::with_gil(|py| py_cb.clone_ref(py));
                    move |bucket: String| async move {
                        get_credential_for_bucket(&cb, bucket, access_key)
                            .await
                            .map_err(|e| e.into()) // Convert PyErr -> Box<dyn Error>
                    }
                });

                config
                    .ensure_credentials(&hdr_bucket, fetcher_opt)
                    .await
                    .map_err(|e| {
                        error!("Credential check failed for {hdr_bucket}: {e}");
                        pingora::Error::new_str("Credential check failed")
                    })?;

                ctx.cos_mapping
                    .write()
                    .await
                    .insert(hdr_bucket.clone(), config);
            }
            None => {
                warn!("No configuration for bucket '{hdr_bucket}'; returning 404");
                // Build an S3-style NoSuchBucket error.  HEAD requests must not
                // include a body per HTTP spec, so we only write one for others.
                let mut hdr = ResponseHeader::build(StatusCode::NOT_FOUND, None)?;
                hdr.insert_header("Server", DEFAULT_SERVER_NAME)?;
                if session.req_header().method == Method::HEAD {
                    session.write_response_header(Box::new(hdr), true).await?;
                } else {
                    let xml = format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Error><Code>NoSuchBucket</Code>\
<Message>The specified bucket does not exist</Message>\
<BucketName>{hdr_bucket}</BucketName></Error>"
                    );
                    let xml_bytes = Bytes::copy_from_slice(xml.as_bytes());
                    hdr.insert_header("Content-Type", "application/xml")?;
                    hdr.insert_header("Content-Length", xml_bytes.len().to_string())?;
                    session.write_response_header(Box::new(hdr), false).await?;
                    session.write_response_body(Some(xml_bytes), true).await?;
                }
                return Ok(true);
            }
        }
        debug!(
            "request_filter::Credentials checked for bucket: {}. End of function.",
            hdr_bucket
        );
        debug!("request_filter::end");
        Ok(false)
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut pingora::http::RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        if let Some(presigned) = ctx.is_presigned
            && presigned
        {
            debug!("upstream_request_filter::presigned");
            let cleaned_q = upstream_request
                .uri
                .query()
                .unwrap_or("")
                .split('&')
                .filter(|kv| !kv.starts_with("X-Amz-"))
                .collect::<Vec<_>>()
                .join("&");

            let _ = upstream_request.remove_header("authorization");

            let new_path_and_query = if cleaned_q.is_empty() {
                upstream_request.uri.path().to_owned()
            } else {
                format!("{}?{}", upstream_request.uri.path(), cleaned_q)
            };

            upstream_request.set_uri(
                new_path_and_query
                    .try_into()
                    .map_err(|_| pingora::Error::new_str("invalid URI after query rewrite"))?,
            );
        }

        let _ = upstream_request.remove_header("accept-encoding");

        debug!("upstream_request_filter::start");

        let (_, (bucket, my_updated_url)) = parse_path(upstream_request.uri.path())
            .map_err(|_| pingora::Error::new_str("failed to parse upstream request path"))?;

        debug!(my_updated_url, "parsed upstream path");

        let hdr_bucket = bucket.to_string();

        let my_query = match upstream_request.uri.query() {
            Some(q) if !q.is_empty() => format!("?{}", q),
            _ => String::new(),
        };

        let bucket_config = {
            let map = ctx.cos_mapping.read().await;
            map.get(&hdr_bucket).cloned()
        };

        let addressing_style = bucket_config
            .as_ref()
            .and_then(|c| c.addressing_style.as_deref())
            .unwrap_or("virtual");

        let this_url = match addressing_style {
            "virtual" => my_updated_url,
            _ => {
                // For bucket-root requests, my_updated_url is "/" which would
                // produce "/bucket/" (with trailing slash).  Bucket-level S3
                // operations (ListObjects, ListMultipartUploads, …) must be
                // addressed as "/bucket" — without the trailing slash.
                let u_url = if my_updated_url == "/" {
                    format!("/{}", bucket)
                } else {
                    format!("/{}{}", bucket, my_updated_url)
                };
                debug!(u_url, "using path addressing style");
                &u_url.clone()
            }
        };

        let endpoint = match &bucket_config {
            Some(cfg) => {
                let this_host = match addressing_style {
                    "path" => cfg.host.clone(),
                    _ => format!("{}.{}", bucket, cfg.host),
                };
                if cfg.port == 443 {
                    this_host
                } else {
                    format!("{}:{}", this_host, cfg.port)
                }
            }
            None => format!("{}.{}", bucket, self.cos_endpoint),
        };

        debug!("endpoint: {}.", &endpoint);

        let authority = Authority::try_from(endpoint.as_str())
            .map_err(|_| pingora::Error::new_str("invalid upstream authority"))?;
        // if addressing_style == "virtual" {

        let new_uri = Uri::builder()
            .scheme("https")
            .authority(authority.clone())
            .path_and_query(this_url.to_owned() + &my_query)
            .build()
            .expect("should build a valid URI");

        upstream_request.set_uri(new_uri.clone());
        // }
        upstream_request.insert_header("host", authority.as_str())?;

        let (maybe_hmac, maybe_api_key) = match &bucket_config {
            Some(cfg) => (cfg.has_hmac(), cfg.api_key.clone()),
            None => (false, None),
        };

        let allowed = [
            "host",
            "content-length",
            "content-type",
            "content-md5",
            "x-amz-date",
            "x-amz-content-sha256",
            "x-amz-security-token",
            "transfer-encoding",
            "content-encoding",
            "x-amz-decoded-content-length",
            "x-amz-trailer",
            "x-amz-sdk-checksum-algorithm",
            // CopyObject headers
            "x-amz-copy-source",
            "x-amz-metadata-directive",
            "x-amz-copy-source-if-match",
            "x-amz-copy-source-if-none-match",
            "x-amz-copy-source-if-modified-since",
            "x-amz-copy-source-if-unmodified-since",
            // UploadPartCopy byte-range
            "x-amz-copy-source-range",
            // Conditional GET/PUT (If-Match, If-None-Match, etc.)
            "if-match",
            "if-none-match",
            "if-modified-since",
            "if-unmodified-since",
            // User-visible metadata that Garage stores and returns verbatim
            "cache-control",
            "content-disposition",
            // Inline object tagging on PutObject
            "x-amz-tagging",
            // TaggingDirective on CopyObject (COPY or REPLACE)
            "x-amz-tagging-directive",
            "range",
            "expect",
        ];

        let to_remove: Vec<String> = upstream_request
            .headers
            .iter()
            .filter_map(|(name, _)| {
                let n = name.as_str();
                let keep = allowed.contains(&n)
                    || n.starts_with("x-amz-checksum-")
                    || n.starts_with("x-amz-meta-");
                if keep { None } else { Some(n.to_owned()) }
            })
            .collect();

        for name in to_remove {
            let _ = upstream_request.remove_header(&name);
        }

        if maybe_hmac {
            debug!("HMAC: Signing request for bucket: {}", hdr_bucket);

            let streaming = {
                upstream_request
                    .headers
                    .get("x-amz-content-sha256")
                    .map(|v| v.as_bytes().starts_with(b"STREAMING-"))
                    .unwrap_or(false)
            };

            if streaming {
                let streaming_header = upstream_request
                    .headers
                    .get("x-amz-content-sha256")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default();

                debug!(streaming_header, "streaming upload detected");

                let cfg = bucket_config.as_ref().ok_or_else(|| {
                    pingora::Error::new_str("no bucket config for streaming upload")
                })?;
                let access_key = cfg.access_key.as_deref().unwrap_or_default().to_string();
                let secret_key = cfg.secret_key.as_deref().unwrap_or_default().to_string();
                let region = cfg.region.as_deref().unwrap_or_default().to_string();

                // let decoded_len = upstream_request
                //     .headers
                //     .get("x-amz-decoded-content-length")
                //     .and_then(|v| v.to_str().ok())
                //     .unwrap_or("0")
                //     .to_owned();

                // remove the original streaming headers we cannot forward.
                // upstream_request.remove_header("x-amz-decoded-content-length");

                //  stream-chunk.
                debug!(headers = ?upstream_request.headers, "upstream request headers before streaming rewrite");
                upstream_request.remove_header("content-length");
                upstream_request.remove_header("content-md5");
                upstream_request.insert_header("transfer-encoding", "chunked")?;
                // upstream_request.insert_header("x-amz-decoded-content-length", decoded_len)?;
                upstream_request.set_send_end_stream(false);

                // produce *seed* signature and signing key that will be reused
                //    for every DATA frame in the forthcoming request_body_filter.
                let ts = chrono::Utc::now();
                resign_streaming_request(upstream_request, &region, &access_key, &secret_key, ts)
                    .map_err(|e| {
                    error!("Failed to sign request: {e}");
                    pingora::Error::new_str("Failed to sign request")
                })?;

                let seed_sig = upstream_request
                    .headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.split("Signature=").nth(1))
                    .expect("seed signature missing")
                    .to_owned();

                // stash everything the body filter will need.
                ctx.stream_state = Some(signer::StreamingState::new(
                    region.to_string(),
                    access_key.to_string(),
                    secret_key.to_string(),
                    ts,
                    seed_sig,
                ));
            } else {
                sign_request(
                    upstream_request,
                    bucket_config
                        .as_ref()
                        .ok_or_else(|| pingora::Error::new_str("no bucket config for signing"))?,
                )
                .await
                .map_err(|e| {
                    error!("Failed to sign request for {}: {e}", hdr_bucket);
                    pingora::Error::new_str("Failed to sign request")
                })?;
            }

            debug!("Request signed for bucket: {}", hdr_bucket);
            debug!("{:#?}", &upstream_request.headers);
        } else {
            debug!("Using API key for bucket: {}", hdr_bucket);
            let api_key = match maybe_api_key {
                Some(key) => key,
                None => {
                    // should be impossible because request_filter already
                    // called ensure_credentials, but double‑check anyway
                    error!("No API key for bucket {hdr_bucket}");
                    return Err(pingora::Error::new_str("No API key configured for bucket"));
                }
            };

            // closure captured by SecretsCache
            let bearer_fetcher = {
                let api_key = api_key.clone();
                move || get_bearer(api_key.clone())
            };

            let bearer_token = ctx
                .secrets_cache
                .get(&hdr_bucket, bearer_fetcher)
                .await
                .ok_or_else(|| pingora::Error::new_str("Failed to obtain bearer token"))?;

            upstream_request.insert_header("Authorization", format!("Bearer {bearer_token}"))?;
        }

        // debug!("Sending request to upstream: {}", &new_uri);

        debug!("Request sent to upstream.");
        debug!("upstream_request_filter::end");

        Ok(())
    }

    async fn response_filter(
        &self,
        #[cfg_attr(not(feature = "metrics"), allow(unused_variables))] session: &mut Session,
        resp: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        let _ = resp.remove_header("Server");
        let _ = resp.insert_header("Server", DEFAULT_SERVER_NAME);

        // S3 guarantees objects always have a Content-Type.  When the backend
        // (Garage) omits it (e.g. object stored without an explicit type), we
        // fall back to the S3 default so clients don't see a missing header.
        if resp.headers.get("content-type").is_none()
            && (resp.status == StatusCode::OK || resp.status == StatusCode::PARTIAL_CONTENT)
        {
            let _ = resp.insert_header("Content-Type", "application/octet-stream");
        }

        #[cfg(feature = "metrics")]
        {
            let status = resp.status.as_str();
            let method = session.req_header().method.as_str();
            let bucket = session
                .req_header()
                .uri
                .path()
                .split('/')
                .nth(1)
                .unwrap_or("-");
            utils::metrics::REQUESTS_TOTAL
                .with_label_values(&[method, bucket, status])
                .inc();
            if resp.status.is_client_error() || resp.status.is_server_error() {
                utils::metrics::REQUEST_ERRORS_TOTAL
                    .with_label_values(&[method, bucket, status])
                    .inc();
            }
            if let Some(cl) = resp
                .headers
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<i64>().ok())
            {
                utils::metrics::TRANSFER_BYTES_TOTAL
                    .with_label_values(&["tx", bucket])
                    .inc_by(cl as u64);
                utils::metrics::RESPONSE_SIZE_BYTES
                    .with_label_values(&[method, bucket])
                    .observe(cl as f64);
            }
        }

        Ok(())
    }

    async fn request_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<bytes::Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        // 0. Track inbound bytes regardless of streaming state
        #[cfg(feature = "metrics")]
        if let Some(payload) = body.as_ref()
            && !payload.is_empty()
        {
            let bucket = _session
                .req_header()
                .uri
                .path()
                .split('/')
                .nth(1)
                .unwrap_or("-");
            utils::metrics::TRANSFER_BYTES_TOTAL
                .with_label_values(&["rx", bucket])
                .inc_by(payload.len() as u64);
        }

        // 1. Only active when we stashed a StreamingState in the request filter
        let Some(state) = ctx.stream_state.as_mut() else {
            return Ok(());
        };

        // 1. Flush frames are empty and *not* EOS - just ignore them
        let Some(payload) = body.take() else {
            return Ok(());
        };
        if payload.is_empty() && !end_of_stream {
            return Ok(());
        };

        // 2. Build the outgoing buffer.
        //    The incoming body already has the client's aws-chunked framing:
        //      <hex-size>;chunk-signature=<sig>\r\n<payload>\r\n
        //    We must strip that framing, extract the raw payload bytes, and
        //    then re-sign/re-frame for the Garage backend.
        let mut out = BytesMut::new();
        state.decode_buf.extend_from_slice(&payload);

        while let Some((header_len, payload_len)) =
            signer::parse_aws_chunk_header(&state.decode_buf)
        {
            // total bytes needed: header + payload + trailing \r\n
            let total = header_len + payload_len + 2;
            if state.decode_buf.len() < total {
                // wait for more data
                break;
            }
            let raw_payload = state.decode_buf[header_len..header_len + payload_len].to_vec();
            use bytes::Buf;
            state.decode_buf.advance(total);

            if payload_len == 0 {
                // This is the client's terminal empty chunk — skip it.
                // We will emit our own terminal chunk below when end_of_stream.
                break;
            }
            out.extend_from_slice(&state.sign_chunk(&raw_payload).map_err(|e| {
                error!("Failed to sign chunk: {e}");
                pingora::Error::new_str("Failed to sign chunk")
            })?);
        }

        if end_of_stream {
            out.extend_from_slice(&state.final_chunk().map_err(|e| {
                error!("Failed to sign trailer: {e}");
                pingora::Error::new_str("Failed to sign trailer")
            })?);
            ctx.stream_state = None; // upload finished
        }

        // 3. Hand the encoded bytes to Pingora
        *body = Some(out.freeze());
        Ok(())
    }
}

/// Initialise the global [`tracing`] subscriber.
///
/// Configures a human-readable formatter with RFC 3339 timestamps.  The log
/// level is controlled by the `RUST_LOG` environment variable (e.g.
/// `RUST_LOG=object_storage_proxy=debug`).
///
/// This is called automatically by [`run_server`] and should not normally be
/// invoked by application code.
pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_timer(ChronoLocal::rfc_3339())
        .with_env_filter(EnvFilter::from_default_env())
        .init();
}

/// Build and run the Pingora proxy server.
///
/// This is the Rust entry-point called from [`start_server`].  It:
/// 1. Initialises tracing.
/// 2. Parses the COS map and HMAC keystore from the Python objects in `run_args`.
/// 3. Creates the Pingora [`Server`], attaches HTTP and/or HTTPS listeners, and
///    enters the run-forever loop (blocking the calling thread).
///
/// # Panics
///
/// Panics if `run_args.cos_map` cannot be parsed, or if the TLS certificate /
/// key paths are missing when `https_port` is set.
pub fn run_server(py: Python, run_args: &ProxyServerConfig) {
    print_banner();
    init_tracing();

    #[cfg(feature = "metrics")]
    {
        utils::metrics::init_metrics();
        if let Some(port) = run_args.metrics_port {
            // Spawn the metrics HTTP server on a background Tokio task.
            // `tokio::spawn` requires an active runtime; Pingora sets one up
            // during `my_server.bootstrap()` but we need a runtime here
            // before bootstrap, so we use a standalone one.
            std::thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("metrics runtime")
                    .block_on(utils::metrics::serve_metrics(port));
            });
        }
    }

    if run_args.http_port.is_none() && run_args.https_port.is_none() {
        error!("At least one of http_port or https_port must be specified!");
        return;
    }

    if let Some(http_port) = run_args.http_port {
        info!("starting HTTP server on port {}", http_port);
    }

    if let Some(https_port) = run_args.https_port {
        info!("starting HTTPS server on port {}", https_port);
    }

    let local_hmac_map = if Python::with_gil(|py| run_args.hmac_keystore.is_none(py)) {
        HashMap::new()
    } else {
        parse_hmac_list(py, &run_args.hmac_keystore).unwrap_or_default()
    };

    debug!("HMAC keys: {:#?}", &local_hmac_map);

    let cosmap = Arc::new(RwLock::new(
        parse_cos_map(py, &run_args.cos_map).expect("failed to parse cos_map"),
    ));
    let hmac_keystore = Arc::new(RwLock::new(local_hmac_map));

    let mut my_server = Server::new(None).expect("failed to create pingora server");
    my_server.bootstrap();

    let validator = run_args.validator.as_ref().map(|v| v.clone_ref(py));
    let hmac_fetcher = run_args.hmac_fetcher.as_ref().map(|v| v.clone_ref(py));

    // Inspect the validator callable's arity once at startup.
    let validator_takes_request = run_args.validator.as_ref().map(|v| {
        Python::with_gil(|py| utils::functions::callable_accepts_request(py, v).unwrap_or(false))
    });

    let auth_cache_instance = AuthCache::new();

    let auth_cache_for_sweep = auth_cache_instance.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            auth_cache_for_sweep.sweep();
            debug!("AuthCache sweep complete");
        }
    });

    let mut my_proxy = pingora::proxy::http_proxy_service(
        &my_server.configuration,
        MyProxy {
            cos_endpoint: "s3.eu-de.cloud-object-storage.appdomain.cloud".to_string(),
            cos_mapping: Arc::clone(&cosmap),
            hmac_keystore: Arc::clone(&hmac_keystore),
            secrets_cache: SecretsCache::new(),
            auth_cache: auth_cache_instance,
            validator,
            bucket_creds_fetcher: run_args
                .bucket_creds_fetcher
                .as_ref()
                .map(|v| v.clone_ref(py)),
            verify: run_args.verify,
            skip_signature_validation: run_args.skip_signature_validation,
            hmac_fetcher,
            tracker: UrlTracker::new(),
            max_presign_url_usage_attempts: run_args.max_presign_url_usage_attempts,
            server_name: "<osp⚡>".to_string(),
            validator_takes_request,
        },
    );

    if run_args.threads.is_some() {
        my_proxy.threads = run_args.threads;
    }

    debug!("Proxy service threads: {:?}", &my_proxy.threads);

    if let Some(http_port) = run_args.http_port {
        info!("starting HTTP server on port {}", &http_port);
        let addr = format!("0.0.0.0:{}", http_port);
        my_proxy.add_tcp(addr.as_str());
    }

    if let Some(https_port) = run_args.https_port {
        let cert_path =
            std::env::var("TLS_CERT_PATH").expect("Set TLS_CERT_PATH to the PEM certificate file");
        let key_path =
            std::env::var("TLS_KEY_PATH").expect("Set TLS_KEY_PATH to the PEM private-key file");

        let mut tls = pingora::listeners::tls::TlsSettings::intermediate(&cert_path, &key_path)
            .expect("failed to build TLS settings");

        tls.enable_h2();
        let https_addr = format!("0.0.0.0:{}", https_port);
        my_proxy.add_tls_with_settings(https_addr.as_str(), /*tcp_opts*/ None, tls);
    }

    my_server.add_service(my_proxy);

    debug!("{:?}", &my_server.configuration);

    py.allow_threads(|| my_server.run_forever());

    info!("server running ...");
}

/// Start an HTTP + HTTPS reverse‑proxy for IBM COS.
///
/// Equivalent to running ``pingora`` with a custom handler.
///
/// Parameters
/// ----------
/// run_args:
///    A :py:class:`ProxyServerConfig` object containing the configuration for the server.
///     The configuration includes the following parameters:
///   - cos_map: A dictionary mapping bucket names to their respective COS configuration.
///     Each entry should contain the following
///     keys:
///        - host: The COS endpoint (e.g., "s3.eu-de.cloud-object-storage.appdomain.cloud")
///        - port: The port number (e.g., 443)
///        - api_key/apikey: The API key for the bucket (optional)
///        - ttl/time-to-live: The time-to-live for the API key in seconds (optional)
///   - bucket_creds_fetcher: Optional Python async callable that fetches the API key for a bucket.
///     The callable should accept a single argument, the bucket name.
///     It should return a string containing the API key.
///   - http_port: The HTTP port to listen on.
///   - https_port: The HTTPS port to listen on.
///   - validator: Optional Python async callable that validates the request.
///     The callable should accept two arguments, the access_key and the bucket name.
///     It should return a boolean indicating whether the request is valid.
///   - threads: Optional number of threads to use for the server.
///     If not specified, the server will use a single thread.
#[pyfunction]
pub fn start_server(py: Python, run_args: &ProxyServerConfig) -> PyResult<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    dotenv().ok();

    run_server(py, run_args);

    Ok(())
}

/// Enable the global request counter (disabled by default).
///
/// Once enabled every request proxied increments an atomic counter that can be
/// read with [`get_request_count`].  Useful for testing and load-measurement.
#[pyfunction]
fn enable_request_counting() {
    REQ_COUNTER_ENABLED.store(true, Ordering::Relaxed);
}

/// Disable the global request counter.
#[pyfunction]
fn disable_request_counting() {
    REQ_COUNTER_ENABLED.store(false, Ordering::Relaxed);
}

/// Return the total number of proxied requests since counting was enabled.
#[pyfunction]
fn get_request_count() -> PyResult<usize> {
    Ok(REQ_COUNTER.load(Ordering::Relaxed))
}

#[pymodule]
fn object_storage_proxy(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(start_server, m)?)?;
    m.add_class::<ProxyServerConfig>()?;
    m.add_class::<CosMapItem>()?;
    m.add_function(wrap_pyfunction!(enable_request_counting, m)?)?;
    m.add_function(wrap_pyfunction!(disable_request_counting, m)?)?;
    m.add_function(wrap_pyfunction!(get_request_count, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── UrlTracker ────────────────────────────────────────────────────────────

    #[test]
    fn url_tracker_new_is_empty() {
        let tracker = UrlTracker::new();
        assert!(tracker.get_all().is_empty());
    }

    #[test]
    fn url_tracker_default_equals_new() {
        let t1 = UrlTracker::new();
        let t2 = UrlTracker::default();
        assert_eq!(t1.get_all().len(), t2.get_all().len());
    }

    #[test]
    fn url_tracker_track_increments_count() {
        let tracker = UrlTracker::new();
        assert_eq!(tracker.get("http://example.com/key"), None);
        tracker.track("http://example.com/key");
        assert_eq!(tracker.get("http://example.com/key"), Some(1));
        tracker.track("http://example.com/key");
        assert_eq!(tracker.get("http://example.com/key"), Some(2));
    }

    #[test]
    fn url_tracker_get_returns_none_for_unknown_url() {
        let tracker = UrlTracker::new();
        assert_eq!(tracker.get("http://example.com/missing"), None);
    }

    #[test]
    fn url_tracker_get_all_returns_all_tracked_urls() {
        let tracker = UrlTracker::new();
        tracker.track("http://example.com/a");
        tracker.track("http://example.com/b");
        tracker.track("http://example.com/a");
        let mut all = tracker.get_all();
        all.sort_by_key(|(k, _)| k.clone());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0], ("http://example.com/a".to_string(), 2));
        assert_eq!(all[1], ("http://example.com/b".to_string(), 1));
    }
}
