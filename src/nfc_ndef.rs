//! Pure NDEF (NFC Data Exchange Format) helpers — record parsing and
//! encoding with no hardware / embassy dependencies, so they are unit-
//! testable on the host. Consumed by [`crate::fw::nfct`].
//!
//! The parsers are deliberately minimal and tolerant: they walk a record
//! chain, skip records they don't care about, and bail (returning
//! `None` / `false`) on chunked (CF=1) or malformed input rather than
//! trying to recover.

/// Largest URL embedded in a generated URI record. Keeps the whole
/// record within the 127-byte NDEF ceiling the CC advertises.
pub const MAX_URL_LEN: usize = 118;

/// Find the first NDEF Well-Known text record in `msg` and return its
/// UTF-8 text bytes (language code stripped), or `None` if there is no
/// usable text record. Tolerates non-text records preceding the text
/// one; bails on chunked records (CF=1) and anything malformed.
pub fn find_text_record(msg: &[u8]) -> Option<&[u8]> {
    let mut i = 0;
    loop {
        let (hdr, type_bytes, payload, next) = parse_record(msg, i)?;
        i = next;
        if hdr.tnf == 0x01 && type_bytes == b"T" {
            // Text payload: [status][lang_code][utf8 text]
            if payload.is_empty() {
                if hdr.me {
                    return None;
                }
                continue;
            }
            let status = payload[0];
            if status & 0x80 != 0 {
                // UTF-16 — we don't decode it; skip.
                if hdr.me {
                    return None;
                }
                continue;
            }
            let lang_len = (status & 0x3F) as usize;
            if 1 + lang_len > payload.len() {
                if hdr.me {
                    return None;
                }
                continue;
            }
            return Some(&payload[1 + lang_len..]);
        }
        if hdr.me {
            return None;
        }
    }
}

/// True if `msg` contains a vCard record (TNF=MIME, type `text/vcard` or
/// `text/x-vcard`, case-insensitive).
pub fn is_vcard_message(msg: &[u8]) -> bool {
    let mut i = 0;
    loop {
        let Some((hdr, type_bytes, _payload, next)) = parse_record(msg, i) else {
            return false;
        };
        i = next;
        if hdr.tnf == 0x02
            && (type_bytes.eq_ignore_ascii_case(b"text/vcard")
                || type_bytes.eq_ignore_ascii_case(b"text/x-vcard"))
        {
            return true;
        }
        if hdr.me {
            return false;
        }
    }
}

/// True if `msg`'s first record is a Well-Known URI record (TNF=1,
/// type `U`) — i.e. a plain URL tag, the natural way to write a vanity
/// URL. Such a message is already a valid NDEF URI record, so callers
/// broadcast it verbatim.
pub fn is_uri_message(msg: &[u8]) -> bool {
    match parse_record(msg, 0) {
        Some((hdr, type_bytes, _payload, _next)) => hdr.tnf == 0x01 && type_bytes == b"U",
        None => false,
    }
}

/// Build a single URI NDEF record for `url` into `buf`, using the NFC
/// Forum URI abbreviation for a recognised scheme. Returns the number of
/// valid bytes written (`NLEN + message`). `url` is clamped to
/// [`MAX_URL_LEN`]. `buf` must be at least `7 + MAX_URL_LEN` bytes.
pub fn build_uri_record(buf: &mut [u8], url: &[u8]) -> usize {
    // Map a leading scheme to a URI-prefix abbreviation (NFC Forum URI
    // RTD, table 3). Fall through to 0x00 (no prefix) so anything else
    // still round-trips as the full literal string.
    let (prefix, rest): (u8, &[u8]) = if let Some(r) = url.strip_prefix(b"https://www.") {
        (0x02, r)
    } else if let Some(r) = url.strip_prefix(b"http://www.") {
        (0x01, r)
    } else if let Some(r) = url.strip_prefix(b"https://") {
        (0x04, r)
    } else if let Some(r) = url.strip_prefix(b"http://") {
        (0x03, r)
    } else {
        (0x00, url)
    };
    let rest = &rest[..rest.len().min(MAX_URL_LEN)];

    for b in buf.iter_mut() {
        *b = 0;
    }
    let payload_len = 1 + rest.len(); // prefix byte + url tail
    let msg_len = 4 + payload_len; // header + type-len + payload-len + type + payload
    buf[0] = (msg_len >> 8) as u8;
    buf[1] = msg_len as u8;
    buf[2] = 0xd1; // MB|ME|SR, TNF=Well-known
    buf[3] = 0x01; // type length
    buf[4] = payload_len as u8;
    buf[5] = 0x55; // 'U'
    buf[6] = prefix;
    buf[7..7 + rest.len()].copy_from_slice(rest);
    7 + rest.len()
}

/// Decoded record header flags we care about.
struct RecordHeader {
    me: bool,
    tnf: u8,
}

