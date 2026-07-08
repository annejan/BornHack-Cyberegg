//! Parser for a user-supplied `LUT.CFG` — a custom e-paper waveform LUT
//! dropped onto the USB-MSC FAT partition, in the same `KEY=VALUE`
//! per-line style as `PETS.CFG` / `BORNPETS.CFG` (`#` starts a comment).
//!
//! The badge probes its real waveform LUT from panel OTP at boot; this
//! lets a calibrated waveform (from the ssd1675-calibration tool) replace
//! it without a reflash. Only two keys are read — everything else is
//! ignored, so the calibration tool's richer export can be trimmed down
//! by hand:
//!
//! ```text
//! # CyberAegg EPD LUT
//! variant=A                 # A = SSD1675/SSD1675A, B = SSD1675B
//! band_lut=08992144...      # 214 hex chars = one 107-byte LUT unit
//! ```
//!
//! `variant` MUST match the panel or the caller rejects the file — an
//! A-panel LUT on a B panel (or vice-versa) uses the wrong row layout /
//! drive voltages and can blank or stress the display.
//!
//! The 107 bytes are a full register-0x33 OTP-image LUT unit: the
//! waveform body plus the trailer timing/voltage bytes the driver reads
//! (bytes 70..=75 on A, 100..=106 on B). The separate voltage `controls`
//! the calibration tool emits are already baked into these bytes, so we
//! don't need them here.
//!
//! Pure / `no_std`, no hardware deps, so it is unit-testable on the host.

/// Length of one LUT unit (the register-0x33 OTP readback image).
pub const LUT_UNIT_LEN: usize = 107;

/// Panel variant a LUT is built for. Mirrors the driver's
/// `DisplayVariant` without depending on the (embassy-only) driver crate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LutVariant {
    /// SSD1675 / SSD1675A — 7-byte rows, trailer at 70..=75.
    A,
    /// SSD1675B — 10-byte rows, trailer at 100..=106.
    B,
}

/// A validated custom LUT parsed from `LUT.CFG`.
#[derive(Debug)]
pub struct ParsedLut {
    pub variant: LutVariant,
    pub lut: [u8; LUT_UNIT_LEN],
}

/// Why a `LUT.CFG` was rejected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LutCfgError {
    /// No `variant=` line, or the value wasn't `A`/`B`.
    MissingOrBadVariant,
    /// No `band_lut=` line.
    MissingBandLut,
    /// `band_lut` had a non-hex char or an odd digit count.
    BadHex,
    /// `band_lut` decoded to something other than [`LUT_UNIT_LEN`] bytes.
    WrongLen,
}

/// Parse `LUT.CFG` bytes into a validated [`ParsedLut`]. Does NOT check
/// the variant against the live panel — the caller does that against the
/// probed `DisplayVariant`.
pub fn parse_lut_cfg(data: &[u8]) -> Result<ParsedLut, LutCfgError> {
    let mut variant: Option<LutVariant> = None;
    let mut lut: Option<[u8; LUT_UNIT_LEN]> = None;

    for line in data.split(|&b| b == b'\n') {
        let line = trim(line);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let Some(eq) = line.iter().position(|&b| b == b'=') else {
            continue;
        };
        let key = trim(&line[..eq]);
        let val = trim(&line[eq + 1..]);

        if key.eq_ignore_ascii_case(b"variant") {
            variant = match val {
                [b'A' | b'a'] => Some(LutVariant::A),
                [b'B' | b'b'] => Some(LutVariant::B),
                _ => return Err(LutCfgError::MissingOrBadVariant),
            };
        } else if key.eq_ignore_ascii_case(b"band_lut") {
            let mut out = [0u8; LUT_UNIT_LEN];
            match decode_hex(val, &mut out) {
                Ok(LUT_UNIT_LEN) => lut = Some(out),
                Ok(_) => return Err(LutCfgError::WrongLen),
                Err(e) => return Err(e),
            }
        }
    }

    Ok(ParsedLut {
        variant: variant.ok_or(LutCfgError::MissingOrBadVariant)?,
        lut: lut.ok_or(LutCfgError::MissingBandLut)?,
    })
}

/// Decode ASCII hex `src` into `dst`. Returns the number of bytes
/// written. Errors on odd length, overflow, or non-hex digits.
fn decode_hex(src: &[u8], dst: &mut [u8]) -> Result<usize, LutCfgError> {
    if src.len() % 2 != 0 {
        return Err(LutCfgError::BadHex);
    }
    let n = src.len() / 2;
    if n > dst.len() {
        return Err(LutCfgError::WrongLen);
    }
    for i in 0..n {
        let hi = hex_val(src[2 * i]).ok_or(LutCfgError::BadHex)?;
        let lo = hex_val(src[2 * i + 1]).ok_or(LutCfgError::BadHex)?;
        dst[i] = (hi << 4) | lo;
    }
    Ok(n)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Trim leading/trailing ASCII whitespace (incl. a trailing `\r`).
fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if first.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = s {
        if last.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 107-byte LUT rendered as 214 hex chars (bytes 0..107 = 0,1,2,...).
    fn sample_hex() -> std::string::String {
        (0..LUT_UNIT_LEN).map(|i| format!("{:02x}", i as u8)).collect()
    }

    #[test]
    fn parses_variant_and_lut() {
        let cfg = format!("# comment\nvariant=A\nband_lut={}\n", sample_hex());
        let p = parse_lut_cfg(cfg.as_bytes()).unwrap();
        assert_eq!(p.variant, LutVariant::A);
        assert_eq!(p.lut[0], 0);
        assert_eq!(p.lut[106], 106);
    }

    #[test]
    fn variant_b_and_case_insensitive_keys() {
        let cfg = format!("VARIANT=b\r\nBAND_LUT={}\r\n", sample_hex());
        let p = parse_lut_cfg(cfg.as_bytes()).unwrap();
        assert_eq!(p.variant, LutVariant::B);
    }

    #[test]
    fn crlf_and_blank_and_comment_lines_ok() {
        let cfg = format!("\n\n#hi\n   variant = A  \n\n band_lut = {} \n", sample_hex());
        assert!(parse_lut_cfg(cfg.as_bytes()).is_ok());
    }

    #[test]
    fn rejects_wrong_length() {
        let cfg = "variant=A\nband_lut=00112233\n";
        assert_eq!(parse_lut_cfg(cfg.as_bytes()).unwrap_err(), LutCfgError::WrongLen);
    }

    #[test]
    fn rejects_bad_hex() {
        let cfg = format!("variant=A\nband_lut={}\n", "zz".repeat(LUT_UNIT_LEN));
        assert_eq!(parse_lut_cfg(cfg.as_bytes()).unwrap_err(), LutCfgError::BadHex);
    }

    #[test]
    fn rejects_missing_fields() {
        let only_lut = format!("band_lut={}\n", sample_hex());
        assert_eq!(
            parse_lut_cfg(only_lut.as_bytes()).unwrap_err(),
            LutCfgError::MissingOrBadVariant
        );
        assert_eq!(
            parse_lut_cfg(b"variant=A\n").unwrap_err(),
            LutCfgError::MissingBandLut
        );
    }

    #[test]
    fn rejects_bad_variant_value() {
        let cfg = format!("variant=C\nband_lut={}\n", sample_hex());
        assert_eq!(
            parse_lut_cfg(cfg.as_bytes()).unwrap_err(),
            LutCfgError::MissingOrBadVariant
        );
    }
}
