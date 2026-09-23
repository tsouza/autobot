//! AWS Signature Version 4 for S3 requests with a signed payload, and the encodings it uses.

use autobot_adapters::artifact::sha256;
use ring::hmac;
use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

/// One request as the signature sees it.
pub(super) struct Request<'a> {
    /// The HTTP method.
    pub(super) method: &'a str,
    /// The request path, already encoded with [`encode_path`].
    pub(super) path: &'a str,
    /// The query parameters, not encoded.
    pub(super) query: &'a [(&'a str, &'a str)],
    /// Every header to sign, with a lowercase name: at least `host`, `x-amz-date` and
    /// `x-amz-content-sha256`.
    pub(super) headers: &'a [(&'a str, &'a str)],
    /// The lowercase hexadecimal SHA-256 of the body.
    pub(super) payload_sha256: &'a str,
}

/// The credentials and region a request is signed for.
pub(super) struct Signer<'a> {
    pub(super) access_key: &'a str,
    pub(super) secret_key: &'a str,
    pub(super) region: &'a str,
}

/// Lowercase hexadecimal.
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// URI-encodes every byte of `text` but the unreserved characters, and `/` too when
/// `keep_slash`.
fn encode(text: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'.' | b'_' | b'~')
            || (keep_slash && b == b'/')
        {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// A path encoded for a request line and for the canonical request.
pub(super) fn encode_path(path: &str) -> String {
    encode(path, true)
}

/// The query string of `query`, sorted and encoded: the canonical query string, which is also
/// what the request sends.
pub(super) fn encode_query(query: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (encode(k, false), encode(v, false)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// The lowercase hexadecimal SHA-256 of `bytes`.
pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    hex(sha256(bytes).as_bytes())
}

fn mac(key: &[u8], data: &str) -> hmac::Tag {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data.as_bytes())
}

impl Signer<'_> {
    /// The `Authorization` header of `request` sent at `amz_date` (`YYYYMMDD'T'HHMMSS'Z'`),
    /// which must also be the request's `x-amz-date` header.
    pub(super) fn authorization(&self, request: &Request<'_>, amz_date: &str) -> String {
        let mut headers: Vec<(&str, &str)> = request.headers.to_vec();
        headers.sort_unstable();
        let canonical_headers: String = headers
            .iter()
            .map(|(k, v)| format!("{k}:{}\n", v.trim()))
            .collect();
        let signed = headers
            .iter()
            .map(|(k, _)| *k)
            .collect::<Vec<_>>()
            .join(";");
        let canonical = format!(
            "{}\n{}\n{}\n{canonical_headers}\n{signed}\n{}",
            request.method,
            request.path,
            encode_query(request.query),
            request.payload_sha256
        );
        let date = amz_date.get(..8).unwrap_or(amz_date);
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical.as_bytes())
        );
        let key = [date, self.region, "s3", "aws4_request"].iter().fold(
            format!("AWS4{}", self.secret_key).into_bytes(),
            |key, part| mac(&key, part).as_ref().to_vec(),
        );
        format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={}",
            self.access_key,
            hex(mac(&key, &to_sign).as_ref())
        )
    }
}

/// `seconds` since the Unix epoch as an `x-amz-date` value, `YYYYMMDD'T'HHMMSS'Z'` in UTC.
pub(super) fn amz_date(seconds: u64) -> String {
    let days = seconds / 86_400;
    let rem = seconds % 86_400;
    // Civil date from days since 1970-01-01 (the proleptic Gregorian calendar), in 400-year
    // eras of 146 097 days that start on 0000-03-01.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// The current time as an `x-amz-date` value.
pub(super) fn now() -> String {
    amz_date(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    )
}
