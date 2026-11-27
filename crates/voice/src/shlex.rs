//! Python `shlex.split` (POSIX mode, no comments) and `shlex.quote`.

use crate::{Error, Result};

const WHITESPACE: &[char] = &[' ', '\t', '\r', '\n'];

fn is_quote(ch: char) -> bool {
    ch == '\'' || ch == '"'
}

/// `shlex.split(text)`: the same tokens and the same `ValueError` texts.
pub fn split(text: &str) -> Result<Vec<String>> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut at = 0;
    let mut out = Vec::new();
    loop {
        // One `read_token` call of Python's shlex.
        let mut state = ' ';
        let mut escaped_state = 'a';
        let mut token = String::new();
        let mut quoted = false;
        let mut ended = false;
        loop {
            let next = chars.get(at).copied();
            at += 1;
            if is_quote(state) {
                quoted = true;
                match next {
                    None => return Err(Error::value("No closing quotation")),
                    Some(ch) if ch == state => state = 'a',
                    Some('\\') if state == '"' => {
                        escaped_state = state;
                        state = '\\';
                    }
                    Some(ch) => token.push(ch),
                }
            } else if state == '\\' {
                match next {
                    None => return Err(Error::value("No escaped character")),
                    Some(ch) => {
                        if is_quote(escaped_state) && ch != state && ch != escaped_state {
                            token.push(state);
                        }
                        token.push(ch);
                        state = escaped_state;
                    }
                }
            } else if state == ' ' {
                match next {
                    None => {
                        ended = true;
                        break;
                    }
                    Some(ch) if WHITESPACE.contains(&ch) => {
                        if !token.is_empty() || quoted {
                            break;
                        }
                    }
                    Some('\\') => {
                        escaped_state = 'a';
                        state = '\\';
                    }
                    Some(ch) if is_quote(ch) => state = ch,
                    Some(ch) => {
                        token.push(ch);
                        state = 'a';
                    }
                }
            } else {
                // state 'a'
                match next {
                    None => {
                        ended = true;
                        break;
                    }
                    Some(ch) if WHITESPACE.contains(&ch) => {
                        if !token.is_empty() || quoted {
                            break;
                        }
                        state = ' ';
                    }
                    Some(ch) if is_quote(ch) => state = ch,
                    Some('\\') => {
                        escaped_state = 'a';
                        state = '\\';
                    }
                    Some(ch) => token.push(ch),
                }
            }
        }
        if token.is_empty() && !quoted {
            return Ok(out);
        }
        out.push(token);
        if ended {
            return Ok(out);
        }
    }
}

/// `shlex.quote(text)`.
pub fn quote(text: &str) -> String {
    if text.is_empty() {
        return "''".into();
    }
    let safe = text
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "_@%+=:,./-".contains(ch));
    if safe {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_python_posix_shlex() {
        assert_eq!(
            split("a 'b c' \"d\\\"e\" f\\ g #h").unwrap(),
            ["a", "b c", "d\"e", "f g", "#h"]
        );
        assert_eq!(split("''").unwrap(), [""]);
        assert_eq!(split("\"a\\$b\"").unwrap(), ["a\\$b"]);
        assert_eq!(split("  ").unwrap(), Vec::<String>::new());
        assert_eq!(
            split("'x").unwrap_err().message,
            "No closing quotation".to_owned()
        );
        assert_eq!(
            split("x\\").unwrap_err().message,
            "No escaped character".to_owned()
        );
    }

    #[test]
    fn quotes_like_python() {
        assert_eq!(quote(""), "''");
        assert_eq!(quote("/a/b.bin"), "/a/b.bin");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), "'it'\"'\"'s'");
    }
}
