use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::take_while1,
    character::complete::char,
    combinator::{eof, map, rest},
    sequence::preceded,
};

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
        &input[input.find(rest).unwrap() - 1..]
    };

    Ok(("", (bucket, rest_path)))
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
}
