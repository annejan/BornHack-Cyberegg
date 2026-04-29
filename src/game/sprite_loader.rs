//! Sprite loader — reads 2bpp PCX files from the FAT12 flash partition
//! and blits them directly into the display's black and red framebuffers.
//!
//! PCX format: version 5, RLE compressed, 2bpp, 1 plane, top-down.
//! Header is 128 bytes; width/height derived from bounding box.
//!
//! Palette mapping (from PCX header):
//!   index 0 (00) = black
//!   index 1 (01) = red
//!   index 2 (10) = white
//!   index 3 (11) = transparent (skip pixel)
//!
//! Supports arbitrary sizes and screen positions with clipping.
//! Uses the display's work buffer as scratch for RLE decoding.

#[cfg(feature = "embassy-base")]
use crate::fw::epd::EpdGfx;
#[cfg(feature = "embassy-base")]
use crate::fw::fat12;

/// Display dimensions.
#[cfg(feature = "embassy-base")]
const DISP_WIDTH: usize = 152;
#[cfg(feature = "embassy-base")]
const DISP_HEIGHT: usize = 152;
/// Display buffer bytes per row.
#[cfg(feature = "embassy-base")]
const DISP_ROW_STRIDE: usize = DISP_WIDTH / 8;

/// PCX header size.
const PCX_HEADER_SIZE: usize = 128;

// ---------------------------------------------------------------------------
// Static state — per-(pp, aa) presence bitmap
// ---------------------------------------------------------------------------
//
// Each entry is a u32 bitmap; bit `ff` set means file `PPAAFF.PCX`
// exists on the FAT12 partition.  Populated once at boot by [`init`]
// and queried synchronously by [`count_anim_frames`].  Way leaner
// than caching every filename — at 4 × 21 × 4 = 336 bytes it accepts
// any number of sprite files without a hardcoded cap.

/// Pet/category-prefix range covered by the catalogue.  PP=0 snail,
/// 1 cat, 2 sponsors, 3 menu icons.  Animations queried via
/// [`count_anim_frames`] today only use 0..=1, but the bitmap is sized
/// for all four prefixes so the same scan can answer for any future
/// caller.
#[cfg(feature = "embassy-base")]
const PP_MAX: usize = 4;
/// Anim-id range covered.  Anim ids go 0x00..=0x14 (start screen +
/// 20 lifecycle anims) — 21 entries.
#[cfg(feature = "embassy-base")]
const AA_MAX: usize = 21;
/// Maximum frame index per animation (bit position in the u32).
#[cfg(feature = "embassy-base")]
const FF_MAX: u8 = 32;

#[cfg(feature = "embassy-base")]
static ANIM_PRESENCE: [[core::sync::atomic::AtomicU32; AA_MAX]; PP_MAX] =
    [const { [const { core::sync::atomic::AtomicU32::new(0) }; AA_MAX] }; PP_MAX];

#[cfg(not(feature = "simulator"))]
static FRAME_COUNT: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

/// Decode `b"AB"` (two ASCII hex digits) into a 0..=255 byte.
#[cfg(feature = "embassy-base")]
fn parse_hex_pair(hi: u8, lo: u8) -> Option<u8> {
    fn d(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'A'..=b'F' => Some(c - b'A' + 10),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    }
    Some((d(hi)? << 4) | d(lo)?)
}

/// Discover PCX files on the FAT12 partition and record per-animation
/// presence in [`ANIM_PRESENCE`].  Walks the entire directory — no
/// catalogue size cap.
#[cfg(feature = "embassy-base")]
pub async fn init() {
    let mut dir = match fat12::DirReader::open().await {
        Ok(d) => d,
        Err(_) => {
            defmt::warn!("sprite: no FAT12 filesystem found");
            return;
        }
    };

    let mut total: u8 = 0;
    loop {
        match dir.next().await {
            Ok(Some((name, _file))) => {
                if &name[8..11] != b"PCX" {
                    continue;
                }
                total = total.saturating_add(1);
                defmt::info!("sprite: catalogued {=[u8]:a}", &name[..]);
                let Some(pp) = parse_hex_pair(name[0], name[1]) else {
                    continue;
                };
                let Some(aa) = parse_hex_pair(name[2], name[3]) else {
                    continue;
                };
                let Some(ff) = parse_hex_pair(name[4], name[5]) else {
                    continue;
                };
                if (pp as usize) < PP_MAX && (aa as usize) < AA_MAX && ff < FF_MAX {
                    let mask = 1u32 << ff;
                    ANIM_PRESENCE[pp as usize][aa as usize]
                        .fetch_or(mask, core::sync::atomic::Ordering::Relaxed);
                }
            }
            _ => break,
        }
    }

    FRAME_COUNT.store(total, core::sync::atomic::Ordering::Relaxed);
    if total == 0 {
        defmt::info!("sprite: no PCX files found on FAT12");
    } else {
        defmt::info!("sprite: catalogued {} PCX file(s)", total);
    }
}

