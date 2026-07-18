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

/// Incremental variant of [`parse_meta`] for streamed files: feed each
/// text line as it comes off flash, then [`MetaScan::finish`]. Lines can
/// arrive in any chunking as long as each `feed_line` call gets one whole
/// line (no `\n`).
pub struct MetaScan {
    variant: Option<LutVariant>,
    speed: Option<u8>,
}

impl Default for MetaScan {
    fn default() -> Self {
        Self::new()
    }
}

impl MetaScan {
    pub const fn new() -> Self {
        Self {
            variant: None,
            speed: None,
        }
    }

    pub fn feed_line(&mut self, line: &[u8]) -> Result<(), LutCfgError> {
        let Some((key, val)) = kv_of_line(line) else {
            return Ok(());
        };
        if key.eq_ignore_ascii_case(b"variant") {
            self.variant = Some(parse_variant(val)?);
        } else if key.eq_ignore_ascii_case(b"speed") {
            self.speed = Some(
                parse_dec(val)
                    .and_then(u8_in_range)
                    .ok_or(LutCfgError::BadSpeed)?,
            );
        }
        Ok(())
    }

    pub fn finish(&self) -> Result<LutMeta, LutCfgError> {
        Ok(LutMeta {
            variant: self.variant.ok_or(LutCfgError::MissingOrBadVariant)?,
            speed: self.speed,
        })
    }
}

/// Incremental band scanner, in one of two modes:
///
/// - [`BandScan::validate`] — dry-run: every waveform value is checked
///   (hex, exact length, band index) but written nowhere. Lets the caller
///   reject a malformed file *before* mutating the live LUT table.
/// - [`BandScan::apply`] — fills the caller's band table. `band_lut` sets
///   a base applied to every band (at [`BandScan::finish`]); `band_lut_NN`
///   overrides band `NN` on sight. Bands set by neither are left untouched
///   so the caller keeps the OTP value there.
///
/// `finish` returns which bands were (or, for a dry-run, would be) set.
pub struct BandScan<'a> {
    bands: Option<&'a mut [[u8; LUT_UNIT_LEN]; NUM_BANDS]>,
    band_set: [bool; NUM_BANDS],
    base: Option<[u8; LUT_UNIT_LEN]>,
}

impl<'a> BandScan<'a> {
    /// Dry-run mode — validate only.
    pub const fn validate() -> BandScan<'static> {
        BandScan {
            bands: None,
            band_set: [false; NUM_BANDS],
            base: None,
        }
    }

    /// Apply mode — write into `bands`. Only run this over data a
    /// [`BandScan::validate`] pass has already accepted: a `feed_line`
    /// error in this mode can leave the table partially written.
    pub fn apply(bands: &'a mut [[u8; LUT_UNIT_LEN]; NUM_BANDS]) -> Self {
        BandScan {
            bands: Some(bands),
            band_set: [false; NUM_BANDS],
            base: None,
        }
    }

    pub fn feed_line(&mut self, line: &[u8]) -> Result<(), LutCfgError> {
        let Some((key, val)) = kv_of_line(line) else {
            return Ok(());
        };
        if key.eq_ignore_ascii_case(b"band_lut") {
            let mut b = [0u8; LUT_UNIT_LEN];
            decode_exact(val, &mut b)?;
            self.base = Some(b);
        } else if let Some(suffix) = strip_prefix_ci(key, b"band_lut_") {
            let idx = parse_dec(suffix)
                .filter(|&n| (n as usize) < NUM_BANDS)
                .ok_or(LutCfgError::BadBandIndex)? as usize;
            match &mut self.bands {
                Some(t) => decode_exact(val, &mut t[idx])?,
                None => {
                    let mut b = [0u8; LUT_UNIT_LEN];
                    decode_exact(val, &mut b)?;
                }
            }
            self.band_set[idx] = true;
        }
        Ok(())
    }

    /// Fill the base into any band not explicitly overridden, and return
    /// the final per-band set map.
    pub fn finish(mut self) -> [bool; NUM_BANDS] {
        if self.base.is_some() {
            match (&mut self.bands, &self.base) {
                (Some(t), Some(b)) => {
                    for i in 0..NUM_BANDS {
                        if !self.band_set[i] {
                            t[i] = *b;
                            self.band_set[i] = true;
                        }
                    }
                }
                _ => self.band_set = [true; NUM_BANDS], // dry-run: base fills all
            }
        }
        self.band_set
    }
}

