#![warn(clippy::all)]
use async_trait::async_trait;
use dotenv::dotenv;
use http::Uri;
use http::uri::Authority;
use parsers::cos_map::{CosMapItem, parse_cos_map};
use pingora::proxy::{ProxyHttp, Session};
use pingora::Result;
use pingora::server::Server;
use pingora::upstreams::peer::HttpPeer;
use pyo3::{Bound, PyResult, Python, pyclass, pyfunction, pymodule, wrap_pyfunction};
use pyo3::prelude::*;
use pyo3::types::{PyModule, PyModuleMethods};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Mutex;
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing::{error, info};

pub mod parsers;
use parsers::path::parse_path;
use parsers::credentials::parse_token_from_header;

pub mod credentials;
use credentials::secrets_proxy::{SecretsCache, get_api_key_for_bucket, get_bearer};

pub mod utils;
use utils::validator::{AuthCache, validate_request};

static REQ_COUNTER: Mutex<usize> = Mutex::new(0);

#[pyclass]
#[pyo3(name = "ProxyServerConfig")]
#[derive(Debug)]
pub struct ProxyServerConfig {
    #[pyo3(get, set)]
    pub bucket_creds_fetcher: Option<Py<PyAny>>,

    #[pyo3(get, set)]
    pub cos_map: PyObject,

    #[pyo3(get, set)]
    pub http_port: u16,

    #[pyo3(get, set)]
    pub https_port: u16,

    #[pyo3(get, set)]
    pub validator: Option<Py<PyAny>>,
}

impl Default for ProxyServerConfig {
    fn default() -> Self {
        ProxyServerConfig {
            cos_map: Python::with_gil(|py| py.None()),
            bucket_creds_fetcher: None,
            http_port: 6190,
            https_port: 443,
            validator: None,
        }
    }
}

#[pymethods]
impl ProxyServerConfig {
    #[new]
    #[pyo3(
        signature = (
            cos_map,
            bucket_creds_fetcher = None,
            http_port = 6190,
            https_port = 443,
            validator = None
        )
    )]
    pub fn new(
        cos_map: PyObject,
        bucket_creds_fetcher: Option<PyObject>,
        http_port: u16,
        https_port: u16,
        validator: Option<PyObject>,
    ) -> Self {
        ProxyServerConfig {
            cos_map,
            bucket_creds_fetcher,
            http_port,
            https_port,
            validator,
        }
    }
}

pub struct MyProxy {
    cos_endpoint: String,
    cos_mapping: HashMap<String, CosMapItem>,
    secrets_cache: SecretsCache,
    auth_cache: AuthCache,
    validator: Option<PyObject>,
    bucket_creds_fetcher: Option<PyObject>,
}

pub struct MyCtx {
    cos_mapping: HashMap<String, CosMapItem>,
    secrets_cache: SecretsCache,
    auth_cache: AuthCache,
    validator: Option<PyObject>,
    bucket_creds_fetcher: Option<PyObject>,
}

