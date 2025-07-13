use http::StatusCode;
use pingora::http::ResponseHeader;
use pingora::proxy::{ProxyHttp, Session};
use pingora::Result;
use bytes::Bytes;

const DEFAULT_SERVER_NAME: &str = "<osp⚡>";

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
        .write_response_body(
            Some(Bytes::copy_from_slice(msg.as_bytes())),
            false,
        )
        .await?;

    session.respond_error_with_body(status_code.as_u16(), msg.clone().into()).await?;
    println!("{} {} responded with {}", "0".repeat(80), &msg, status_code);
    Ok(true)
}
