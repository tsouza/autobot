//! The few S3 XML response elements the adapter reads.
//!
//! S3 answers with flat, namespaced documents in which each element the adapter needs is a leaf
//! holding text, so the adapter reads them by tag, without a general XML parser.

/// The text of every `<tag>…</tag>` leaf of `xml`, in document order, with the five predefined
/// entities and numeric character references resolved. An element with attributes is matched
/// too; a self-closing one is not.
pub(super) fn texts(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        // `<KeyCount>` also starts with `<Key`: the name must end at `>` or whitespace.
        let Some(end) = after.find('>') else { break };
        let named = after[..end].chars().next().is_none_or(char::is_whitespace);
        if !named || after[..end].ends_with('/') {
            rest = &after[end..];
            continue;
        }
        let body = &after[end + 1..];
        let Some(stop) = body.find(&close) else { break };
        out.push(unescape(&body[..stop]));
        rest = &body[stop + close.len()..];
    }
    out
}

/// The text of the first `<tag>` leaf of `xml`.
pub(super) fn text(xml: &str, tag: &str) -> Option<String> {
    texts(xml, tag).into_iter().next()
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let entity = tail.find(';').map(|semi| (&tail[1..semi], semi));
        let resolved = entity.and_then(|(name, semi)| {
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => name
                    .strip_prefix("#x")
                    .map(|h| u32::from_str_radix(h, 16))
                    .or_else(|| name.strip_prefix('#').map(str::parse))
                    .and_then(Result::ok)
                    .and_then(char::from_u32),
            }?;
            Some((c, semi))
        });
        match resolved {
            Some((c, semi)) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}