#[async_trait]
impl ProxyHttp for MyProxy {
    type CTX = MyCtx;
    fn new_ctx(&self) -> Self::CTX {
        MyCtx {
            cos_mapping: self.cos_mapping.clone(),
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
        }
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        let path = session.req_header().uri.path();

        let parse_path_result = parse_path(path);
        if parse_path_result.is_err() {
            error!("Failed to parse path: {:?}", parse_path_result);
            return Err(pingora::Error::new_str("Failed to parse path"));
        }

        let (_, (bucket, _)) = parse_path(path).unwrap();

        let hdr_bucket = bucket.to_owned();

        let auth_header = session
            .req_header()
            .headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .map(ToString::to_string)
            .unwrap_or_default();




        let ttl = ctx
            .cos_mapping
            .get(bucket)
            .and_then(|config| config.ttl)
            .unwrap_or(0);

        let is_authorized = if let Some(py_cb) = &ctx.validator {
            let token = parse_token_from_header(&auth_header)
                .map_err(|_| pingora::Error::new_str("Failed to parse token"))?
                .1
                .to_string();
            let cache_key = format!("{}:{}", token, bucket);

            let bucket_clone = bucket.to_string();
            let callback_clone: PyObject = Python::with_gil(|py| py_cb.clone_ref(py));

            ctx.auth_cache
                .get_or_validate(
                    &cache_key,
                    Duration::from_secs(ttl), // keep this short enough , TODO: pass from python
                    move || {
                        let tk = token.clone();
                        let bu = bucket_clone.clone();
                        let cb = Python::with_gil(|py| callback_clone.clone_ref(py));
                        async move {
                            validate_request(&tk, &bu, cb)
                                .await
                                .map_err(|_| pingora::Error::new_str("Validator error"))
                        }
                    },
                )
                .await?
        } else {
            true
        };

        if !is_authorized {
            info!("Access denied for bucket: {}.  End of request.", bucket);
            session.respond_error(401).await?;
            return Ok(true);
        }

        let bucket_config = ctx.cos_mapping.get(&hdr_bucket);

        let api_key = bucket_config.and_then(|config| config.api_key.clone());
        let _api_key = if let Some(key) = api_key {
            info!("Using API key from config for bucket: {}", hdr_bucket);
            key
        } else if let Some(py_cb) = &ctx.bucket_creds_fetcher {
            info!(
                "No key provided in config. Fetching API key for bucket: {}",
                hdr_bucket
            );
            match get_api_key_for_bucket(py_cb, hdr_bucket.clone()).await {
                Ok(k) => k,
                Err(err) => {
                    error!(
                        "Error fetching API key for bucket {}: {:?}",
                        hdr_bucket, err
                    );
                    return Err(pingora::Error::new_str(
                        "Failed to fetch API key for bucket",
                    ));
                }
            }
        } else {
            error!("No API key available for bucket: {}", hdr_bucket);
            return Err(pingora::Error::new_str("No API key configured for bucket"));
        };


        Ok(false)
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let mut req_counter = REQ_COUNTER.lock().unwrap();
        *req_counter += 1;
        info!("Request count: {}", *req_counter);

        let path = session.req_header().uri.path();

        let parse_path_result = parse_path(path);
        if parse_path_result.is_err() {
            error!("Failed to parse path: {:?}", parse_path_result);
            return Err(pingora::Error::new_str("Failed to parse path"));
        }

        let (_, (bucket, _)) = parse_path(path).unwrap();

        let hdr_bucket = bucket.to_owned();

        let bucket_config = ctx.cos_mapping.get(&hdr_bucket);
        let endpoint = match bucket_config {
            Some(config) => {
                config.host.to_owned()
            }
            None => {
                format!("{}.{}", bucket, self.cos_endpoint)
            }
        };

        let port = bucket_config
            .and_then(|config| Some(config.port))
            .unwrap_or(443);

        let addr = (endpoint.clone(), port);

        let mut peer = Box::new(HttpPeer::new(addr, true, endpoint.clone()));
        peer.options.verify_cert = false;
        Ok(peer)
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut pingora::http::RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        let (_, (bucket, my_updated_url)) = parse_path(upstream_request.uri.path()).unwrap();

        let hdr_bucket = bucket.to_string();

        let my_query = match upstream_request.uri.query() {
            Some(q) if !q.is_empty() => format!("?{}", q),
            _ => String::new(),
        };

        let bucket_config = ctx.cos_mapping.get(&hdr_bucket);

        let endpoint = match bucket_config {
            Some(config) => {
                format!("{}.{}:{}", bucket, config.host, config.port)
            }
            None => {
                format!("{}.{}", bucket, self.cos_endpoint)
            }
        };

        // todo: we already know we have an api key here (request_filter), we just need to use it
        let api_key = bucket_config.and_then(|config| config.api_key.clone());
        let api_key = if let Some(key) = api_key {
            info!("Using API key from config for bucket: {}", hdr_bucket);
            key
        } else if let Some(py_cb) = &ctx.bucket_creds_fetcher {
            info!(
                "No key provided in config. Fetching API key for bucket: {}",
                hdr_bucket
            );
            match get_api_key_for_bucket(py_cb, hdr_bucket.clone()).await {
                Ok(k) => k,
                Err(err) => {
                    error!(
                        "Error fetching API key for bucket {}: {:?}",
                        hdr_bucket, err
                    );
                    return Err(pingora::Error::new_str(
                        "Failed to fetch API key for bucket",
                    ));
                }
            }
        } else {
            error!("No API key available for bucket: {}", hdr_bucket);
            return Err(pingora::Error::new_str("No API key configured for bucket"));
        };

        // a partial, a closure with the api_key already bound to the get_bearer function
        let bearer_fetcher = {
            let api_key = api_key.clone();
            move || get_bearer(api_key.clone())
        };

        let bearer_token = ctx.secrets_cache.get(&hdr_bucket, bearer_fetcher).await;

        // Box:leak the temporary string to get a static reference which will outlive the function
        let authority = Authority::from_static(Box::leak(endpoint.clone().into_boxed_str()));

        let new_uri = Uri::builder()
            .scheme("https")
            .authority(authority.clone())
            .path_and_query(my_updated_url.to_owned() + &my_query)
            .build()
            .expect("should build a valid URI");

        info!("Sending request to upstream: {}", &new_uri);

        upstream_request.set_uri(new_uri);

        upstream_request.insert_header("host", authority.as_str())?;

        upstream_request
            .insert_header("Authorization", format!("Bearer {}", bearer_token.unwrap()))?;

        info!("Request sent to upstream.");

        Ok(())
    }
}

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_timer(ChronoLocal::rfc_3339())
        .with_env_filter(EnvFilter::from_default_env())
        .init();
}

