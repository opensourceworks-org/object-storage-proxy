use nom::{
    bytes::complete::{tag, take_until}, sequence::{preceded, tuple}, IResult, Parser
};

pub fn parse_token_from_header(header: &str) -> IResult<&str, &str> {
    let (_, token) =
        (preceded(tag("AWS4-HMAC-SHA256 Credential="), take_until("/"))).parse(header)?;

    Ok(("", token))
}

pub fn parse_credential_scope(input: &str) -> IResult<&str, (&str, &str)> {
    let (input, _) = take_until("Credential=")(input)?;
    let (remaining, (_, _, _, _, _, region, _, service, _)) = (
        tag("Credential="),          // prefix
        take_until("/"),            // access key
        tag("/"),
        take_until("/"),            // date
        tag("/"),
        take_until("/"),            // region
        tag("/"),
        take_until("/aws4_request"),// service
        tag("/aws4_request"),       // trailing
    ).parse(input)?;
    Ok((remaining, (region, service)))
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
}