/// Pass 1 — parse `variant` (required) and `speed` (optional), ignoring
/// waveform data. Cheap; lets the caller reject a variant mismatch before
/// filling the band table.
pub fn parse_meta(data: &[u8]) -> Result<LutMeta, LutCfgError> {
    let mut scan = MetaScan::new();
    for line in data.split(|&b| b == b'\n') {
        scan.feed_line(line)?;
    }
    scan.finish()
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
    let mut scan = BandScan::apply(bands);
    for line in data.split(|&b| b == b'\n') {
        scan.feed_line(line)?;
    }
    *band_set = scan.finish();
    Ok(band_set.iter().any(|&s| s))
}

/// Dry-run of [`parse_bands`]: validate every waveform value (hex + exact
/// length) and every band index, writing nowhere. Lets the caller reject a
/// malformed file *before* it mutates the live LUT table, so no scratch
/// table is needed. Each value decodes into a small reused stack local.
pub fn validate_bands(data: &[u8]) -> Result<(), LutCfgError> {
    let mut scan = BandScan::validate();
    for line in data.split(|&b| b == b'\n') {
        scan.feed_line(line)?;
    }
    Ok(())
}

/// Feed every complete (`\n`-terminated) line in `buf` to `f`, and return
/// the index just past the last newline — i.e. the start of the trailing
/// incomplete line, which the caller carries over into the next read
/// chunk. Building block for streaming a file through a scratch buffer
/// smaller than the file.
pub fn drain_lines<F>(buf: &[u8], f: &mut F) -> Result<usize, LutCfgError>
where
    F: FnMut(&[u8]) -> Result<(), LutCfgError>,
{
    let mut start = 0;
    while let Some(pos) = buf[start..].iter().position(|&b| b == b'\n') {
        f(&buf[start..start + pos])?;
        start += pos + 1;
    }
    Ok(start)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split one line into `(key, value)`, or `None` for comments / blanks /
/// lines without `=`.
fn kv_of_line(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let line = trim(line);
    if line.is_empty() || line[0] == b'#' {
        return None;
    }
    let eq = line.iter().position(|&b| b == b'=')?;
    Some((trim(&line[..eq]), trim(&line[eq + 1..])))
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
    if !src.len().is_multiple_of(2) {
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
        s.push_str(
            &(1..LUT_UNIT_LEN)
                .map(|_| "00")
                .collect::<std::string::String>(),
        );
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
        let good = format!(
            "variant=A\nband_lut={}\nband_lut_03={}\n",
            hex_tagged(1),
            hex_tagged(2)
        );
        assert!(validate_bands(good.as_bytes()).is_ok());
        // Bad hex, wrong length, and bad index all caught without writing.
        let bad_hex = format!("variant=A\nband_lut={}\n", "zz".repeat(LUT_UNIT_LEN));
        assert_eq!(
            validate_bands(bad_hex.as_bytes()).unwrap_err(),
            LutCfgError::BadHex
        );
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

    // ── streaming API ────────────────────────────────────────────────────

    /// Push `data` through the scanners the way the firmware's chunked
    /// file reader does: read `chunk`-sized pieces into a window buffer,
    /// drain complete lines, carry the partial tail over. Mirrors
    /// `epd::for_each_file_line`.
    fn stream_chunked<F>(data: &[u8], window: usize, chunk: usize, f: &mut F)
    where
        F: FnMut(&[u8]) -> Result<(), LutCfgError>,
    {
        let mut buf = vec![0u8; window];
        let mut carry = 0usize;
        let mut offset = 0usize;
        loop {
            let space = window - carry;
            let n = chunk.min(space).min(data.len() - offset);
            buf[carry..carry + n].copy_from_slice(&data[offset..offset + n]);
            offset += n;
            let filled = carry + n;
            let consumed = drain_lines(&buf[..filled], f).unwrap();
            if offset == data.len() {
                if consumed < filled {
                    f(&buf[consumed..filled]).unwrap();
                }
                return;
            }
            carry = filled - consumed;
            assert!(carry < window, "line longer than window");
            buf.copy_within(consumed..filled, 0);
        }
    }

    #[test]
    fn streamed_equals_slice_parse() {
        // A file bigger than the stream window: base + all 16 overrides
        // (~3.7 KB), streamed through a 512-byte window in 128-byte reads.
        let mut cfg = format!(
            "# big\nvariant=B\nspeed=90\nband_lut={}\n",
            hex_tagged(0xEE)
        );
        for i in 0..NUM_BANDS {
            cfg.push_str(&format!("band_lut_{:02}={}\n", i, hex_tagged(i as u8)));
        }
        let data = cfg.as_bytes();
        assert!(data.len() > 512);

        let mut meta = MetaScan::new();
        let mut check = BandScan::validate();
        stream_chunked(data, 512, 128, &mut |line: &[u8]| {
            meta.feed_line(line)?;
            check.feed_line(line)
        });
        let m = meta.finish().unwrap();
        assert_eq!(m.variant, LutVariant::B);
        assert_eq!(m.speed, Some(90));
        assert!(check.finish().iter().all(|&s| s));

        let (mut bands, _) = empty_bands();
        let mut apply = BandScan::apply(&mut bands);
        stream_chunked(data, 512, 128, &mut |line: &[u8]| apply.feed_line(line));
        let set = apply.finish();
        assert!(set.iter().all(|&s| s));
        // Overrides win over base; every band carries its own tag.
        for (i, b) in bands.iter().enumerate() {
            assert_eq!(b[0], i as u8);
        }

        // Same input through the slice API gives the same table.
        let (mut bands2, mut set2) = empty_bands();
        parse_bands(data, &mut bands2, &mut set2).unwrap();
        assert_eq!(bands, bands2);
    }

    #[test]
    fn streamed_base_fill_at_finish() {
        let cfg = format!(
            "variant=A\nband_lut={}\nband_lut_05={}\n",
            hex_tagged(0xBB),
            hex_tagged(0x55)
        );
        let (mut bands, _) = empty_bands();
        let mut apply = BandScan::apply(&mut bands);
        // Tiny window/chunk to force many carry-overs mid-line.
        stream_chunked(cfg.as_bytes(), 256, 7, &mut |line: &[u8]| {
            apply.feed_line(line)
        });
        let set = apply.finish();
        assert!(set.iter().all(|&s| s));
        assert_eq!(bands[5][0], 0x55);
        assert_eq!(bands[0][0], 0xBB);
        assert_eq!(bands[15][0], 0xBB);
    }

    #[test]
    fn streamed_validate_catches_errors() {
        let mut check = BandScan::validate();
        let bad = format!("variant=A\nband_lut_16={}\n", hex_tagged(1));
        let mut err = None;
        for line in bad.as_bytes().split(|&b| b == b'\n') {
            if let Err(e) = check.feed_line(line) {
                err = Some(e);
                break;
            }
        }
        assert_eq!(err, Some(LutCfgError::BadBandIndex));
    }

    #[test]
    fn drain_lines_returns_partial_tail_start() {
        let mut seen: std::vec::Vec<std::vec::Vec<u8>> = vec![];
        let consumed = drain_lines(b"a=1\nb=2\npartial", &mut |l: &[u8]| {
            seen.push(l.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(consumed, 8);
        assert_eq!(seen, vec![b"a=1".to_vec(), b"b=2".to_vec()]);
    }
}
