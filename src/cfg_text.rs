//! Line helpers shared by the `KEY=VALUE` text-file parsers (`PETS.CFG`,
//! `BORNPETS.CFG`, `LUT.CFG`). Pure, `no_std`.

/// Split one line into `(key, value)`, or `None` for comments, blanks and
/// lines without `=`. Both halves are trimmed of ASCII whitespace (which
/// also removes a trailing `\r`); either may still be empty.
///
/// `inline_comments`: `true` strips everything from the first `#` (so
/// `A=1 # note` gives `1`); `false` treats `#` as a comment only as the
/// first non-blank character of the line.
pub fn kv_of_line(line: &[u8], inline_comments: bool) -> Option<(&[u8], &[u8])> {
    let mut line = line.trim_ascii();
    if inline_comments {
        if let Some(i) = line.iter().position(|&b| b == b'#') {
            line = line[..i].trim_ascii();
        }
    } else if line.first() == Some(&b'#') {
        return None;
    }
    if line.is_empty() {
        return None;
    }
    let eq = line.iter().position(|&b| b == b'=')?;
    Some((line[..eq].trim_ascii(), line[eq + 1..].trim_ascii()))
}

/// Every `\n`-separated line of `data` that parses with [`kv_of_line`] and
/// has both a non-empty key and a non-empty value.
pub fn kv_lines(data: &[u8], inline_comments: bool) -> impl Iterator<Item = (&[u8], &[u8])> {
    data.split(|&b| b == b'\n')
        .filter_map(move |l| kv_of_line(l, inline_comments))
        .filter(|(k, v)| !k.is_empty() && !v.is_empty())
}

/// Reassembles text lines from arbitrary byte chunks (a file read piece by
/// piece), so a file of any size streams through `N` bytes of state.
///
/// A line longer than `N` is dropped whole, never delivered truncated, so
/// a cut-off value can't be applied by mistake.
pub struct LineAssembler<const N: usize> {
    buf: [u8; N],
    len: usize,
    skipping: bool,
}

impl<const N: usize> LineAssembler<N> {
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
            skipping: false,
        }
    }

    /// Feed the next chunk; `f` runs once per completed line (no `\n`).
    pub fn push(&mut self, chunk: &[u8], f: &mut impl FnMut(&[u8])) {
        for &b in chunk {
            if b == b'\n' {
                if !self.skipping {
                    f(&self.buf[..self.len]);
                }
                self.len = 0;
                self.skipping = false;
            } else if self.skipping {
                // Rest of an over-long line: dropped.
            } else if self.len == N {
                self.skipping = true;
                self.len = 0;
            } else {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }

    /// End of file: flush a last line that has no trailing newline.
    pub fn finish(&mut self, f: &mut impl FnMut(&[u8])) {
        if !self.skipping && self.len > 0 {
            f(&self.buf[..self.len]);
        }
        self.len = 0;
        self.skipping = false;
    }
}

impl<const N: usize> Default for LineAssembler<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a non-empty ASCII decimal into a `u32`, or `None` on any
/// non-digit / empty / overflow.
pub fn parse_dec_u32(s: &[u8]) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    for &c in s {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - b'0') as u32)?;
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_basic_and_whitespace() {
        assert_eq!(
            kv_of_line(b"  A = b c \r", true),
            Some((&b"A"[..], &b"b c"[..]))
        );
        assert_eq!(kv_of_line(b"A=1", false), Some((&b"A"[..], &b"1"[..])));
    }

    #[test]
    fn kv_comment_rules() {
        assert_eq!(kv_of_line(b"# A=1", true), None);
        assert_eq!(kv_of_line(b"# A=1", false), None);
        assert_eq!(kv_of_line(b"  # A=1", false), None);
        assert_eq!(kv_of_line(b"A=1 # n", true), Some((&b"A"[..], &b"1"[..])));
        assert_eq!(
            kv_of_line(b"A=1 # n", false),
            Some((&b"A"[..], &b"1 # n"[..]))
        );
    }

    #[test]
    fn kv_no_eq_or_blank() {
        assert_eq!(kv_of_line(b"", true), None);
        assert_eq!(kv_of_line(b"   \r", false), None);
        assert_eq!(kv_of_line(b"novalue", true), None);
    }

    #[test]
    fn kv_lines_skips_empty_halves() {
        let data = b"# c\nA=1\n=x\nB=\n\r\n C = 3 \r\nbad\n";
        let got: std::vec::Vec<_> = kv_lines(data, true).collect();
        assert_eq!(got, [(&b"A"[..], &b"1"[..]), (&b"C"[..], &b"3"[..])]);
    }

    #[test]
    fn dec_parse() {
        assert_eq!(parse_dec_u32(b"0"), Some(0));
        assert_eq!(parse_dec_u32(b"4294967295"), Some(u32::MAX));
        assert_eq!(parse_dec_u32(b"4294967296"), None);
        assert_eq!(parse_dec_u32(b""), None);
        assert_eq!(parse_dec_u32(b"1a"), None);
        assert_eq!(parse_dec_u32(b"-1"), None);
    }
}
