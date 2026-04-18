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
use crate::fw::fat12::{self, FileRef};
#[cfg(feature = "embassy-base")]
use crate::fw::epd::EpdGfx;

/// Display dimensions.
#[cfg(feature = "embassy-base")]
const DISP_WIDTH: usize = 152;
#[cfg(feature = "embassy-base")]
const DISP_HEIGHT: usize = 152;
/// Display buffer bytes per row.
#[cfg(feature = "embassy-base")]
const DISP_ROW_STRIDE: usize = DISP_WIDTH / 8;

/// PCX header size.
#[cfg(feature = "embassy-base")]
const PCX_HEADER_SIZE: usize = 128;

/// Maximum number of sprite files.
#[cfg(feature = "embassy-base")]
const MAX_FRAMES: usize = 32;

// ---------------------------------------------------------------------------
// Static state
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy-base")]
struct SyncCell<T>(core::cell::UnsafeCell<T>);
#[cfg(feature = "embassy-base")]
unsafe impl<T> Sync for SyncCell<T> {}
#[cfg(feature = "embassy-base")]
impl<T> SyncCell<T> {
    const fn new(v: T) -> Self { Self(core::cell::UnsafeCell::new(v)) }
    fn get(&self) -> *mut T { self.0.get() }
}

#[cfg(feature = "embassy-base")]
static FRAMES: SyncCell<[FileRef; MAX_FRAMES]> = SyncCell::new([FileRef::EMPTY; MAX_FRAMES]);
#[cfg(feature = "embassy-base")]
static NAMES: SyncCell<[[u8; 11]; MAX_FRAMES]> = SyncCell::new([[0; 11]; MAX_FRAMES]);
static FRAME_COUNT: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

/// Discover PCX files on the FAT12 partition and store their handles.
#[cfg(feature = "embassy-base")]
pub async fn init() {
    let mut dir = match fat12::DirReader::open().await {
        Ok(d) => d,
        Err(_) => {
            defmt::warn!("sprite: no FAT12 filesystem found");
            return;
        }
    };

    let frames = unsafe { &mut *FRAMES.get() };
    let names = unsafe { &mut *NAMES.get() };
    let mut count = 0u8;

    while (count as usize) < MAX_FRAMES {
        match dir.next().await {
            Ok(Some((name, file))) => {
                if &name[8..11] == b"PCX" {
                    names[count as usize] = name;
                    frames[count as usize] = file;
                    count += 1;
                }
            }
            _ => break,
        }
    }

    if count == 0 {
        defmt::info!("sprite: no PCX files found on FAT12");
        return;
    }

    // Sort by filename so frame order is deterministic regardless of
    // the order files were written to the FAT directory.
    for i in 0..count as usize {
        for j in (i + 1)..count as usize {
            if names[j] < names[i] {
                names.swap(i, j);
                frames.swap(i, j);
            }
        }
    }

    for i in 0..count as usize {
        defmt::info!("sprite: [{}] {=[u8]:a}", i, &names[i][..8]);
    }

    FRAME_COUNT.store(count, core::sync::atomic::Ordering::Relaxed);
    defmt::info!("sprite: found {} PCX frame(s)", count);
}

