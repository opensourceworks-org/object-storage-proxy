use nom::{
    IResult, Parser,
    bytes::complete::{tag, take_until},
    sequence::preceded,
};

pub fn parse_token_from_header(header: &str) -> IResult<&str, &str> {
    let (_, token) =
        (preceded(tag("AWS4-HMAC-SHA256 Credential="), take_until("/"))).parse(header)?;

    Ok(("", token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_token_from_header() {
        let input = "AWS4-HMAC-SHA256 Credential=MYLOCAL123/20250417/eu-west-3/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=ec323a7db4d0b8bd27eced3b2bb0d59f9b9dd";
        let result = parse_token_from_header(input);
        assert_eq!(result, Ok(("", ("MYLOCAL123"))));
    }
}
