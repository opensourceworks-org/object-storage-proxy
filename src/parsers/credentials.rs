use std::collections::HashMap;

use nom::{
    IResult, Parser,
    bytes::complete::{tag, take_until, take_while1},
    multi::separated_list1,
    sequence::{preceded, separated_pair},
};

use nom::character::complete::char as nomchar;
use nom::error::{Error, ErrorKind, make_error};
use percent_encoding::percent_decode_str;

fn miss(i: &str) -> nom::Err<Error<&str>> {
    nom::Err::Error(make_error(i, ErrorKind::Tag))
}

/// Extract the AWS access key ID from an `Authorization` header value.
///
/// Expects the standard `AWS4-HMAC-SHA256 Credential=<ACCESS_KEY>/…` format.
/// Returns the access key as a borrowed slice of the input.
///
/// # Errors
///
/// Returns a nom parse error if the header does not start with the expected prefix.
pub fn parse_token_from_header(header: &str) -> IResult<&str, &str> {
    let (_, token) =
        (preceded(tag("AWS4-HMAC-SHA256 Credential="), take_until("/"))).parse(header)?;

    Ok(("", token))
}

/// Extract the `(region, service)` pair from a `Credential=…` scope string.
///
/// The credential scope has the form:
/// `<ACCESS_KEY>/<DATE>/<REGION>/<SERVICE>/aws4_request`
///
/// Works whether the input starts directly at `Credential=` or has other text
/// before it (e.g. a full `Authorization:` header value).
///
/// # Errors
///
/// Returns a nom parse error if `Credential=` is absent or the scope does not
/// contain the mandatory `/aws4_request` trailer.
pub fn parse_credential_scope(input: &str) -> IResult<&str, (&str, &str)> {
    let (input, _) = take_until("Credential=")(input)?;
    let (remaining, (_, _, _, _, _, region, _, service, _)) = (
        tag("Credential="), // prefix
        take_until("/"),    // access key
        tag("/"),
        take_until("/"), // date
        tag("/"),
        take_until("/"), // region
        tag("/"),
        take_until("/aws4_request"), // service
        tag("/aws4_request"),        // trailing
    )
        .parse(input)?;
    Ok((remaining, (region, service)))
}

/// Structured representation of the query parameters in an AWS presigned URL.
///
/// Produced by [`parse_presigned_params`].
#[derive(Debug, PartialEq)]
pub struct PresignedParams {
    /// Signing algorithm, e.g. `"AWS4-HMAC-SHA256"`.
    pub algorithm: String,
    /// AWS access key ID extracted from `X-Amz-Credential`.
    pub access_key: String,
    /// Short date (`YYYYMMDD`) extracted from `X-Amz-Credential`.
    pub credential_date: String,
    /// AWS region extracted from `X-Amz-Credential`.
    pub region: String,
    /// AWS service code extracted from `X-Amz-Credential`.
    pub service: String,
    /// Full ISO 8601 timestamp from `X-Amz-Date`.
    pub amz_date: String,
    /// Validity window in seconds from `X-Amz-Expires`.
    pub expires: String,
    /// Semicolon-separated list of signed headers.
    pub signed_headers: String,
    /// Hex-encoded request signature.
    pub signature: String,
}

/// key chars: letters, digits, dash, dot
fn is_key_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '.'
}
/// val chars: anything except `&`
fn is_val_char(c: char) -> bool {
    c != '&'
}

/// Parse the `?` and then a list of `key=val` pairs separated by `&`
fn query_pairs(input: &str) -> IResult<&str, Vec<(&str, &str)>> {
    // skip everything up to the '?'
    let (input, _) = if let Some(i) = input.find('?') {
        // consume everything up to – and including – the first '?'
        nom::bytes::complete::take::<_, _, nom::error::Error<_>>(i + 1usize)(input)?
    } else {
        // the input *is* the query string, start parsing immediately
        ("", input)
    }; // then parse key=val (& key=val)* until the end
    separated_list1(
        nomchar('&'),
        separated_pair(
            take_while1(is_key_char),
            nomchar('='),
            take_while1(is_val_char),
        ),
    )
    .parse(input)
}

