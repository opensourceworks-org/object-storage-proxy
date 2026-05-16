[![CI](https://github.com/opensourceworks-org/object-storage-proxy/actions/workflows/ci.yml/badge.svg)](https://github.com/opensourceworks-org/object-storage-proxy/actions/workflows/ci.yml)
[![PyPI version](https://img.shields.io/pypi/v/object-storage-proxy)](https://pypi.org/project/object-storage-proxy/)
[![PyPI downloads](https://img.shields.io/pypi/dm/object-storage-proxy)](https://pypi.org/project/object-storage-proxy/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust edition](https://img.shields.io/badge/Rust-2024-orange.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/)

# <osp⚡> object-storage-proxy

A fast, in-process reverse proxy for AWS S3 and IBM Cloud Object Storage, built on Cloudflare's [pingora](https://github.com/cloudflare/pingora). It exposes a Python interface so you can plug in your own credential fetching, request signing, and authorization logic without touching the Rust core.

> **Note:** This project is under active development. APIs are likely to change before 1.0.

- [High-Level Documentation](https://osp.flexworks.eu)
- [Detailed Documentation](https://osp-docs.flexworks.eu/)
- [Code Documentation (cargo doc)](https://osp-docs.flexworks.eu/api/object_storage_proxy/)
- [Benchmarks](BENCHMARKS.md)
- [Changelog](CHANGELOG.md)
- [Contributing](CONTRIBUTING.md)

## Why

Object storage backends like IBM COS assign one endpoint and one set of credentials per storage instance, which may contain many buckets. Managing credentials and endpoints across instances becomes cumbersome, especially when clients expect a single uniform endpoint.

This proxy solves that by:

1. Translating path-style requests (`http://proxy/bucket/key`) to virtual-hosted-style (`https://bucket.s3.region.host/key`) on the way out.
2. Re-signing requests with the correct backend credentials, so clients only need one keypair pointed at the proxy.
3. Calling your Python functions for credential lookup and request authorization, with TTL-based caching.

![Request lifecycle](https://raw.githubusercontent.com/opensourceworks-org/object-storage-proxy/62adceaddefa2ad911d80fb13a3f9cec2eff8829/img/request_lifecycle.svg)

![Request stages](https://raw.githubusercontent.com/opensourceworks-org/object-storage-proxy/d8ca9ee95f820c9525fef0b703ad28a8bcceedb7/img/request_stages.svg)

## Features

- Compatible with any AWS S3-compatible client: aws-cli, boto3, polars, spark, datafusion, presto, trino, ...
- Normalises differences between S3-compatible backends so clients work regardless of whether the backend is AWS S3, MinIO, Garage, or IBM COS (see [Backend compatibility](#backend-compatibility) below).
- Decouples frontend authentication (what the client sends) from backend authentication (what the storage expects).
- Python callables for credential fetching, HMAC key lookup, and per-request authorization.
- TTL-based credential and authorization caching.
- HTTP and HTTPS frontends (HTTPS supports HTTP/2).
- Configurable thread count and per-URL request counting.
- Presigned URL support with configurable max-usage limiting.
- Built-in Prometheus metrics endpoint (`/metrics`) — on by default, opt-out via `--no-default-features`.

## Installation

```bash
pip install object-storage-proxy
```

Or install from source (requires Rust stable and [uv](https://docs.astral.sh/uv/)):

```bash
git clone https://github.com/opensourceworks-org/object-storage-proxy.git
cd object-storage-proxy
uv run maturin develop --release
```

See [DEVELOP.md](DEVELOP.md) for full develop/build instructions including Nix and Taskfile usage.

## Quick start

### 1. Configure your AWS client

`~/.aws/config`:

```ini
[profile osp]
region = eu-west-3
output = json
services = osp-services
s3 =
    addressing_style = path

[services osp-services]
s3 =
  endpoint_url = http://localhost:6190
```

`~/.aws/credentials`:

```ini
[osp]
aws_access_key_id = MYCLIENTID
aws_secret_access_key = myclientsecret
```

The `aws_access_key_id` is passed as the `token` argument to your Python callables. It can be any identifier meaningful to your auth system: an internal client ID, an OAuth2 subject, etc.

### 2. Write your server script

```python
import json
import os
from object_storage_proxy import ProxyServerConfig, start_server

def fetch_credentials(token: str, bucket: str) -> str:
    # Return either an IBM COS API key string, or a JSON string:
    # '{"access_key": "...", "secret_key": "..."}'
    return json.dumps({
        "access_key": os.environ["BACKEND_ACCESS_KEY"],
        "secret_key": os.environ["BACKEND_SECRET_KEY"],
    })

def lookup_secret(access_key: str) -> str | None:
    # Called to verify incoming HMAC signatures.
    return os.getenv("MYCLIENTSECRET") if access_key == "MYCLIENTID" else None

def authorize(token: str, bucket: str, request: dict) -> bool:
    # Return True to allow, False to deny.
    return True

cos_map = {
    "my-bucket": {
        "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
        "region": "eu-de",
        "port": 443,
        "ttl": 300,
    },
}

config = ProxyServerConfig(
    cos_map=cos_map,
    bucket_creds_fetcher=fetch_credentials,
    hmac_fetcher=lookup_secret,
    validator=authorize,
    http_port=6190,
)

start_server(config)
```

### 3. Run it

```bash
uv run python my_server.py
```

### 4. Use it

```bash
aws s3 ls s3://my-bucket/ --profile osp
aws s3 cp file.txt s3://my-bucket/file.txt --profile osp
```

A fuller example with HTTPS, HMAC keystores, and IBM COS is in [examples/minimal_server.py](examples/minimal_server.py).

## Configuration reference

### ProxyServerConfig

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `cos_map` | `dict` | yes | | Bucket-to-backend mapping. See below. |
| `hmac_keystore` | `list[dict]` | no | `[]` | Static HMAC keypairs accepted on the frontend. |
| `bucket_creds_fetcher` | `callable(token, bucket) -> str` | no | | Called once per bucket to fetch backend credentials. Return an IBM COS API key string or `{"access_key":...,"secret_key":...}` JSON. |
| `hmac_fetcher` | `callable(access_key) -> str \| None` | no | | Called per request to resolve a secret key from an access key, used to verify incoming signatures. |
| `validator` | `callable(token, bucket[, request]) -> bool` | no | | Called per request to authorize access. Cached by (token, bucket) for the bucket TTL. |
| `http_port` | `int` | one of http/https required | | HTTP listener port. |
| `https_port` | `int` | one of http/https required | | HTTPS listener port (HTTP/2 supported). |
| `threads` | `int` | no | `1` | Number of worker threads. |
| `verify` | `bool` | no | `None` | Disable TLS verification on upstream connections. Development only. |
| `skip_signature_validation` | `bool` | no | `False` | Skip verification of incoming request signatures. Development only. |
| `max_presign_url_usage_attempts` | `int` | no | `3` | Max times a presigned URL may be used before being rejected. |
| `server_name` | `str` | no | `"osp"` | Server name included in log output. |
| `metrics_port` | `int` | no | `None` | Port to expose the Prometheus `/metrics` scrape endpoint. When `None` no endpoint is started. |

### cos_map entries

Each key is the bucket name as the client addresses it. The value is a dict:

| Field | Required | Description |
|-------|----------|-------------|
| `host` | yes | Backend hostname |
| `port` | yes | Backend port (typically `443`) |
| `region` | no | AWS/COS region string |
| `apikey` | no | IBM COS IAM API key (mutually exclusive with `access_key`/`secret_key`) |
| `access_key` | no | Backend HMAC access key |
| `secret_key` | no | Backend HMAC secret key |
| `ttl` | no | Credential and auth cache TTL in seconds. Default `300`. Set to `0` to disable. |
| `addressing_style` | no | `"path"` or `"virtual"` (default `"virtual"`) |
| `is_tls_enabled` | no | Defaults to `true` when port is 443 |

### Python callable signatures

```python
# Fetch backend credentials for a bucket.
# token: the access key from the client's Authorization header.
# Return an IBM COS API key string, or JSON: '{"access_key":"...","secret_key":"..."}'
def fetch_credentials(token: str, bucket: str) -> str: ...

# Resolve the secret key for an access key (used to verify incoming signatures).
def lookup_secret(access_key: str) -> str | None: ...

# Authorize a request. request dict contains: method, path, query, headers.
def authorize(token: str, bucket: str, request: dict | None = None) -> bool: ...
```

## Backend compatibility

S3-compatible backends differ in how strictly they follow the AWS S3 specification. OSP irons out these differences so clients don't need to care which backend is underneath.

| Behaviour | AWS S3 spec | Garage | MinIO | OSP handling |
|-----------|-------------|--------|-------|-------------|
| `Content-MD5` on `DeleteObjects` | **Required** | Accepted without it (lenient) | Enforced (`400` if missing) | Forwarded when present; test suite injects it because botocore ≥ 1.43 no longer sends it by default |
| `x-amz-tagging-directive` on `CopyObject` | `COPY` or `REPLACE` | N/A (tagging not implemented) | ✅ enforced | Header is in OSP's forwarding allowlist — was previously stripped |
| `PutObjectTagging` / `GetObjectTagging` | Supported | `NotImplemented` | ✅ | Forwarded; backend limitation is transparent |
| `If-Match` / `If-Unmodified-Since` on `GET` | Must return `412` | Returns `200` (header ignored) | ✅ Returns `412` | Forwarded; backend limitation is transparent |
| `ListMultipartUploads` with `Prefix` ending in `/` | Returns matching uploads | ✅ works | Returns empty list (MinIO bug) | Forwarded; MinIO limitation documented as `xfail` in the test suite |

> **botocore ≥ 1.43 note:** Recent versions of boto3 switched from `Content-MD5` to `x-amz-checksum-crc32` for body integrity on `DeleteObjects`, regardless of the `request_checksum_calculation` setting. `Content-MD5` is still required by MinIO. If you use boto3 ≥ 1.43 directly against MinIO through OSP you may need to inject `Content-MD5` manually via a `before-sign` event hook — see [DEVELOP.md](DEVELOP.md#botocore-43-and-content-md5) for details and example code.

The integration test suite covers all of the above: every test runs parametrized over both Garage and MinIO backends, so regressions surface immediately. See [DEVELOP.md](DEVELOP.md#s3-api-compliance-differences) for the full compliance table and the internal proxy fixes that enable it.

## Prometheus metrics

The proxy ships with a built-in Prometheus scrape endpoint. Set `metrics_port` to enable it:

```python
config = ProxyServerConfig(
    cos_map=cos_map,
    http_port=6190,
    metrics_port=9090,   # exposes http://localhost:9090/metrics
)
```

Then scrape it:

```bash
curl http://localhost:9090/metrics
```

Or add a Prometheus scrape config:

```yaml
scrape_configs:
  - job_name: object-storage-proxy
    static_configs:
      - targets: ["localhost:9090"]
```

Exposed metrics (all prefixed `osp_`):

| Metric | Type | Labels | Description |
|---|---|---|---|
| `osp_requests_total` | Counter | `method`, `bucket`, `status` | Total proxied requests |
| `osp_request_errors_total` | Counter | `method`, `bucket`, `error` | 4xx / 5xx responses |
| `osp_transfer_bytes_total` | Counter | `direction` (`rx`/`tx`), `bucket` | Bytes transferred |
| `osp_presigned_url_hits_total` | Counter | `bucket` | Presigned URL uses |
| `osp_presigned_url_rejected_total` | Counter | `bucket` | Presigned URLs rejected (over limit) |
| `osp_active_connections` | Gauge | — | In-flight connections |
| `osp_memory_bytes` | Gauge | — | Resident set size (Linux only) |
| `osp_build_info` | Gauge | `version`, `rustc` | Static build metadata |
| `osp_request_duration_seconds` | Histogram | `method`, `bucket` | End-to-end request latency |
| `osp_response_size_bytes` | Histogram | `method`, `bucket` | Response body size |

To build without the metrics endpoint:

```bash
maturin develop --no-default-features
```

## HTTPS setup

Generate a self-signed certificate for local development:

```bash
openssl req -x509 -nodes -days 365 \
  -newkey rsa:4096 \
  -keyout key.pem \
  -out cert.pem \
  -config localhost.cnf

export TLS_CERT_PATH=/path/to/cert.pem
export TLS_KEY_PATH=/path/to/key.pem
```

Then pass `https_port=8443` to `ProxyServerConfig`.

## Environment variables

See [.env.example](.env.example) for the full list. Key variables:

| Variable | Description |
|----------|-------------|
| `COS_API_KEY` | IBM COS IAM API key |
| `AWS_ACCESS_KEY` / `AWS_SECRET_KEY` | AWS backend credentials |
| `TLS_CERT_PATH` / `TLS_KEY_PATH` | Paths to TLS certificate and key |
| `OSP_ENABLE_REQUEST_COUNTING` | Set to `true` to enable per-URL request counting |
| `AWS_REQUEST_CHECKSUM_CALCULATION` | Set to `WHEN_REQUIRED` to avoid checksum errors with AWS CLI v2 |

## Build targets

Pre-built wheels are published to [PyPI](https://pypi.org/project/object-storage-proxy/) for the following platforms:

| Platform | Architecture | Libc | Python |
|----------|-------------|------|--------|
| Linux (`ubuntu-22.04`) | x86_64 | glibc (manylinux) | 3.x |
| Linux (`ubuntu-22.04`) | aarch64 | glibc (manylinux) | 3.x |
| Linux (`alpine 3.18`) | x86_64 | musl (musllinux_1_2) | 3.x |
| macOS (`macos-14`) | aarch64 (Apple Silicon) | — | 3.x |
| Source distribution | any | any | 3.x |

Windows builds are not currently active in CI. An sdist is always published so you can build from source on any platform with Rust stable installed.

## Building from source

See [BUILD.md](BUILD.md).

## Roadmap

These backlog items are currently not yet implemented:

- [ ] Pass path and method to Python callbacks; cache by (token, bucket, path, method)
- [ ] Expose pingora server and service configuration directly to Python
- [x] Spark streaming write support
- [ ] AWS CLI checksum workaround ([aws/aws-cli#9214](https://github.com/aws/aws-cli/issues/9214))
- [ ] Allow same bucket name on different providers
- [ ] Pluggable distributed cache

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Bug reports and feature requests go through [GitHub Issues](https://github.com/opensourceworks-org/object-storage-proxy/issues).

## License

[MIT](LICENSE)
