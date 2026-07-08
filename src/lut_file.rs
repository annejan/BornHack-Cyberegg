//! Parser for a user-supplied `LUT.CFG` — a custom e-paper waveform LUT
//! dropped onto the USB-MSC FAT partition, in the same `KEY=VALUE`
//! per-line style as `PETS.CFG` / `BORNPETS.CFG` (`#` starts a comment).
//!
//! The badge probes its real waveform LUT from panel OTP at boot; this
//! lets a calibrated waveform (from the ssd1675-calibration tool) replace
//! it without a reflash. Recognised keys:
//!
//! ```text
//! # CyberAegg EPD LUT
//! variant=A                 # A = SSD1675/SSD1675A, B = SSD1675B (required)
//! band_lut=08992144...      # 214 hex chars = one 107-byte LUT unit,
//!                           #   applied to every temperature band
//! band_lut_07=...           # optional: override a single band (00..15),
//!                           #   for a temperature-compensated set
//! speed=100                 # optional: LUT cycle-duration scale (EPD_LUT_SPEED)
//! ```
//!
//! `variant` MUST match the panel or the caller rejects the file — an
//! A-panel LUT on a B panel (or vice-versa) uses the wrong row layout /
//! drive voltages and can blank or stress the display.
//!
//! Band model: `band_lut` sets a base waveform for all 16 temperature
//! bands; `band_lut_NN` overrides band `NN` (0..15). Bands left unset by
//! both keep the OTP-probed waveform (so a partial set still tracks
//! temperature via the panel's own LUTs). The 107 bytes are a full
//! register-0x33 OTP-image LUT unit (waveform body + the trailer
//! timing/voltage bytes the driver reads at 70..=75 on A / 100..=106 on
//! B), so the calibration tool's separate voltage `controls` are already
//! baked in and not needed here.
//!
//! Pure / `no_std`, no hardware deps, so it is unit-testable on the host.

/// Length of one LUT unit (the register-0x33 OTP readback image).
pub const LUT_UNIT_LEN: usize = 107;
/// Number of temperature bands the driver's LUT table holds.
pub const NUM_BANDS: usize = 16;

/// Panel variant a LUT is built for. Mirrors the driver's
/// `DisplayVariant` without depending on the (embassy-only) driver crate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LutVariant {
    /// SSD1675 / SSD1675A — 7-byte rows, trailer at 70..=75.
    A,
    /// SSD1675B — 10-byte rows, trailer at 100..=106.
    B,
}

/// Non-waveform metadata: read cheaply first so the caller can validate
/// `variant` against the live panel *before* touching the band table.
#[derive(Clone, Copy, Debug)]
pub struct LutMeta {
    pub variant: LutVariant,
    /// LUT cycle-duration scale override, if the file set `speed=`.
    pub speed: Option<u8>,
}

/// Why a `LUT.CFG` was rejected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LutCfgError {
    /// No `variant=` line, or the value wasn't `A`/`B`.
    MissingOrBadVariant,
    /// A `*_lut` value had a non-hex char or an odd digit count.
    BadHex,
    /// A `*_lut` value decoded to something other than [`LUT_UNIT_LEN`] bytes.
    WrongLen,
    /// `band_lut_NN` with `NN` outside `0..=15`.
    BadBandIndex,
    /// `speed=` was not a decimal in `0..=255`.
    BadSpeed,
}

/// Pass 1 — parse `variant` (required) and `speed` (optional), ignoring
/// waveform data. Cheap; lets the caller reject a variant mismatch before
/// filling the band table.
pub fn parse_meta(data: &[u8]) -> Result<LutMeta, LutCfgError> {
    let mut variant: Option<LutVariant> = None;
    let mut speed: Option<u8> = None;
    for_each_kv(data, |key, val| {
        if key.eq_ignore_ascii_case(b"variant") {
            variant = Some(parse_variant(val)?);
        } else if key.eq_ignore_ascii_case(b"speed") {
            speed = Some(parse_dec(val).and_then(u8_in_range).ok_or(LutCfgError::BadSpeed)?);
        }
        Ok(())
    })?;
    Ok(LutMeta {
        variant: variant.ok_or(LutCfgError::MissingOrBadVariant)?,
        speed,
    })
}