/// Parse all AWS presigned URL query parameters into a [`PresignedParams`] struct.
///
/// `input` must start with a `?` (or be a bare query string without a leading
/// `?` — in that case the parser expects a full URL-like input and will attempt
/// to locate the `?` separator).
///
/// All `X-Amz-*` values are percent-decoded before being stored.
///
/// # Errors
///
/// Returns a nom error if any mandatory field (`X-Amz-Algorithm`,
/// `X-Amz-Credential`, `X-Amz-Date`, `X-Amz-Expires`, `X-Amz-SignedHeaders`,
/// `X-Amz-Signature`) is absent.
pub fn parse_presigned_params(input: &str) -> IResult<&str, PresignedParams> {
    let (rest, pairs) = query_pairs(input)?;
    // build a little map
    let mut m = HashMap::new();
    for (k, v) in pairs {
        // percent-decode the value
        let val = percent_decode_str(v).decode_utf8_lossy().into_owned();
        m.insert(k, val);
    }

    // pull out each required field (error if missing)
    let algorithm = m.remove("X-Amz-Algorithm").ok_or_else(|| miss(rest))?;
    let credential_raw = m.remove("X-Amz-Credential").ok_or_else(|| miss(rest))?;
    let amz_date = m.remove("X-Amz-Date").ok_or_else(|| miss(rest))?;
    let expires = m.remove("X-Amz-Expires").ok_or_else(|| miss(rest))?;
    let signed_headers = m.remove("X-Amz-SignedHeaders").ok_or_else(|| miss(rest))?;
    let signature = m.remove("X-Amz-Signature").ok_or_else(|| miss(rest))?;

    // split the credential into its components
    // format is: ACCESSKEY/DATE/REGION/SERVICE/aws4_request
    let mut parts = credential_raw.split('/');
    let access_key = parts.next().unwrap_or("").to_string();
    let credential_date = parts.next().unwrap_or("").to_string();
    let region = parts.next().unwrap_or("").to_string();
    let service = parts.next().unwrap_or("").to_string();
    // ignore the final “aws4_request”

    Ok((
        rest,
        PresignedParams {
            algorithm,
            access_key,
            credential_date,
            region,
            service,
            amz_date,
            expires,
            signed_headers,
            signature,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nom::Err;

    #[test]
    fn test_parse_token_from_header() {
        let input = "AWS4-HMAC-SHA256 Credential=MYLOCAL123/20250417/eu-west-3/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=ec323a7db4d0b8bd27eced3b2bb0d59f9b9dd";
        let result = parse_token_from_header(input);
        assert_eq!(result, Ok(("", ("MYLOCAL123"))));
    }

    #[test]
    fn parse_token_from_header_success_and_error() {
        let input = "AWS4-HMAC-SHA256 Credential=TOKEN123/20250417/eu-west-1/s3/aws4_request, SignedHeaders=host,Signature=abc";
        let result = parse_token_from_header(input);
        assert!(result.is_ok());
        let (remaining, token) = result.unwrap();
        assert_eq!(token, "TOKEN123");
        assert_eq!(remaining, "");

        let bad = "NoCredentialHere";
        assert!(parse_token_from_header(bad).is_err());
    }

    #[test]
    fn parse_credential_scope_missing_prefix_fails() {
        assert!(parse_credential_scope("no-credential-here").is_err());
    }

    #[test]
    fn parse_token_from_header_empty_fails() {
        assert!(parse_token_from_header("").is_err());
    }

    #[test]
    fn parse_token_from_header_no_credential_fails() {
        assert!(parse_token_from_header("AWS4-HMAC-SHA256 SignedHeaders=host").is_err());
    }

    #[test]
    fn test_parse_valid_scope() {
        let header = "Credential=AKIAEXAMPLE/20250425/us-west-2/s3/aws4_request, SignedHeaders=host;x-amz-date";
        let (rem, (region, service)) = parse_credential_scope(header).expect("parse failed");
        assert_eq!(region, "us-west-2");
        assert_eq!(service, "s3");
        assert!(rem.starts_with(", SignedHeaders"));
    }

    #[test]
    fn test_parse_invalid_scope() {
        let header = "Credential=AKIAEXAMPLE/20250425/us-west-2/s3/some_request";
        assert!(matches!(parse_credential_scope(header), Err(Err::Error(_))));
    }

    #[test]
    fn test_parse_with_prefix() {
        let header = "Authorization: AWS4-HMAC-SHA256 Credential=XYZ/20250425/eu-central-1/dynamodb/aws4_request/extra";
        let idx = header.find("Credential=").unwrap();
        let substr = &header[idx..];
        let (rem, (region, service)) = parse_credential_scope(substr).expect("parse failed");
        assert_eq!(region, "eu-central-1");
        assert_eq!(service, "dynamodb");
        assert!(rem.starts_with("/extra"));
    }

    #[test]
    fn parses_all_fields() {
        let url = "http://localhost:6190/proxy-aws-bucket01/mandelbrot/?\
            X-Amz-Algorithm=AWS4-HMAC-SHA256&\
            X-Amz-Credential=MYLOCAL123%2F20250426%2Feu-west-3%2Fs3%2Faws4_request&\
            X-Amz-Date=20250426T143249Z&\
            X-Amz-Expires=3600&\
            X-Amz-SignedHeaders=host&\
            X-Amz-Signature=53cb3d8a12c8c1078fba3fcd55ced9c93fcdc8e2f98184e9ffea50245f4ebea5";

        let (_, p) = parse_presigned_params(url).unwrap();
        assert_eq!(p.algorithm, "AWS4-HMAC-SHA256");
        assert_eq!(p.access_key, "MYLOCAL123");
        assert_eq!(p.credential_date, "20250426");
        assert_eq!(p.region, "eu-west-3");
        assert_eq!(p.service, "s3");
        assert_eq!(p.amz_date, "20250426T143249Z");
        assert_eq!(p.expires, "3600");
        assert_eq!(p.signed_headers, "host");
        assert_eq!(
            p.signature,
            "53cb3d8a12c8c1078fba3fcd55ced9c93fcdc8e2f98184e9ffea50245f4ebea5"
        );
    }

    #[test]
    fn fails_if_missing_signature() {
        let url = "https://example.com/?X-Amz-Credential=AK/20250426/us-west-2/s3/aws4_request";
        assert!(parse_presigned_params(url).is_err());
    }

    #[test]
    fn parses_presigned_params_from_raw_query() {
        // the very same query string that shows up in the log
        let q = "X-Amz-Algorithm=AWS4-HMAC-SHA256&\
                 X-Amz-Credential=MYLOCAL123%2F20250426%2Feu-west-3%2Fs3%2Faws4_request&\
                 X-Amz-Date=20250426T143249Z&\
                 X-Amz-Expires=3600&\
                 X-Amz-SignedHeaders=host&\
                 X-Amz-Signature=53cb3d8a12c8c1078fba3fcd55ced9c93fcdc8e2f98184e9ffea50245f4ebea5";

        // ❌ This is what the proxy does today — and it should FAIL until we fix the parser.
        assert!(
            parse_presigned_params(q).is_err(),
            "the parser should reject a query that has no leading '?'"
        );

        // ✅ This is what the proxy *should* do (or what the parser should accept):
        let wrapped = format!("?{q}");
        let (_, p) = parse_presigned_params(&wrapped)
            .expect("parser must succeed when a leading '?' is present");

        assert_eq!(p.algorithm, "AWS4-HMAC-SHA256");
        assert_eq!(p.access_key, "MYLOCAL123");
        assert_eq!(p.credential_date, "20250426");
        assert_eq!(p.region, "eu-west-3");
        assert_eq!(p.service, "s3");
        assert_eq!(p.amz_date, "20250426T143249Z");
        assert_eq!(p.expires, "3600");
        assert_eq!(p.signed_headers, "host");
        assert_eq!(
            p.signature,
            "53cb3d8a12c8c1078fba3fcd55ced9c93fcdc8e2f98184e9ffea50245f4ebea5"
        );
    }
}