/// Number of PCX sprite files available.
///
/// On hardware this reflects the FAT12 partition scan done in
/// [`init`] — zero means the badge was flashed without art assets,
/// which the game UI surfaces as a "No sprites on flash" placeholder
/// in the pet area.
///
/// In the simulator there's no FAT12 to scan, but PCX files are
/// resolved at draw time from `assets/to-badge/` via
/// [`blit_pcx_sim`].  We therefore report `u8::MAX` so the firmware
/// "no sprites" fallback message doesn't fire under `make sim`; if a
/// specific PCX is actually missing the simulator silently shows
/// nothing in the pet area, matching the embedded fail-soft.
pub fn frame_count() -> u8 {
    #[cfg(feature = "simulator")]
    {
        u8::MAX
    }
    #[cfg(not(feature = "simulator"))]
    {
        FRAME_COUNT.load(core::sync::atomic::Ordering::Relaxed)
    }
}

/// Count the contiguous run of frames available for an animation,
/// starting at frame `00`.  `prefix` is the 4-byte `PPAA` portion of
/// the FAT12 8.3 filename.
///
/// Firmware: read directly from the [`ANIM_PRESENCE`] bitmap built at
/// boot.  Simulator: probes `assets/to-badge/` directly.
///
/// Frame `00` missing → returns 0 (no animation available).
pub fn count_anim_frames(prefix: &[u8; 4]) -> u8 {
    #[cfg(feature = "embassy-base")]
    {
        let pp = match parse_hex_pair(prefix[0], prefix[1]) {
            Some(v) if (v as usize) < PP_MAX => v as usize,
            _ => return 0,
        };
        let aa = match parse_hex_pair(prefix[2], prefix[3]) {
            Some(v) if (v as usize) < AA_MAX => v as usize,
            _ => return 0,
        };
        let bitmap = ANIM_PRESENCE[pp][aa].load(core::sync::atomic::Ordering::Relaxed);
        bitmap.trailing_ones().min(FF_MAX as u32) as u8
    }
    #[cfg(all(feature = "simulator", not(feature = "embassy-base")))]
    {
        const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";
        const MAX_ANIM_FRAMES: u8 = 32;
        use std::path::Path;
        let mut count = 0u8;
        while count < MAX_ANIM_FRAMES {
            let name: [u8; 11] = [
                prefix[0],
                prefix[1],
                prefix[2],
                prefix[3],
                HEX_DIGITS[(count >> 4) as usize],
                HEX_DIGITS[(count & 0xF) as usize],
                b' ',
                b' ',
                b'P',
                b'C',
                b'X',
            ];
            let path = std::format!("{}{}", SIM_ASSET_DIR, fat_name_to_dotted(&name));
            if !Path::new(&path).exists() {
                break;
            }
            count += 1;
        }
        count
    }
    #[cfg(not(any(feature = "embassy-base", feature = "simulator")))]
    {
        let _ = prefix;
        0
    }
}

// ---------------------------------------------------------------------------
// PCX header (shared between embedded blit_file and the simulator helper)
// ---------------------------------------------------------------------------

struct PcxInfo {
    width: u16,
    height: u16,
    bytes_per_line: u16,
}

fn parse_pcx_header(hdr: &[u8]) -> Option<PcxInfo> {
    if hdr.len() < PCX_HEADER_SIZE {
        return None;
    }
    if hdr[0] != 0x0A {
        return None;
    } // manufacturer
    if hdr[2] != 1 {
        return None;
    } // RLE encoding
    if hdr[3] != 2 {
        return None;
    } // 2 bpp
    if hdr[65] != 1 {
        return None;
    } // 1 plane

    let xmin = u16::from_le_bytes([hdr[4], hdr[5]]);
    let ymin = u16::from_le_bytes([hdr[6], hdr[7]]);
    let xmax = u16::from_le_bytes([hdr[8], hdr[9]]);
    let ymax = u16::from_le_bytes([hdr[10], hdr[11]]);
    let bytes_per_line = u16::from_le_bytes([hdr[66], hdr[67]]);

    if xmax < xmin || ymax < ymin {
        return None;
    }

    Some(PcxInfo {
        width: xmax - xmin + 1,
        height: ymax - ymin + 1,
        bytes_per_line,
    })
}

// ---------------------------------------------------------------------------
// RLE decoder
// ---------------------------------------------------------------------------

