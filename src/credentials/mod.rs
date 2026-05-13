//! Credential types, signing, and secret management.
//!
//! Sub-modules:
//! * [`hmac_keystore`] — in-memory HMAC key store exposed to Python.
//! * [`models`] — [`models::BucketCredential`] enum parsed from raw credential strings.
//! * [`secrets_proxy`] — IBM IAM bearer-token exchange and caching.
//! * [`signer`] — AWS Signature Version 4 request signing and verification.

pub mod hmac_keystore;
pub mod models;
pub mod secrets_proxy;
pub mod signer;
