use regex::{escape, Captures, Error as RegexError, Regex};
use std::str;

#[derive(Debug, Fail)]
pub enum FormatParserError {
    #[fail(display = "compiling the regular expression failed: {}", inner)]
    CompilationFailed { inner: RegexError },
}

/// a single entry/line in a log.
pub struct Entry<'a> {
    captures: Captures<'a>,
}

impl<'a> Entry<'a> {
    ///
    /// accesses the value of a named field in a log
    ///
    /// ```rust
    /// # extern crate nginx_log_parser;
    /// # use nginx_log_parser::Format;
    /// # use std::str::FromStr;
    /// #
    /// let format = Format::from_str("$remote_addr [$time_local] $request").unwrap();
    /// let entry = format.parse("1.2.3.4 [11/Sep/2018:08:44:17 +0000] GET / HTTP/1.1").unwrap();
    /// assert_eq!(Some("GET / HTTP/1.1"), entry.get("request"));
    /// ```
    pub fn get(&'a self, key: &str) -> Option<&'a str> {
        self.captures.name(key).map(|mat| mat.as_str())
    }

    /// checks if the log line contains a field named `key`
    pub fn has(&self, key: &str) -> bool {
        self.captures.name(key).is_some()
    }
}

#[derive(Debug)]
/// Represents the parsed format of an nginx log line.
/// Can be obtained by parsing an nginx log format string using `Format::from_str`.
pub struct Format<'a> {
    parts: Vec<FormatPart<'a>>,
    re: Regex,
}

