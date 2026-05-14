use std::{collections::HashMap, string::FromUtf8Error};

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{tag, take_until, take_while1},
    character::complete::char,
    combinator::{eof, map, map_res, rest},
    multi::separated_list0,
    sequence::{preceded, separated_pair},
};

/// Parse an S3-style request path into `(bucket, object_path)`.
///
/// The expected format is `/<bucket>[/<object_path>]`.
///
/// * `/my-bucket` -> `("my-bucket", "/")`
/// * `/my-bucket/prefix/key` -> `("my-bucket", "/prefix/key")`
///
/// # Errors
///
/// Returns a nom error if the input is empty or does not start with `/`.
pub(crate) fn parse_path(input: &str) -> IResult<&str, (&str, &str)> {
    let (_remaining, (_, bucket, rest)) = (
        char('/'),
        take_while1(|c| c != '/'),
        alt((preceded(char('/'), rest), map(eof, |_| ""))),
    )
        .parse(input)?;

    let rest_path = if rest.is_empty() {
        "/"
    } else {
        // recover the slash before `rest`
        &input[input.find(rest).expect("rest is a substring of input") - 1..]
    };

    Ok(("", (bucket, rest_path)))
}

fn decode_segment(input: &str) -> Result<String, FromUtf8Error> {
    urlencoding::decode(input).map(|s| s.to_string())
}

fn key_value_pair(input: &str) -> IResult<&str, (String, String)> {
    // First try the normal key=value form.
    if input.contains('=')
        && (input
            .find('&')
            .is_none_or(|a| input.find('=').is_some_and(|e| e < a)))
    {
        let (input, (key, value)) = (separated_pair(
            map_res(take_until("="), decode_segment),
            tag("="),
            map_res(take_until_either("&"), decode_segment),
        ))
        .parse(input)?;
        return Ok((input, (key, value)));
    }
    // Bare sub-resource key with no value (e.g. "delete", "uploads", "tagging").
    let (input, key) = map_res(take_until_either("&"), decode_segment).parse(input)?;
    Ok((input, (key, String::new())))
}

fn take_until_either<'a>(end: &'static str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    move |input: &'a str| match input.find(end) {
        Some(idx) => Ok((&input[idx..], &input[..idx])),
        None => rest(input),
    }
}

/// Parse a URL-encoded query string into a key/value map.
///
/// Keys and values are percent-decoded.  An empty input yields an empty map.
///
/// # Errors
///
/// Returns a nom error if any key/value pair cannot be parsed.
pub fn parse_query(input: &str) -> IResult<&str, HashMap<String, String>> {
    let (rest, pairs) = (separated_list0(char('&'), key_value_pair)).parse(input)?;
    let map = pairs.into_iter().collect::<HashMap<_, _>>();
    Ok((rest, map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_path_with_bucket_and_path() {
        let input = "/bucket_name/some/path";
        let result = parse_path(input);
        assert_eq!(result, Ok(("", ("bucket_name", "/some/path"))));
    }

    #[test]
    fn test_parse_path_with_bucket_only() {
        let input = "/bucket_name";
        let result = parse_path(input);
        assert_eq!(result, Ok(("", ("bucket_name", "/"))));
    }

    #[test]
    fn test_parse_path_with_empty_input() {
        let input = "";
        let result = parse_path(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_path_with_no_leading_slash() {
        let input = "bucket_name/some/path";
        let result = parse_path(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_path_with_trailing_slash() {
        let input = "/bucket_name/";
        let result = parse_path(input);
        assert_eq!(result, Ok(("", ("bucket_name", "/"))));
    }

    #[test]
    fn test_parse_path_with_multiple_slashes_in_path() {
        let input = "/bucket_name/some//path";
        let result = parse_path(input);
        assert_eq!(result, Ok(("", ("bucket_name", "/some//path"))));
    }

    #[test]
    fn test_parse_path_with_special_characters_in_bucket() {
        let input = "/bucket-name_123/some/path";
        let result = parse_path(input);
        assert_eq!(result, Ok(("", ("bucket-name_123", "/some/path"))));
    }

    #[test]
    fn test_parse_path_with_special_characters_in_path() {
        let input = "/bucket_name/some/path-with_special.chars";
        let result = parse_path(input);
        assert_eq!(
            result,
            Ok(("", ("bucket_name", "/some/path-with_special.chars")))
        );
    }

    #[test]
    fn test_parse_query_nom_urlencoded() {
        let input = "name=John%20Doe&path=%2Fusr%2Fbin&lang=Rust%26C%2B%2B";
        let (_rest, map) = parse_query(input).unwrap();
        assert_eq!(map.get("name").unwrap(), "John Doe");
        assert_eq!(map.get("path").unwrap(), "/usr/bin");
        assert_eq!(map.get("lang").unwrap(), "Rust&C++");
    }
}