/// Decode one scanline of PCX RLE data.
///
/// `src` is the compressed data starting at the current position.
/// `dst` receives exactly `bytes_per_line` decoded bytes.
/// Returns the number of bytes consumed from `src`.
fn decode_rle_line(src: &[u8], dst: &mut [u8], bytes_per_line: usize) -> usize {
    let mut si = 0;
    let mut di = 0;
    while di < bytes_per_line && si < src.len() {
        let byte = src[si];
        si += 1;
        if byte >= 0xC0 {
            // Run: lower 6 bits = count, next byte = value.
            let count = (byte & 0x3F) as usize;
            // Only consume the value byte if it's actually present — otherwise
            // si would overshoot src.len() when a slice ends on a run header.
            let val = if si < src.len() {
                let v = src[si];
                si += 1;
                v
            } else {
                0
            };
            for _ in 0..count {
                if di < bytes_per_line {
                    dst[di] = val;
                    di += 1;
                }
            }
        } else {
            // Literal byte.
            dst[di] = byte;
            di += 1;
        }
    }
    // Pad remainder with zeros if stream ended early.
    while di < bytes_per_line {
        dst[di] = 0;
        di += 1;
    }
    si
}

// ---------------------------------------------------------------------------
// Blit
// ---------------------------------------------------------------------------

