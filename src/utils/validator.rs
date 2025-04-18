use pyo3::{PyObject, Python};
use tracing::{error, info};

use crate::parsers::credentials::parse_token_from_header;

pub fn validate_request(
    header: &str,
    bucket: &str,
    py: Python,
    callback: &PyObject,
) -> Result<bool, String> {
    if header.is_empty() {
        return Err("Header is empty".to_string());
    }

    if !header.starts_with("AWS4-HMAC-SHA256 Credential=") {
        return Err("Invalid header format".to_string());
    }

    let token = parse_token_from_header(header).map_err(|_| "Failed to parse token")?;
    let (_, token) = token;

    match callback.call1(py, (token, bucket)) {
        Ok(result) => {
            let is_authorized = result
                .extract::<bool>(py)
                .map_err(|_| "Failed to extract boolean from Python callback")?;
            info!("Callback returned: {:?}", is_authorized);
            return Ok(is_authorized);
        }
        Err(err) => {
            error!("Python callback raised an exception: {:?}", err);

            // Option 1: Return the error to Python (so Python sees the exception)
            // return Err(err);

            // Option 2: Convert it into a custom Python exception or a new error message
            return Err("Failed to call callback due to an inner Python exception".to_string());
        }
    }

}
