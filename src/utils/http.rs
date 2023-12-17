use std::string::ToString;
use std::collections::HashMap;

// push percent-encoded digit
fn _push_unicode_digit(digit: u8, result: &mut String) {
    result.push('%');
    result.push(
        char::from_digit((digit>>4).into(), 16)
            .unwrap()
            .to_uppercase()
            .next()
            .unwrap()
    );
    result.push(
        char::from_digit((digit&15).into(), 16)
            .unwrap()
            .to_uppercase()
            .next()
            .unwrap()
    );
}

// read percent-encoded byte into u8
// if first != true then `%` has already been fetched
fn _read_unicode_digit(chars: &mut std::str::Chars<'_>, first: bool) -> Result<u8, ()> {
    let mut digit: u8;
    if first {
        if let Some(c_char) = chars.next() {
            if c_char != '%' {
                return Err(())
            }
        } else {
            return Err(())
        }
    }
    if let Some(c_char) = chars.next() {
        if let Some(first_digit) = c_char.to_digit(16) {
            digit = (first_digit as u8) <<4;
        } else {
            return Err(())
        }
    } else {
        return Err(())
    }
    if let Some(c_char) = chars.next() {
        if let Some(second_digit) = c_char.to_digit(16) {
            digit += second_digit as u8;
        } else {
            return Err(())
        }
    } else {
        return Err(())
    }
    Ok(digit)
}

// Normalize term with proper percent-encoding
pub fn normalize_term(part: String) -> String {
    let mut result = String::new();
    let mut chars = part.chars();
    let mut buffer = [0; 4];
    loop {
        if let Some(c_char) = chars.next() {
            c_char.encode_utf8(&mut buffer);
            if c_char == '%' {
                if let Ok(digit) = _read_unicode_digit(&mut chars, false) {
                    buffer[0] = digit;
                } else {
                    return String::new();
                }
                if buffer[0] >= 192 {
                    if let Ok(digit) = _read_unicode_digit(&mut chars, true) {
                        buffer[1] = digit
                    } else {
                        return String::new();
                    }
                }
                if buffer[0] >= 224 {
                    if let Ok(digit) = _read_unicode_digit(&mut chars, true) {
                        buffer[2] = digit
                    } else {
                        return String::new();
                    }
                }
                if buffer[0] >= 240 {
                    if let Ok(digit) = _read_unicode_digit(&mut chars, true) {
                        buffer[3] = digit
                    } else {
                        return String::new();
                    }
                }
            }
            if (buffer[0] <= 0x5a && buffer[0] >= 0x41)
                || (buffer[0] >= 0x61 && buffer[0] <= 0x7A)
                || (buffer[0] >= 0x30 && buffer[0] <= 0x39)
                || buffer[0] == 0x2d
                || buffer[0] == 0x2e
                || buffer[0] == 0x5f
                || buffer[0] == 0x7e {
                result.push(c_char);
                continue;
            }
            _push_unicode_digit(buffer[0], &mut result);
            if buffer[0] >= 192 {
                _push_unicode_digit(buffer[1], &mut result);
            }
            if buffer[0] >= 224 {
                _push_unicode_digit(buffer[2], &mut result);
            }
            if buffer[0] >= 240 && buffer[0] <= 247 {
                _push_unicode_digit(buffer[3], &mut result);
            } if buffer[0] > 247 {
                return String::new()
            }
        } else {
            return result
        }
    }
}

// Normalize uri, which includes path, query params, reference
pub fn normalized(source: String) -> String {
    let mut path: Vec<String> = Vec::new();
    let mut params: HashMap<String, Option<String>> = HashMap::new();
    let mut reference: String = String::new();
    let query_end = source.find("#").unwrap_or(source.len());
    let query = &source[..query_end];
    let path_end = query.find("?").unwrap_or(query.len());
    let local_path = &query[..path_end];
    for split in local_path[1..].split("/") {
        if split == "." {
            continue;
        }
        else if split.len() == 0 {
            path.push(split.to_string());
        }
        else if split == ".." {
            path.pop();
        } else {
            path.push(normalize_term(String::from(split)));
        }
    }
    if path_end < query.len() {
        for split in query[(path_end+1)..].split("&") {
            let eq = split.find("=").unwrap_or(split.len());
            if eq < split.len() {
                params.insert(
                    String::from(&split[..eq]),
                    Some(String::from(
                            split[eq..]
                            .strip_prefix("=")
                            .unwrap())
                        )
                    );
            } else {
                params.insert(String::from(split), None);
            }

        }
    }
    if query_end < source.len() {
        reference = String::from(
            source[query_end..]
                .strip_prefix("#")
                .unwrap()
            );
    }
    let mut result = String::new();
    for path in &path {
        result.push('/');
        result = result + path.as_str();
    }
    if params.len() > 0 {
        let mut result_params = String::new();
        for (k, v) in &params {
            result_params = result_params + "&" + k.as_str();
            if let Some(value) = v {
                result_params = result_params + "=" + value.as_str();
            }
        }
        result = result + "?" + &result_params[1..];
    }
    if reference.len() > 0 {
        result = result + "#" + &reference;
    }
    return result;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_normalize() {
        assert_eq!(normalized(String::from("/test/../../test1/./tеst2?test1=1&test2=2&test3#ref")), String::from("/test1/t%D0%B5st2?test1=1&test2=2&test3#ref"));
    }
}