/// Number of PCX sprite files found at init.
pub fn frame_count() -> u8 {
    FRAME_COUNT.load(core::sync::atomic::Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// PCX header
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy-base")]
struct PcxInfo {
    width: u16,
    height: u16,
    bytes_per_line: u16,
}

#[cfg(feature = "embassy-base")]
fn parse_pcx_header(hdr: &[u8]) -> Option<PcxInfo> {
    if hdr.len() < PCX_HEADER_SIZE { return None; }
    if hdr[0] != 0x0A { return None; }         // manufacturer
    if hdr[2] != 1 { return None; }             // RLE encoding
    if hdr[3] != 2 { return None; }             // 2 bpp
    if hdr[65] != 1 { return None; }            // 1 plane

    let xmin = u16::from_le_bytes([hdr[4], hdr[5]]);
    let ymin = u16::from_le_bytes([hdr[6], hdr[7]]);
    let xmax = u16::from_le_bytes([hdr[8], hdr[9]]);
    let ymax = u16::from_le_bytes([hdr[10], hdr[11]]);
    let bytes_per_line = u16::from_le_bytes([hdr[66], hdr[67]]);

    if xmax < xmin || ymax < ymin { return None; }

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
#[cfg(feature = "embassy-base")]
fn decode_rle_line(src: &[u8], dst: &mut [u8], bytes_per_line: usize) -> usize {
    let mut si = 0;
    let mut di = 0;
    while di < bytes_per_line && si < src.len() {
        let byte = src[si];
        si += 1;
        if byte >= 0xC0 {
            // Run: lower 6 bits = count, next byte = value.
            let count = (byte & 0x3F) as usize;
            let val = if si < src.len() { src[si] } else { 0 };
            si += 1;
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

/// Blit PCX frame `index` (from the init-time file list) onto the display.
#[cfg(feature = "embassy-base")]
pub async fn blit(display: &mut EpdGfx<'_>, index: u8, x: i32, y: i32) {
    let count = frame_count();
    if count == 0 || index >= count { return; }
    let file = unsafe { &(*FRAMES.get())[index as usize] };
    blit_file(display, file, x, y).await;
}

/// Blit a PCX file (by [`FileRef`]) onto the display at position (`x`, `y`).
///
/// Reads the PCX header for size, RLE-decodes scanlines via the work
/// buffer, and writes 2bpp pixels into the black and red framebuffers.
/// Clips to display bounds.  Transparent pixels (index 3) are skipped.
#[cfg(feature = "embassy-base")]
pub async fn blit_file(display: &mut EpdGfx<'_>, file: &fat12::FileRef, x: i32, y: i32) {
    let file_size = file.size as usize;
    let work_len = display.work_buffer_mut().len();
    if file_size > work_len || file_size < PCX_HEADER_SIZE {
        defmt::warn!("sprite: PCX too large or too small ({}B)", file_size);
        return;
    }

    {
        let work = display.work_buffer_mut();
        let n = fat12::read_file(file, 0, &mut work[..file_size])
            .await
            .unwrap_or(0);
        if n < file_size {
            defmt::warn!("sprite: short read: {} of {}", n, file_size);
            return;
        }
    }

    // Parse header from work buffer.
    let work = display.work_buffer_mut();
    let info = match parse_pcx_header(&work[..PCX_HEADER_SIZE]) {
        Some(i) => i,
        None => {
            defmt::trace!("sprite: invalid PCX header");
            return;
        }
    };

    let bpl = info.bytes_per_line as usize;
    let pcx_w = info.width as i32;
    let pcx_h = info.height as i32;

    // RLE-decode each scanline and blit to the framebuffers.
    // We decode one line at a time into a stack buffer, then write pixels.
    // The compressed data starts right after the 128-byte header.
    //
    // We need to split: work buffer holds the compressed file, and we also
    // need the black/red buffers.  Use all_buffers_mut to get all three,
    // then RLE-decode from work into a stack-local line buffer.

    let (black, red, work) = display.all_buffers_mut();
    let compressed = &work[PCX_HEADER_SIZE..file_size];
    let mut src_offset = 0;

    // Stack buffer for one decoded scanline (max 38 bytes for 152px @ 2bpp).
    let mut line_buf = [0u8; 256];

    for pcx_row in 0..pcx_h {
        let consumed = decode_rle_line(
            &compressed[src_offset..],
            &mut line_buf[..bpl],
            bpl,
        );
        src_offset += consumed;

        // PCX is top-down (row 0 = top of image).
        let screen_y = y + pcx_row;
        if screen_y < 0 || screen_y >= DISP_HEIGHT as i32 { continue; }
        let disp_row_off = screen_y as usize * DISP_ROW_STRIDE;

        for pixel in 0..pcx_w {
            let screen_x = x + pixel;
            if screen_x < 0 || screen_x >= DISP_WIDTH as i32 { continue; }

            // 2bpp: 4 pixels per byte, MSB first.
            let byte_idx = pixel as usize / 4;
            let shift = 6 - (pixel as usize % 4) * 2;
            let val = (line_buf[byte_idx] >> shift) & 0x03;

            let disp_byte = disp_row_off + screen_x as usize / 8;
            let bit = 0x80u8 >> (screen_x as usize % 8);

            // Palette: 00=black, 01=red, 10=white, 11=transparent
            match val {
                0b00 => { black[disp_byte] &= !bit; red[disp_byte] &= !bit; }
                0b01 => { black[disp_byte] |= bit;  red[disp_byte] |= bit;  }  // red
                0b10 => { black[disp_byte] |= bit;  red[disp_byte] &= !bit; }  // white
                _    => {} // transparent
            }
        }
    }
}
