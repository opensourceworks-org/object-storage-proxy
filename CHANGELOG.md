# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.3] - 2026-05-13

### Added
- **Prometheus metrics** — built-in `/metrics` scrape endpoint, on by default (disable with `--no-default-features`).
  - New `metrics_port: Option<u16>` field on `ProxyServerConfig`; when set, a lightweight Tokio HTTP listener serves `/metrics` on that port.
  - Metrics: `osp_requests_total`, `osp_request_errors_total`, `osp_transfer_bytes_total`, `osp_presigned_url_hits_total`, `osp_presigned_url_rejected_total`, `osp_active_connections`, `osp_memory_bytes`, `osp_build_info`, `osp_request_duration_seconds`, `osp_response_size_bytes`.
  - Memory gauge reads `/proc/self/status` (Linux); no-op on other platforms.
- `flake.nix` for reproducible Rust + Python development environment via Nix.
- `Taskfile.yml` with tasks for build, run, test, lint, and clean.
- `BUILD.md` with detailed build and run instructions.
- `CONTRIBUTING.md`, `CHANGELOG.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`.
- GitHub issue templates and pull request template.
- `.env.example` documenting all required environment variables.

### Changed
- `prometheus` and `once_cell` are now gated behind the `metrics` Cargo feature (enabled by default).
- Fixed `pyproject.toml` license classifier from Proprietary to MIT.
- Added `license`, `homepage`, `repository`, and `description` to `Cargo.toml`.
- Updated `object_storage_proxy.pyi` stub with all `ProxyServerConfig` parameters.
- Commented out broken `[tool.uv.workspace]` entry (integration/ has no pyproject.toml).

### Fixed
- Unused import compiler warnings resolved via `cargo fix`.
- CI: aarch64 Linux builds now use native `ubuntu-22.04-arm` runner (removes cross-compilation).
- CI: `perl-core` + `OPENSSL_STATIC=1` for fully self-contained wheels on manylinux.
- NixOS stale `.so` workaround: `task build` uses `cargo build` + `cp` instead of `maturin develop`.

## [0.4.3] - 2025-04-19

### Added
- Configurable `max_presign_url_usage_attempts` for presigned URL access control.
- `server_name` field on `ProxyServerConfig`.
- `hmac_fetcher` callable for dynamic secret key lookup by access key.

### Changed
- Migrated from `pingora 0.4` (OpenSSL) to `pingora 0.5` (rustls).
- Switched from `openssl` to `rustls` + `aws-lc-rs` throughout.

## [0.4.0] - 2025-03-01

### Added
- Configurable request counting (`enable_request_counting`, `disable_request_counting`, `get_request_count`).
- `skip_signature_validation` option for development use.
- `verify` option to disable upstream TLS certificate verification.
- `hmac_keystore` support for multi-credential HMAC key management.

### Changed
- `ProxyServerConfig` now accepts `hmac_keystore` as a list of access/secret key dicts.

## [0.3.0] - 2025-01-15

### Added
- Python callable for authorization (`validator`).
- TTL-based authorization cache.
- HTTP/2 support on the HTTPS frontend.

## [0.2.0] - 2024-11-01

### Added
- Python callable for credential fetching (`bucket_creds_fetcher`).
- IBM COS IAM bearer token cache with configurable TTL.
- Path-style to virtual-style address translation.

## [0.1.0] - 2024-09-01

### Added
- Initial release.
- Pingora-based reverse proxy for AWS S3 and IBM Cloud Object Storage.
- AWS SigV4 request re-signing.
- `ProxyServerConfig` Python class with `cos_map`, `http_port`, `https_port`, `threads`.
