pub fn parse_f32(s: &str) -> Option<f32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let bytes = s.as_bytes();
    let (negative, chars) = if bytes[0] == b'-' {
        (true, &bytes[1..])
    } else if bytes[0] == b'+' {
        (false, &bytes[1..])
    } else {
        (false, bytes)
    };

    let mut integer_part: u32 = 0;
    let mut fraction_part: u32 = 0;
    let mut fraction_digits: u32 = 0;
    let mut seen_dot = false;
    let mut any_digit = false;

    for &b in chars {
        if b == b'.' {
            if seen_dot {
                return None;
            }
            seen_dot = true;
        } else if b.is_ascii_digit() {
            any_digit = true;
            let d = (b - b'0') as u32;
            if seen_dot {
                if fraction_digits < 6 {
                    fraction_part = fraction_part * 10 + d;
                    fraction_digits += 1;
                }
            } else {
                integer_part = integer_part.checked_mul(10)?.checked_add(d)?;
            }
        } else {
            return None;
        }
    }

    if !any_digit {
        return None;
    }

    let mut result = integer_part as f32;
    if fraction_digits > 0 {
        let mut divisor = 1u32;
        for _ in 0..fraction_digits {
            divisor *= 10;
        }
        result += fraction_part as f32 / divisor as f32;
    }
    if negative {
        result = -result;
    }
    Some(result)
}

pub fn parse_f32_bytes(s: &[u8]) -> Option<f32> {
    core::str::from_utf8(s).ok().and_then(parse_f32)
}

pub fn line_contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    for i in 0..=(haystack.len() - needle.len()) {
        if &haystack[i..i + needle.len()] == needle {
            return true;
        }
    }
    false
}

pub fn extract_value_after(line: &[u8], prefix: &[u8]) -> Option<f32> {
    let plen = prefix.len();
    if plen > line.len() {
        return None;
    }
    let mut start = None;
    for i in 0..=(line.len() - plen) {
        if &line[i..i + plen] == prefix {
            start = Some(i + plen);
            break;
        }
    }
    let start = start?;
    let mut end = start;
    while end < line.len() && (line[end].is_ascii_digit() || line[end] == b'.') {
        end += 1;
    }
    if end == start {
        return None;
    }
    parse_f32_bytes(&line[start..end])
}