/// Parse one NDEF record starting at `msg[i]`. Returns the header flags,
/// the type bytes, the payload bytes, and the index of the next record,
/// or `None` on chunked / truncated / malformed input.
fn parse_record(msg: &[u8], mut i: usize) -> Option<(RecordHeader, &[u8], &[u8], usize)> {
    if i >= msg.len() {
        return None;
    }
    let header = msg[i];
    i += 1;
    let me = header & 0x40 != 0;
    let cf = header & 0x20 != 0;
    let sr = header & 0x10 != 0;
    let il = header & 0x08 != 0;
    let tnf = header & 0x07;
    if cf {
        return None;
    }

    let type_len = *msg.get(i)? as usize;
    i += 1;

    let payload_len = if sr {
        let pl = *msg.get(i)? as usize;
        i += 1;
        pl
    } else {
        if i + 4 > msg.len() {
            return None;
        }
        let pl = u32::from_be_bytes([msg[i], msg[i + 1], msg[i + 2], msg[i + 3]]) as usize;
        i += 4;
        pl
    };

    let id_len = if il {
        let b = *msg.get(i)? as usize;
        i += 1;
        b
    } else {
        0
    };

    if i + type_len > msg.len() {
        return None;
    }
    let type_bytes = &msg[i..i + type_len];
    i += type_len + id_len;

    if i + payload_len > msg.len() {
        return None;
    }
    let payload = &msg[i..i + payload_len];
    i += payload_len;

    Some((RecordHeader { me, tnf }, type_bytes, payload, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wrap a Well-Known text record (en) around `text`.
    fn text_record(text: &[u8]) -> std::vec::Vec<u8> {
        let lang = b"en";
        let payload_len = 1 + lang.len() + text.len();
        let mut v = std::vec![0xd1, 0x01, payload_len as u8, b'T', lang.len() as u8];
        v.extend_from_slice(lang);
        v.extend_from_slice(text);
        v
    }

    #[test]
    fn text_record_roundtrip() {
        let msg = text_record(b"token:borg-cube");
        assert_eq!(find_text_record(&msg), Some(&b"token:borg-cube"[..]));
    }

    #[test]
    fn set_prefix_detected_via_text() {
        let msg = text_record(b"set:https://me.example");
        let t = find_text_record(&msg).unwrap();
        assert_eq!(t.strip_prefix(b"set:".as_slice()), Some(&b"https://me.example"[..]));
    }

    #[test]
    fn vcard_mime_record_detected() {
        // TNF=0x02 (MIME), type "text/vcard", short-record.
        let ty = b"text/vcard";
        let body = b"BEGIN:VCARD\r\nEND:VCARD\r\n";
        let hdr = 0xc0 | 0x10 | 0x02; // MB|ME|SR, TNF=MIME
        let mut msg = std::vec![hdr, ty.len() as u8, body.len() as u8];
        msg.extend_from_slice(ty);
        msg.extend_from_slice(body);
        assert!(is_vcard_message(&msg));
        // A plain text record is not a vCard.
        assert!(!is_vcard_message(&text_record(b"hello")));
    }

    #[test]
    fn vcard_type_case_insensitive() {
        let ty = b"text/x-vCard";
        let body = b"BEGIN:VCARD";
        let hdr = 0xc0 | 0x10 | 0x02;
        let mut msg = std::vec![hdr, ty.len() as u8, body.len() as u8];
        msg.extend_from_slice(ty);
        msg.extend_from_slice(body);
        assert!(is_vcard_message(&msg));
    }

    #[test]
    fn uri_record_detected_as_profile() {
        // A plain URL tag: d1 01 0c 55 04 "annejan.com" (what a phone writes).
        let msg = b"\xd1\x01\x0c\x55\x04annejan.com";
        assert!(is_uri_message(msg));
        // A text record is not a URI record.
        assert!(!is_uri_message(&text_record(b"token:x")));
        // A generated URI record round-trips as a URI record too.
        let mut buf = [0u8; 256];
        let n = build_uri_record(&mut buf, b"https://me.example");
        let nlen = u16::from_be_bytes([buf[0], buf[1]]) as usize;
        assert_eq!(n, 2 + nlen);
        assert!(is_uri_message(&buf[2..2 + nlen]));
    }

    #[test]
    fn build_uri_abbreviates_scheme() {
        let mut buf = [0u8; 256];
        let n = build_uri_record(&mut buf, b"https://me.example");
        // NLEN header + record.
        let nlen = u16::from_be_bytes([buf[0], buf[1]]) as usize;
        assert_eq!(n, 2 + nlen);
        assert_eq!(buf[2], 0xd1); // record header
        assert_eq!(buf[5], 0x55); // 'U'
        assert_eq!(buf[6], 0x04); // https:// abbreviation
        assert_eq!(&buf[7..7 + "me.example".len()], b"me.example");
        // The generated record must parse back as... not text (it's a URI),
        // but is_vcard must be false.
        assert!(!is_vcard_message(&buf[2..2 + nlen]));
    }

    #[test]
    fn build_uri_unknown_scheme_keeps_literal() {
        let mut buf = [0u8; 256];
        build_uri_record(&mut buf, b"mailto:me@example.com");
        assert_eq!(buf[6], 0x00); // no abbreviation
        assert_eq!(&buf[7..7 + "mailto:me@example.com".len()], b"mailto:me@example.com");
    }

    #[test]
    fn build_uri_clamps_long_url() {
        let mut buf = [0u8; 256];
        let long = std::vec![b'a'; 500];
        let mut url = std::vec::Vec::from(&b"https://"[..]);
        url.extend_from_slice(&long);
        build_uri_record(&mut buf, &url);
        let payload_len = buf[4] as usize;
        // payload = 1 prefix byte + clamped url
        assert_eq!(payload_len, 1 + MAX_URL_LEN);
    }

    #[test]
    fn malformed_and_chunked_rejected() {
        assert_eq!(find_text_record(&[]), None);
        assert_eq!(find_text_record(&[0xff]), None); // truncated
        // CF=1 (chunked) record header → bail.
        assert!(!is_vcard_message(&[0x20, 0x00, 0x00]));
    }
}
