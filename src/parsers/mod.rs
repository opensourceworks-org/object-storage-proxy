//! Nom-based parsers for paths, query strings, credentials, and the COS map.
//!
//! Sub-modules:
//! * [`cos_map`] — parse the Python `cos_map` dict into [`cos_map::CosMapItem`] structs.
//! * [`credentials`] — parse `Authorization` headers and presigned URL parameters.
//! * [`keystore`] — parse the Python HMAC keystore list.
//! * [`path`] — parse S3-style request paths and query strings.

pub mod cos_map;
pub mod credentials;
pub mod keystore;
pub mod path;
