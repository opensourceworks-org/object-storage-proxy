# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `flake.nix` for reproducible Rust + Python development environment via Nix.
- `Taskfile.yml` with tasks for build, run, test, lint, and clean.
- `BUILD.md` with detailed build and run instructions.
- `CONTRIBUTING.md`, `CHANGELOG.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`.
- GitHub issue templates and pull request template.
- `.env.example` documenting all required environment variables.

### Changed
- Fixed `pyproject.toml` license classifier from Proprietary to MIT.
- Added `license`, `homepage`, `repository`, and `description` to `Cargo.toml`.
- Updated `object_storage_proxy.pyi` stub with all `ProxyServerConfig` parameters.
- Commented out broken `[tool.uv.workspace]` entry (integration/ has no pyproject.toml).

### Fixed
- Unused import compiler warnings resolved via `cargo fix`.

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
