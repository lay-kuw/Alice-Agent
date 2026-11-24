//! Small shared helpers: IDs, time, logging.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const ID_ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Random 12-character ID (60 bits), lowercase Crockford-like base32.
/// Platform independent and short enough to type in chat commands.
pub fn new_id() -> String {
    let mut buf = [0u8; 12];
    getrandom::fill(&mut buf).expect("OS random source unavailable");
    buf.iter().map(|b| ID_ALPHABET[(*b & 31) as usize] as char).collect()
}

/// 32 random bytes as hex (API tokens).
pub fn new_token() -> String {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).expect("OS random source unavailable");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Constant-time comparison for secrets.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Format unix milliseconds as `YYYY-MM-DD HH:MM:SS` in UTC.
pub fn fmt_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

static QUIET: AtomicBool = AtomicBool::new(false);

pub fn set_log_quiet(q: bool) {
    QUIET.store(q, Ordering::Relaxed);
}

/// Minimal logger writing to stderr (redirected to the log file by the service).
/// Never pass secrets or message content to it.
pub fn log(level: &str, msg: std::fmt::Arguments) {
    if QUIET.load(Ordering::Relaxed) {
        return;
    }
    let line = format!("{} {level:<5} {msg}\n", fmt_utc(now_ms()));
    let _ = std::io::stderr().write_all(line.as_bytes());
}

#[macro_export]
macro_rules! info { ($($t:tt)*) => { $crate::util::log("INFO", format_args!($($t)*)) } }
#[macro_export]
macro_rules! warn { ($($t:tt)*) => { $crate::util::log("WARN", format_args!($($t)*)) } }
#[macro_export]
macro_rules! error { ($($t:tt)*) => { $crate::util::log("ERROR", format_args!($($t)*)) } }

/// Truncate a string at a char boundary to at most `max` bytes.
pub fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_short() {
        let a = new_id();
        let b = new_id();
        assert_eq!(a.len(), 12);
        assert_ne!(a, b);
    }

    #[test]
    fn utc_format() {
        assert_eq!(fmt_utc(0), "1970-01-01 00:00:00");
        assert_eq!(fmt_utc(1_700_000_000_000), "2023-11-14 22:13:20");
    }

    #[test]
    fn truncate_respects_chars() {
        assert_eq!(truncate("あいう", 4), "あ");
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod b64_tests {
    #[test]
    fn encodes() {
        assert_eq!(super::b64_encode(b""), "");
        assert_eq!(super::b64_encode(b"f"), "Zg==");
        assert_eq!(super::b64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(super::b64_encode(b"foobar"), "Zm9vYmFy");
    }
}

/// URL-safe base64 without padding (PKCE, Gmail raw messages).
pub fn b64url_encode(data: &[u8]) -> String {
    b64_encode(data).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

/// Decode standard or URL-safe base64 (padding optional, whitespace ignored).
pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// Percent-encode everything except unreserved characters.
pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod b64_decode_tests {
    #[test]
    fn roundtrip() {
        for s in ["", "f", "fo", "foo", "日本語テキスト", "\u{0}\u{ff}"] {
            assert_eq!(super::b64_decode(&super::b64_encode(s.as_bytes())).unwrap(), s.as_bytes());
            assert_eq!(super::b64_decode(&super::b64url_encode(s.as_bytes())).unwrap(), s.as_bytes());
        }
        assert_eq!(super::urlencode("a b/ü"), "a%20b%2F%C3%BC");
    }
}
