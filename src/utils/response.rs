use bytes::Bytes;
use http::StatusCode;
use pingora::Result;
use pingora::http::ResponseHeader;
use pingora::proxy::Session;
use tracing::debug;

const DEFAULT_SERVER_NAME: &str = "<osp⚡>";

/// Write a plain-text HTTP error response to the downstream client.
///
/// Sets `Content-Type: text/plain`, `X-Content-Type-Options: nosniff`, and a
/// custom `Server` header before sending `msg` as the response body.
///
/// Always returns `Ok(true)` so callers can do
/// `return write_error_response_with_header(…).await?;`
/// from within a `request_filter` implementation.
pub async fn write_error_response_with_header(
    session: &mut Session,
    status_code: StatusCode,
    msg: String,
) -> Result<bool> {
    let mut hdr = ResponseHeader::build(status_code, None)?;
    hdr.insert_header("Content-Type", "text/plain")?;
    hdr.insert_header("Server", DEFAULT_SERVER_NAME)?;
    hdr.insert_header("X-Content-Type-Options", "nosniff")?;

    session.write_response_header(Box::new(hdr), false).await?;

    session
        .write_response_body(Some(Bytes::copy_from_slice(msg.as_bytes())), false)
        .await?;

    session
        .respond_error_with_body(status_code.as_u16(), msg.clone().into())
        .await?;
    debug!(status = status_code.as_u16(), msg, "wrote error response");
    Ok(true)
}
