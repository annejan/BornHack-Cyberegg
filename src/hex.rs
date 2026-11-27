//! Hex nibble encode / decode helpers shared by the persisted-key builders,
//! the device-ID formatter, the PCX file-name builder and the LUT/PCX
//! parsers. Pure, `no_std`.

/// Lowercase ASCII hex digit for the low 4 bits of `n`.
pub const fn nibble_lower(n: u8) -> u8 {
    let n = n & 0xF;
    if n < 10 { b'0' + n } else { b'a' + n - 10 }
}

/// Uppercase ASCII hex digit for the low 4 bits of `n`.
pub const fn nibble_upper(n: u8) -> u8 {
    let n = n & 0xF;
    if n < 10 { b'0' + n } else { b'A' + n - 10 }
}

/// Two uppercase ASCII hex digits for `b`, high nibble first.
pub const fn byte_upper(b: u8) -> [u8; 2] {
    [nibble_upper(b >> 4), nibble_upper(b)]
}

/// Append the lowercase hex of `bytes` to `out`. Stops silently when `out`
/// is full (same as the `let _ = push(..)` copies this replaced).
pub fn push_lower<const N: usize>(out: &mut heapless::String<N>, bytes: &[u8]) {
    for &b in bytes {
        let _ = out.push(nibble_lower(b >> 4) as char);
        let _ = out.push(nibble_lower(b) as char);
    }
}

/// Value of one ASCII hex digit (either case).
pub const fn val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Decode two ASCII hex digits (either case) into one byte.
pub fn pair(hi: u8, lo: u8) -> Option<u8> {
    Some((val(hi)? << 4) | val(lo)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_lower_pins_persisted_key_format() {
        let mut s: heapless::String<8> = heapless::String::new();
        push_lower(&mut s, &[0x0a, 0xf0]);
        assert_eq!(s.as_str(), "0af0");
        let mut s: heapless::String<2> = heapless::String::new();
        push_lower(&mut s, &[0xB7]);
        assert_eq!(s.as_str(), "b7");
    }

    #[test]
    fn push_lower_truncates_when_full() {
        let mut s: heapless::String<3> = heapless::String::new();
        push_lower(&mut s, &[0x12, 0x34]);
        assert_eq!(s.as_str(), "123");
    }

    #[test]
    fn upper_encode() {
        assert_eq!(byte_upper(0xA3), *b"A3");
        assert_eq!(byte_upper(0x0F), *b"0F");
        assert_eq!(nibble_upper(0x1B), b'B');
        assert_eq!(nibble_lower(0xB), b'b');
    }

    #[test]
    fn decode_both_cases_and_rejects() {
        assert_eq!(val(b'0'), Some(0));
        assert_eq!(val(b'f'), Some(15));
        assert_eq!(val(b'F'), Some(15));
        assert_eq!(val(b'g'), None);
        assert_eq!(val(b' '), None);
        assert_eq!(pair(b'A', b'3'), Some(0xA3));
        assert_eq!(pair(b'a', b'3'), Some(0xA3));
        assert_eq!(pair(b'a', b'x'), None);
    }

    #[test]
    fn roundtrip_all_bytes() {
        for b in 0..=255u8 {
            let u = byte_upper(b);
            assert_eq!(pair(u[0], u[1]), Some(b));
            let mut s: heapless::String<2> = heapless::String::new();
            push_lower(&mut s, &[b]);
            let l = s.as_bytes();
            assert_eq!(pair(l[0], l[1]), Some(b));
        }
    }
}
