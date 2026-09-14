//! Reading what a browser posts.
//!
//! Two encodings, and only two, because a page with no script can only send
//! two. `application/x-www-form-urlencoded` is a query string that arrived in a
//! body, so `wgvb-view` already parses it. `multipart/form-data` is what a file
//! picker sends, and there is no parser for it in the tree — this is one,
//! written to the one shape that matters here rather than to the whole of
//! RFC 7578: find the boundary, walk the parts, take the first one that carries
//! a filename.
//!
//! A `<textarea>` sits beside the file picker on the page for exactly this
//! reason. Pasting a configuration goes through the encoding that was already
//! understood, and is often faster than a file dialog anyway.

/// Every field of an `application/x-www-form-urlencoded` body.
#[must_use]
pub fn fields(body: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(body);
    wgvb_view::pairs(&text).collect()
}

/// The first uploaded file in a `multipart/form-data` body, as text.
///
/// `None` if the content type carries no boundary, if no part has a filename,
/// or if the part is not UTF-8 — all of which are "there is no configuration
/// here", which is the only distinction the caller needs.
#[must_use]
pub fn uploaded_file(content_type: &str, body: &[u8]) -> Option<String> {
    let boundary = boundary_of(content_type)?;
    let separator = format!("--{boundary}").into_bytes();

    for part in split(body, &separator) {
        // Headers and content are separated by a blank line. Anything before
        // the first blank line is headers, whatever their case.
        let split_at = find(part, b"\r\n\r\n")?;
        let (headers, content) = part.split_at(split_at);
        let headers = String::from_utf8_lossy(headers).to_ascii_lowercase();
        if !headers.contains("filename=") {
            continue;
        }
        let content = &content[4..];
        // A part's content is terminated by the CRLF that introduces the next
        // boundary, and that CRLF belongs to the boundary rather than to the
        // file.
        let content = content.strip_suffix(b"\r\n").unwrap_or(content);
        return String::from_utf8(content.to_vec()).ok();
    }
    None
}

/// The boundary a multipart content type declares.
fn boundary_of(content_type: &str) -> Option<String> {
    let lower = content_type.to_ascii_lowercase();
    if !lower.starts_with("multipart/form-data") {
        return None;
    }
    let at = lower.find("boundary=")? + "boundary=".len();
    let rest = content_type[at..].trim();
    let rest = rest.split(';').next().unwrap_or(rest).trim();
    let rest = rest.trim_matches('"');
    (!rest.is_empty()).then(|| rest.to_string())
}

/// The parts of a multipart body, without the preamble or the epilogue.
fn split<'a>(body: &'a [u8], separator: &[u8]) -> Vec<&'a [u8]> {
    let mut parts = Vec::new();
    let mut rest = body;
    // Everything before the first boundary is preamble and is dropped.
    let Some(first) = find(rest, separator) else {
        return parts;
    };
    rest = &rest[first + separator.len()..];

    while let Some(next) = find(rest, separator) {
        let part = &rest[..next];
        // Each boundary is preceded by a CRLF and followed by one; the leading
        // CRLF of this part is the tail of the previous boundary line.
        let part = part.strip_prefix(b"\r\n").unwrap_or(part);
        parts.push(part);
        rest = &rest[next + separator.len()..];
    }
    parts
}

/// Where one byte string first occurs in another.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&at| &haystack[at..at + needle.len()] == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body shaped the way a browser actually sends one, CRLFs included.
    fn multipart(boundary: &str, filename: &str, content: &str) -> Vec<u8> {
        format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n\
             Content-Type: application/octet-stream\r\n\
             \r\n\
             {content}\r\n\
             --{boundary}--\r\n"
        )
        .into_bytes()
    }

    #[test]
    fn a_urlencoded_body_is_a_query_string_in_a_body() {
        let fields = fields(b"sea_level=0.05&local_wavelength_miles=180");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0], ("sea_level".to_string(), "0.05".to_string()));
        assert_eq!(
            fields[1],
            ("local_wavelength_miles".to_string(), "180".to_string())
        );
    }

    #[test]
    fn a_uploaded_file_comes_back_verbatim() {
        let body = multipart("----abc123", "world.toml", "sea_level = 0.05\nfoo = 1\n");
        let text =
            uploaded_file("multipart/form-data; boundary=----abc123", &body).expect("a file part");
        assert_eq!(text, "sea_level = 0.05\nfoo = 1\n");
    }

    #[test]
    fn a_quoted_boundary_and_a_trailing_parameter_are_both_read() {
        let body = multipart("xyz", "a.toml", "sea_level = 0.0");
        assert_eq!(
            uploaded_file(
                "multipart/form-data; boundary=\"xyz\"; charset=utf-8",
                &body
            ),
            Some("sea_level = 0.0".to_string())
        );
    }

    #[test]
    fn a_part_without_a_filename_is_not_a_file() {
        let body =
            b"--xyz\r\nContent-Disposition: form-data; name=\"seed\"\r\n\r\n7\r\n--xyz--\r\n";
        assert_eq!(
            uploaded_file("multipart/form-data; boundary=xyz", body),
            None
        );
    }

    #[test]
    fn nothing_is_read_out_of_a_body_with_no_boundary() {
        assert_eq!(uploaded_file("multipart/form-data", b"whatever"), None);
        assert_eq!(uploaded_file("text/plain", b"whatever"), None);
    }

    #[test]
    fn an_empty_upload_is_an_empty_file_rather_than_a_panic() {
        let body = multipart("xyz", "", "");
        // A file picker submitted with nothing chosen still sends a part, with
        // an empty filename and no content. It must not be mistaken for a
        // configuration, but it must also not crash a worker.
        assert_eq!(
            uploaded_file("multipart/form-data; boundary=xyz", &body),
            Some(String::new())
        );
    }
}