impl<'a> Format<'a> {
    ///
    /// Reads an input line returning an optional `Entry` that contains the parsed result.
    ///
    /// # Example
    /// ```rust
    /// # extern crate nginx_log_parser;
    /// # use nginx_log_parser::Format;
    /// # use std::str::FromStr;
    /// #
    /// let format = Format::from_str("$remote_addr [$time_local] $request").unwrap();
    /// let entry = format.parse("1.2.3.4 [11/Sep/2018:08:44:17 +0000] GET / HTTP/1.1");
    /// assert_eq!(Some("GET / HTTP/1.1"), entry.unwrap().get("request"));
    /// ```
    ///
    /// # Invalid input
    /// May return `None` when the format did not match the input line.
    ///
    /// ```rust
    /// # extern crate nginx_log_parser;
    /// # use nginx_log_parser::Format;
    /// # use std::str::FromStr;
    /// #
    /// let format = Format::from_str("$remote_addr [$time_local] $request").unwrap();
    /// assert!(format.parse("this does not work").is_none());
    /// ```
    pub fn parse<'b>(&self, line: &'b str) -> Option<Entry<'b>> {
        // TODO: i do not want to use regex here but i'm not smart enough to write my own parser
        self.re.captures(line).map(|captures| Entry { captures })
    }

    /// creates a format from a list of FormatParts. currently internal.
    fn from_parts(parts: Vec<FormatPart>) -> Result<Format, FormatParserError> {
        let pattern: String = parts.iter().map(|part| part.get_pattern()).collect();
        let re = match Regex::new(&pattern) {
            Ok(re) => re,
            Err(err) => return Err(FormatParserError::CompilationFailed { inner: err }),
        };

        Ok(Format { parts, re })
    }

    pub fn new(input: &str) -> Result<Format, FormatParserError> {
        read_format(input.as_bytes())
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum FormatPart<'a> {
    Variable(&'a str),
    Fixed(&'a str),
}

impl<'a> FormatPart<'a> {
    fn get_pattern(&self) -> String {
        use self::FormatPart::*;
        match self {
            Variable(name) => format!(
                "(?P<{}>{})",
                name.trim_left_matches('$'),
                match *name {
                    "$status" => "\\d{3}",
                    "$body_bytes_sent" => "\\d+",
                    _ => ".*",
                }
            ),
            Fixed(fixed_part) => escape(fixed_part),
        }
    }
}

enum FormatParserState {
    Start,
    Variable(usize, usize),
    Fixed(usize, usize),
}

fn is_var_char(char: &u8) -> bool {
    match char {
        b'a'...b'z' | b'A'...b'Z' | b'_' => true,
        _ => false,
    }
}

fn read_byte(chr: &u8, index: usize, state: &FormatParserState) -> FormatParserState {
    use format::FormatParserState::*;
    match state {
        Start => match chr {
            b'$' => Variable(index, index + 1),
            _ => Fixed(index, index + 1),
        },
        Variable(start, _end) => match chr {
            x if is_var_char(x) => Variable(*start, index + 1),
            _ => Fixed(index, index + 1),
        },
        Fixed(start, _end) => match chr {
            b'$' => Variable(index, index + 1),
            _ => Fixed(*start, index + 1),
        },
    }
}

fn read_format(bytes: &[u8]) -> Result<Format, FormatParserError> {
    use format::FormatParserState::*;
    let mut state = Start;
    let mut stack = vec![];
    unsafe {
        for i in 0..bytes.len() {
            let new_state = read_byte(&bytes[i], i, &state);
            match (&state, &new_state) {
                (Variable(start, end), Fixed(_, _)) => stack.push(FormatPart::Variable(
                    str::from_utf8_unchecked(&bytes[*start..*end]),
                )),
                (Fixed(start, end), Variable(_, _)) => stack.push(FormatPart::Fixed(
                    str::from_utf8_unchecked(&bytes[*start..*end]),
                )),
                _ => {}
            };

            state = new_state
        }
        match &state {
            Variable(start, end) => stack.push(FormatPart::Variable(
                str::from_utf8_unchecked(&bytes[*start..*end]),
            )),
            Fixed(start, end) => stack.push(FormatPart::Fixed(
                str::from_utf8_unchecked(&bytes[*start..*end]),
            )),
            _ => {}
        };
    }

    Format::from_parts(stack)
}

#[cfg(test)]
mod test {
    use format::Format;
    use format::FormatPart::Fixed;
    use format::FormatPart::Variable;
    use std::str::FromStr;

    #[test]
    fn test_parse_format() {
        let format_input = r#"$remote_addr - $remote_user [$time_local] "$request" $status $body_bytes_sent "$http_referer" "$http_user_agent" "$http_x_forwarded_for""#;

        let format = Format::from_str(format_input).unwrap();

        assert_eq!(
            Some(&Variable("$remote_addr")),
            format.parts.get(0)
        );
        assert_eq!(Some(&Fixed(" - ")), format.parts.get(1));
        assert_eq!(
            Some(&Variable("$remote_user")),
            format.parts.get(2)
        );
        assert_eq!(Some(&Fixed(" [")), format.parts.get(3));
        assert_eq!(
            Some(&Variable("$time_local")),
            format.parts.get(4)
        );
        assert_eq!(Some(&Fixed(r#"] ""#)), format.parts.get(5));
        assert_eq!(Some(&Fixed(r#"] ""#)), format.parts.get(5));
        assert_eq!(
            Some(&Variable("$request")),
            format.parts.get(6)
        );
        assert_eq!(Some(&Fixed(r#"" ""#)), format.parts.get(15));
        assert_eq!(Some(&Fixed(r#"""#)), format.parts.get(17));
    }

    #[test]
    fn test_parse_main_format() {
        let data = r#"192.0.2.139 - - [11/Sep/2018:13:45:22 +0000] "GET /favicon.ico HTTP/1.1" 404 142 "-" "python-requests/2.13.0""#;
        let format_input = r#"$remote_addr - $remote_user [$time_local] "$request" $status $body_bytes_sent "$http_referer" "$http_user_agent""#;
        let format = Format::from_str(format_input).unwrap();
        let result = format.parse(data).unwrap();

        assert_eq!(Some("192.0.2.139"), result.get("remote_addr"));
        assert_eq!(Some("-"), result.get("remote_user"));
        assert_eq!(Some("11/Sep/2018:13:45:22 +0000"), result.get("time_local"));
        assert_eq!(Some("GET /favicon.ico HTTP/1.1"), result.get("request"));
        assert_eq!(Some("404"), result.get("status"));
        assert_eq!(Some("142"), result.get("body_bytes_sent"));
        assert_eq!(Some("-"), result.get("http_referer"));
        assert_eq!(
            Some("python-requests/2.13.0"),
            result.get("http_user_agent")
        );
    }
}