/// Pass 2 — fill `bands` from the file's waveform keys.
///
/// `band_lut` sets a base applied to every band; `band_lut_NN` overrides
/// band `NN`. Bands set by neither are left untouched (caller keeps the
/// OTP value there) and their `band_set[i]` stays `false`. Returns `true`
/// if any band was written.
pub fn parse_bands(
    data: &[u8],
    bands: &mut [[u8; LUT_UNIT_LEN]; NUM_BANDS],
    band_set: &mut [bool; NUM_BANDS],
) -> Result<bool, LutCfgError> {
    let mut base: Option<[u8; LUT_UNIT_LEN]> = None;
    for_each_kv(data, |key, val| {
        if key.eq_ignore_ascii_case(b"band_lut") {
            let mut b = [0u8; LUT_UNIT_LEN];
            decode_exact(val, &mut b)?;
            base = Some(b);
        } else if let Some(suffix) = strip_prefix_ci(key, b"band_lut_") {
            let idx = parse_dec(suffix)
                .filter(|&n| (n as usize) < NUM_BANDS)
                .ok_or(LutCfgError::BadBandIndex)? as usize;
            decode_exact(val, &mut bands[idx])?;
            band_set[idx] = true;
        }
        Ok(())
    })?;
    // Base fills any band not explicitly overridden.
    if let Some(b) = base {
        for i in 0..NUM_BANDS {
            if !band_set[i] {
                bands[i] = b;
                band_set[i] = true;
            }
        }
    }
    Ok(band_set.iter().any(|&s| s))
}

