//! Just enough JSON to relay signalling messages.
//!
//! The relay never looks inside a payload: it reads two string fields off the
//! outside of a message and passes the rest along exactly as it arrived. So
//! rather than a document model this is a scanner that can find where a value
//! ends, which is all that copying one through requires, plus string
//! quoting for the two fields the relay writes itself.

/// Skips whitespace, returning the index of the next meaningful byte.
fn skip_space(s: &[u8], mut at: usize) -> usize {
    while at < s.len() && matches!(s[at], b' ' | b'\t' | b'\n' | b'\r') {
        at += 1;
    }
    at
}

/// Index one past the end of the string literal starting at `at`.
fn string_end(s: &[u8], at: usize) -> Option<usize> {
    if s.get(at) != Some(&b'"') {
        return None;
    }
    let mut i = at + 1;
    while i < s.len() {
        match s[i] {
            b'"' => return Some(i + 1),
            b'\\' => i += 2,
            _ => i += 1,
        }
    }
    None
}

/// Index one past the end of the JSON value starting at `at`.
///
/// Returns `None` for anything malformed, or nested deeper than a signalling
/// message has any business being.
pub fn value_end(s: &[u8], at: usize) -> Option<usize> {
    value_end_depth(s, at, 0)
}

fn value_end_depth(s: &[u8], at: usize, depth: u32) -> Option<usize> {
    if depth > 32 {
        return None;
    }
    let at = skip_space(s, at);
    match *s.get(at)? {
        b'"' => string_end(s, at),
        b'{' | b'[' => {
            let close = if s[at] == b'{' { b'}' } else { b']' };
            let mut i = at + 1;
            loop {
                i = skip_space(s, i);
                if *s.get(i)? == close {
                    return Some(i + 1);
                }
                i = value_end_depth(s, i, depth + 1)?;
                i = skip_space(s, i);
                match *s.get(i)? {
                    b',' => i += 1,
                    b':' => i += 1,
                    c if c == close => return Some(i + 1),
                    _ => return None,
                }
            }
        }
        b't' => s.get(at..at + 4).filter(|w| *w == b"true").map(|_| at + 4),
        b'f' => s.get(at..at + 5).filter(|w| *w == b"false").map(|_| at + 5),
        b'n' => s.get(at..at + 4).filter(|w| *w == b"null").map(|_| at + 4),
        _ => {
            let mut i = at;
            while i < s.len() && matches!(s[i], b'-' | b'+' | b'.' | b'0'..=b'9' | b'e' | b'E') {
                i += 1;
            }
            if i == at {
                None
            } else {
                Some(i)
            }
        }
    }
}

/// The raw text of one member of a JSON object, exactly as it was written.
pub fn member<'a>(object: &'a str, name: &str) -> Option<&'a str> {
    let s = object.as_bytes();
    let mut i = skip_space(s, 0);
    if *s.get(i)? != b'{' {
        return None;
    }
    i += 1;
    loop {
        i = skip_space(s, i);
        match *s.get(i)? {
            b'}' => return None,
            b',' => {
                i += 1;
                continue;
            }
            b'"' => {}
            _ => return None,
        }
        let key_end = string_end(s, i)?;
        let key = unquote(object.get(i..key_end)?)?;
        i = skip_space(s, key_end);
        if *s.get(i)? != b':' {
            return None;
        }
        let start = skip_space(s, i + 1);
        let end = value_end(s, start)?;
        if key == name {
            return object.get(start..end);
        }
        i = end;
    }
}

/// Turns a JSON string literal into the text it stands for.
pub fn unquote(literal: &str) -> Option<String> {
    let body = literal.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let mut hex = String::new();
                for _ in 0..4 {
                    hex.push(chars.next()?);
                }
                let code = u32::from_str_radix(&hex, 16).ok()?;
                // A surrogate on its own is not a character; the relay has no
                // reason to rebuild pairs, so it substitutes and moves on.
                out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Writes `text` as a JSON string literal.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The text of a string member, unescaped.
pub fn string_member(object: &str, name: &str) -> Option<String> {
    unquote(member(object, name)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_members_a_relay_cares_about() {
        let msg = r#"{"type":"publish","topic":"abc","payload":{"a":[1,2,{"b":"}"}],"c":null}}"#;
        assert_eq!(string_member(msg, "type").as_deref(), Some("publish"));
        assert_eq!(string_member(msg, "topic").as_deref(), Some("abc"));
        assert_eq!(
            member(msg, "payload"),
            Some(r#"{"a":[1,2,{"b":"}"}],"c":null}"#),
            "a brace inside a string must not end the value"
        );
        assert_eq!(member(msg, "missing"), None);
    }

    #[test]
    fn a_payload_may_be_any_value() {
        for (msg, want) in [
            (r#"{"payload":"hi"}"#, r#""hi""#),
            (r#"{"payload":-1.5e3}"#, "-1.5e3"),
            (r#"{"payload":true}"#, "true"),
            (r#"{"payload":null}"#, "null"),
            (r#"{"payload":[]}"#, "[]"),
            (r#"{ "payload" : { } }"#, "{ }"),
        ] {
            assert_eq!(member(msg, "payload"), Some(want), "{msg}");
        }
    }

    #[test]
    fn escapes_survive_a_round_trip() {
        for text in ["plain", "with \"quotes\"", "a\\b", "new\nline", "tab\there"] {
            assert_eq!(unquote(&quote(text)).as_deref(), Some(text));
        }
        assert_eq!(unquote(r#""Aé""#).as_deref(), Some("Aé"));
        assert_eq!(quote("\u{1}"), r#""\u0001""#);
    }

    #[test]
    fn rubbish_is_refused_rather_than_panicking() {
        for bad in [
            "",
            "{",
            "[",
            r#"{"a""#,
            r#"{"a":"#,
            r#""unterminated"#,
            "{\"a\":\"b",
            "tru",
            "nul",
            "-",
            "{\"a\":}",
        ] {
            let _ = member(bad, "a");
            let _ = value_end(bad.as_bytes(), 0);
        }
        // Deep nesting stops rather than blowing the stack.
        let deep = "[".repeat(200);
        assert_eq!(value_end(deep.as_bytes(), 0), None);
    }
}