/// Blit a PCX file (by [`FileRef`]) onto the display at position (`x`, `y`).
///
/// Streams the PCX from flash through a small sliding buffer, RLE-decodes
/// one scanline at a time, and writes 2bpp pixels into the black and red
/// framebuffers.  No size limit: file can exceed any on-device buffer.
/// Clips to display bounds.  Transparent pixels (index 3) are skipped.
#[cfg(feature = "embassy-base")]
pub async fn blit_file(display: &mut EpdGfx<'_>, file: &fat12::FileRef, x: i32, y: i32) {
    let file_size = file.size as usize;
    if file_size < PCX_HEADER_SIZE {
        defmt::warn!("sprite: PCX too small ({}B)", file_size);
        return;
    }

    // Sliding read-ahead buffer.  Refilled from flash as the RLE decoder
    // consumes bytes; sized for several worst-case scanlines (bpl ≤ 38 for
    // 152-wide 2bpp) to amortise flash reads.
    let mut read_buf = [0u8; 256];
    let mut read_pos: usize; // bytes consumed from read_buf (set after header)
    let mut read_len: usize; // valid bytes in read_buf (set by prime read)
    let mut file_offset: u32; // next byte to fetch from flash (set by prime read)

    // Prime the buffer with the header + as much compressed data as fits.
    let first = read_buf.len().min(file_size);
    match fat12::read_file(file, 0, &mut read_buf[..first]).await {
        Ok(n) if n >= PCX_HEADER_SIZE => {
            read_len = n;
            file_offset = n as u32;
        }
        _ => {
            defmt::warn!("sprite: short header read");
            return;
        }
    }

    let info = match parse_pcx_header(&read_buf[..PCX_HEADER_SIZE]) {
        Some(i) => i,
        None => {
            defmt::trace!("sprite: invalid PCX header");
            return;
        }
    };
    read_pos = PCX_HEADER_SIZE;

    let bpl = info.bytes_per_line as usize;
    let pcx_w = info.width as i32;
    let pcx_h = info.height as i32;

    // Stack buffer for one decoded scanline (max 38 bytes for 152px @ 2bpp).
    let mut line_buf = [0u8; 256];

    // We only need black/red framebuffers now — the work buffer is no
    // longer used for sprite decoding.
    let (black, red, _work) = display.all_buffers_mut();

    for pcx_row in 0..pcx_h {
        // Worst-case compressed bytes per scanline is `2 * bpl`: when every
        // pixel byte is ≥ 0xC0 it must be escaped as a (0xC1, val) 2-byte
        // run — doubling the per-scanline size.  Refill if we don't have
        // that much buffered and flash still has data.
        if read_len - read_pos < 2 * bpl && (file_offset as usize) < file_size {
            // Compact: slide unread bytes to the start.
            read_buf.copy_within(read_pos..read_len, 0);
            read_len -= read_pos;
            read_pos = 0;

            let want = read_buf.len() - read_len;
            let can = (file_size - file_offset as usize).min(want);
            if can > 0 {
                match fat12::read_file(file, file_offset, &mut read_buf[read_len..read_len + can])
                    .await
                {
                    Ok(n) => {
                        read_len += n;
                        file_offset += n as u32;
                    }
                    Err(_) => {
                        defmt::warn!("sprite: flash read failed mid-stream");
                        return;
                    }
                }
            }
        }

        let consumed = decode_rle_line(&read_buf[read_pos..read_len], &mut line_buf[..bpl], bpl);
        read_pos += consumed;

        // PCX is top-down (row 0 = top of image).
        let screen_y = y + pcx_row;
        if screen_y < 0 || screen_y >= DISP_HEIGHT as i32 {
            continue;
        }
        let disp_row_off = screen_y as usize * DISP_ROW_STRIDE;

        for pixel in 0..pcx_w {
            let screen_x = x + pixel;
            if screen_x < 0 || screen_x >= DISP_WIDTH as i32 {
                continue;
            }

            // 2bpp: 4 pixels per byte, MSB first.
            let byte_idx = pixel as usize / 4;
            let shift = 6 - (pixel as usize % 4) * 2;
            let val = (line_buf[byte_idx] >> shift) & 0x03;

            let disp_byte = disp_row_off + screen_x as usize / 8;
            let bit = 0x80u8 >> (screen_x as usize % 8);

            // Palette: 00=black, 01=red, 10=white, 11=transparent
            match val {
                0b00 => {
                    black[disp_byte] &= !bit;
                    red[disp_byte] &= !bit;
                }
                0b01 => {
                    black[disp_byte] |= bit;
                    red[disp_byte] |= bit;
                } // red
                0b10 => {
                    black[disp_byte] |= bit;
                    red[disp_byte] &= !bit;
                } // white
                _ => {} // transparent
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Simulator helper — load + blit a PCX from the host filesystem
// ---------------------------------------------------------------------------
//
// The embedded path streams sprites from the FAT12 partition on QSPI flash;
// the simulator has no flash, so it loads PCX bytes directly from
// `<CARGO_MANIFEST_DIR>/assets/to-badge/<name>.PCX`.  Path is resolved at
// compile time so the working directory at run time doesn't matter.

#[cfg(feature = "simulator")]
const SIM_ASSET_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/to-badge/");

/// Convert an FAT12 8.3 padded filename like `b"010F00  PCX"` to the
/// dotted form `"010F00.PCX"`.
#[cfg(feature = "simulator")]
fn fat_name_to_dotted(name: &[u8; 11]) -> std::string::String {
    use std::string::String;
    let mut out = String::with_capacity(12);
    for &b in &name[..8] {
        if b != b' ' {
            out.push(b as char);
        }
    }
    out.push('.');
    for &b in &name[8..11] {
        if b != b' ' {
            out.push(b as char);
        }
    }
    out
}

/// Blit a PCX file from `assets/to-badge/<name>.PCX` onto a `DrawTarget`.
/// Silently does nothing if the file is missing or malformed — same
/// "best effort" stance as the embedded blitter.
///
/// Used by the simulator binary; the embedded firmware uses
/// [`blit_file`] which streams from FAT12 instead.
#[cfg(feature = "simulator")]
pub fn blit_pcx_sim<D>(display: &mut D, name: &[u8; 11], x: i32, y: i32)
where
    D: embedded_graphics::draw_target::DrawTarget<Color = crate::TriColor>,
{
    use embedded_graphics::Pixel;
    use embedded_graphics::geometry::Point;

    let path = std::format!("{}{}", SIM_ASSET_DIR, fat_name_to_dotted(name));
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return,
    };
    if bytes.len() < PCX_HEADER_SIZE {
        return;
    }
    let info = match parse_pcx_header(&bytes[..PCX_HEADER_SIZE]) {
        Some(i) => i,
        None => return,
    };
    let bpl = info.bytes_per_line as usize;
    let pcx_w = info.width as i32;
    let pcx_h = info.height as i32;

    let mut data = &bytes[PCX_HEADER_SIZE..];
    let mut line = std::vec![0u8; bpl];
    let mut pixels: std::vec::Vec<Pixel<crate::TriColor>> = std::vec::Vec::new();

    for pcx_row in 0..pcx_h {
        let consumed = decode_rle_line(data, &mut line, bpl);
        data = &data[consumed.min(data.len())..];

        let screen_y = y + pcx_row;
        for pixel_x in 0..pcx_w {
            let screen_x = x + pixel_x;
            let byte_idx = pixel_x as usize / 4;
            let shift = 6 - (pixel_x as usize % 4) * 2;
            let val = (line[byte_idx] >> shift) & 0x03;

            // Palette: 00=black, 01=red, 10=white, 11=transparent.
            let color = match val {
                0b00 => crate::BLACK,
                0b01 => crate::RED,
                0b10 => crate::WHITE,
                _ => continue,
            };
            pixels.push(Pixel(Point::new(screen_x, screen_y), color));
        }
    }

    let _ = display.draw_iter(pixels);
}