/// Dry-run of [`parse_bands`]: validate every waveform value (hex + exact
/// length) and every band index, writing nowhere. Lets the caller reject a
/// malformed file *before* it mutates the live LUT table, so no scratch
/// table is needed. Each value decodes into a small reused stack local.
pub fn validate_bands(data: &[u8]) -> Result<(), LutCfgError> {
    for_each_kv(data, |key, val| {
        if key.eq_ignore_ascii_case(b"band_lut") {
            let mut b = [0u8; LUT_UNIT_LEN];
            decode_exact(val, &mut b)?;
        } else if let Some(suffix) = strip_prefix_ci(key, b"band_lut_") {
            parse_dec(suffix)
                .filter(|&n| (n as usize) < NUM_BANDS)
                .ok_or(LutCfgError::BadBandIndex)?;
            let mut b = [0u8; LUT_UNIT_LEN];
            decode_exact(val, &mut b)?;
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Call `f(key, val)` for each non-comment `KEY=VALUE` line.
fn for_each_kv<F>(data: &[u8], mut f: F) -> Result<(), LutCfgError>
where
    F: FnMut(&[u8], &[u8]) -> Result<(), LutCfgError>,
{
    for line in data.split(|&b| b == b'\n') {
        let line = trim(line);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let Some(eq) = line.iter().position(|&b| b == b'=') else {
            continue;
        };
        f(trim(&line[..eq]), trim(&line[eq + 1..]))?;
    }
    Ok(())
}

fn parse_variant(val: &[u8]) -> Result<LutVariant, LutCfgError> {
    match val {
        [b'A' | b'a'] => Ok(LutVariant::A),
        [b'B' | b'b'] => Ok(LutVariant::B),
        _ => Err(LutCfgError::MissingOrBadVariant),
    }
}

/// Decode ASCII hex `src` into `dst`, requiring exactly `dst.len()` bytes.
fn decode_exact(src: &[u8], dst: &mut [u8]) -> Result<(), LutCfgError> {
    if src.len() % 2 != 0 {
        return Err(LutCfgError::BadHex);
    }
    if src.len() / 2 != dst.len() {
        return Err(LutCfgError::WrongLen);
    }
    for (i, out) in dst.iter_mut().enumerate() {
        let hi = hex_val(src[2 * i]).ok_or(LutCfgError::BadHex)?;
        let lo = hex_val(src[2 * i + 1]).ok_or(LutCfgError::BadHex)?;
        *out = (hi << 4) | lo;
    }
    Ok(())
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Parse a non-empty ASCII decimal into a `u32`, or `None` on any
/// non-digit / empty / overflow.
fn parse_dec(s: &[u8]) -> Option<u32> {
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

fn u8_in_range(v: u32) -> Option<u8> {
    (v <= 255).then_some(v as u8)
}

/// Case-insensitive `strip_prefix` for byte slices.
fn strip_prefix_ci<'a>(s: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
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

    // A 107-byte LUT as 214 hex chars, first byte = `tag` so we can tell
    // bands apart in assertions.
    fn hex_tagged(tag: u8) -> std::string::String {
        let mut s = format!("{:02x}", tag);
        s.push_str(&(1..LUT_UNIT_LEN).map(|_| "00").collect::<std::string::String>());
        s
    }

    fn empty_bands() -> ([[u8; LUT_UNIT_LEN]; NUM_BANDS], [bool; NUM_BANDS]) {
        ([[0u8; LUT_UNIT_LEN]; NUM_BANDS], [false; NUM_BANDS])
    }

    #[test]
    fn meta_variant_and_speed() {
        let m = parse_meta(b"# c\nvariant=A\nspeed=120\n").unwrap();
        assert_eq!(m.variant, LutVariant::A);
        assert_eq!(m.speed, Some(120));
        // speed optional
        let m = parse_meta(b"variant=b\n").unwrap();
        assert_eq!(m.variant, LutVariant::B);
        assert_eq!(m.speed, None);
    }

    #[test]
    fn meta_requires_variant() {
        assert_eq!(
            parse_meta(b"speed=100\n").unwrap_err(),
            LutCfgError::MissingOrBadVariant
        );
    }

    #[test]
    fn meta_bad_speed() {
        assert_eq!(
            parse_meta(b"variant=A\nspeed=999\n").unwrap_err(),
            LutCfgError::BadSpeed
        );
        assert_eq!(
            parse_meta(b"variant=A\nspeed=x\n").unwrap_err(),
            LutCfgError::BadSpeed
        );
    }

    #[test]
    fn base_band_fills_all() {
        let cfg = format!("variant=A\nband_lut={}\n", hex_tagged(0xAB));
        let (mut bands, mut set) = empty_bands();
        assert!(parse_bands(cfg.as_bytes(), &mut bands, &mut set).unwrap());
        assert!(set.iter().all(|&s| s));
        assert!(bands.iter().all(|b| b[0] == 0xAB));
    }

    #[test]
    fn per_band_override_wins_over_base() {
        let cfg = format!(
            "variant=A\nband_lut={}\nband_lut_07={}\n",
            hex_tagged(0x11),
            hex_tagged(0x77)
        );
        let (mut bands, mut set) = empty_bands();
        parse_bands(cfg.as_bytes(), &mut bands, &mut set).unwrap();
        assert!(set.iter().all(|&s| s));
        assert_eq!(bands[7][0], 0x77);
        assert_eq!(bands[0][0], 0x11);
        assert_eq!(bands[15][0], 0x11);
    }

    #[test]
    fn per_band_only_leaves_others_unset() {
        let cfg = format!("variant=A\nband_lut_00={}\n", hex_tagged(0x42));
        let (mut bands, mut set) = empty_bands();
        assert!(parse_bands(cfg.as_bytes(), &mut bands, &mut set).unwrap());
        assert!(set[0]);
        assert_eq!(bands[0][0], 0x42);
        assert!(!set[1]); // untouched → caller keeps OTP there
    }

    #[test]
    fn no_waveform_returns_false() {
        let (mut bands, mut set) = empty_bands();
        assert!(!parse_bands(b"variant=A\nspeed=100\n", &mut bands, &mut set).unwrap());
    }

    #[test]
    fn rejects_bad_band_index() {
        let cfg = format!("variant=A\nband_lut_16={}\n", hex_tagged(1));
        let (mut bands, mut set) = empty_bands();
        assert_eq!(
            parse_bands(cfg.as_bytes(), &mut bands, &mut set).unwrap_err(),
            LutCfgError::BadBandIndex
        );
    }

    #[test]
    fn rejects_wrong_len_and_bad_hex() {
        let (mut bands, mut set) = empty_bands();
        assert_eq!(
            parse_bands(b"variant=A\nband_lut=00112233\n", &mut bands, &mut set).unwrap_err(),
            LutCfgError::WrongLen
        );
        let cfg = format!("variant=A\nband_lut={}\n", "zz".repeat(LUT_UNIT_LEN));
        assert_eq!(
            parse_bands(cfg.as_bytes(), &mut bands, &mut set).unwrap_err(),
            LutCfgError::BadHex
        );
    }

    #[test]
    fn validate_matches_parse() {
        let good = format!("variant=A\nband_lut={}\nband_lut_03={}\n", hex_tagged(1), hex_tagged(2));
        assert!(validate_bands(good.as_bytes()).is_ok());
        // Bad hex, wrong length, and bad index all caught without writing.
        let bad_hex = format!("variant=A\nband_lut={}\n", "zz".repeat(LUT_UNIT_LEN));
        assert_eq!(validate_bands(bad_hex.as_bytes()).unwrap_err(), LutCfgError::BadHex);
        assert_eq!(
            validate_bands(b"variant=A\nband_lut=0011\n").unwrap_err(),
            LutCfgError::WrongLen
        );
        let bad_idx = format!("variant=A\nband_lut_99={}\n", hex_tagged(1));
        assert_eq!(
            validate_bands(bad_idx.as_bytes()).unwrap_err(),
            LutCfgError::BadBandIndex
        );
    }

    #[test]
    fn crlf_and_case_insensitive() {
        let cfg = format!("VARIANT=A\r\nBAND_LUT={}\r\n", hex_tagged(9));
        assert_eq!(parse_meta(cfg.as_bytes()).unwrap().variant, LutVariant::A);
        let (mut bands, mut set) = empty_bands();
        assert!(parse_bands(cfg.as_bytes(), &mut bands, &mut set).unwrap());
        assert_eq!(bands[3][0], 9);
    }
}
