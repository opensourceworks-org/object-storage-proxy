//! Utility helpers shared across the proxy.
//!
//! Sub-modules:
//! * [`banner`] — startup banner.
//! * [`functions`] — Python introspection helpers.
//! * [`response`] — helpers for writing HTTP error responses.
//! * [`validator`] — authorization cache and Python validator bridge.

pub mod banner;
pub mod functions;
#[cfg(feature = "metrics")]
pub mod metrics;
pub mod response;
pub mod validator;