pub fn run_server(py: Python, run_args: &ProxyServerConfig) {
    init_tracing();
    info!(
        "Logger initialized; starting server on http port {} and https port {}",
        run_args.http_port, run_args.https_port
    );

    let cosmap = parse_cos_map(py, &run_args.cos_map).unwrap();

    let mut my_server = Server::new(None).unwrap();
    my_server.bootstrap();

    let validator = run_args.validator.as_ref().map(|v| v.clone_ref(py));

    let mut my_proxy = pingora::proxy::http_proxy_service(
        &my_server.configuration,
        MyProxy {
            cos_endpoint: "s3.eu-de.cloud-object-storage.appdomain.cloud".to_string(),
            cos_mapping: cosmap,
            secrets_cache: SecretsCache::new(),
            auth_cache: AuthCache::new(),
            validator,
            bucket_creds_fetcher: run_args
                .bucket_creds_fetcher
                .as_ref()
                .map(|v| v.clone_ref(py)),
        },
    );

    let addr = format!("0.0.0.0:{}", run_args.http_port);
    my_proxy.add_tcp(addr.as_str());

    let cert_path = std::env::var("TLS_CERT_PATH")
        .expect("Set TLS_CERT_PATH to the PEM certificate file");
    let key_path  = std::env::var("TLS_KEY_PATH")
        .expect("Set TLS_KEY_PATH to the PEM private‑key file");

    let mut tls = pingora::listeners::tls::TlsSettings::intermediate(&cert_path, &key_path)
        .expect("failed to build TLS settings");

    tls.enable_h2();
    let https_addr = format!("0.0.0.0:{}", run_args.https_port);
    my_proxy
        .add_tls_with_settings(https_addr.as_str(), /*tcp_opts*/ None, tls);
    my_server.add_service(my_proxy);

    py.allow_threads(|| my_server.run_forever());

    info!("server running ...");
}

#[pyfunction]
pub fn start_server(py: Python, run_args: &ProxyServerConfig) -> PyResult<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    dotenv().ok();

    run_server(py, run_args);

    Ok(())
}

#[pymodule]
fn object_storage_proxy(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(start_server, m)?)?;
    m.add_class::<ProxyServerConfig>()?;
    m.add_class::<CosMapItem>()?;
    Ok(())
}
