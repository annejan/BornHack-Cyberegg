//! Minimal read-only FAT12 filesystem for the external flash FAT partition.
//!
//! Provides a cursor-based [`DirReader`] over directory entries and a file
//! reader that streams data into a caller-supplied buffer.  All flash
//! access goes through [`crate::fw::flash`] (async-mutex-protected).
//!
//! # FAT12 on-disk layout (1 MiB partition)
//!
//! ```text
//!   ┌──────────────────┐  sector 0
//!   │   Boot sector    │  BPB (BIOS Parameter Block): sector size, cluster
//!   │                  │  size, FAT count, root entry count, etc.
//!   ├──────────────────┤  sector 1  (= reserved_sectors)
//!   │   FAT #1         │  File Allocation Table — linked list of cluster
//!   │                  │  chains.  Each entry is 12 bits (1.5 bytes).
//!   ├──────────────────┤
//!   │   FAT #2         │  Backup copy of FAT #1 (we only read FAT #1).
//!   ├──────────────────┤
//!   │  Root directory   │  Fixed-size array of 32-byte entries.  Each entry
//!   │                  │  holds an 8.3 filename, attributes, first cluster
//!   │                  │  number, and file size.
//!   ├──────────────────┤
//!   │   Data region    │  File contents stored in clusters.  Cluster 2 is
//!   │                  │  the first data cluster (clusters 0 and 1 are
//!   │                  │  reserved in the FAT).
//!   └──────────────────┘
//! ```
//!
//! # How file reading works
//!
//! 1. Parse the boot sector to learn where the FAT, root directory, and data
//!    region start.
//! 2. Scan the root directory (32 bytes per entry) to find the file's first
//!    cluster number and size.
//! 3. To read file data: look up the cluster's absolute flash address, read
//!    from it, then follow the FAT chain to the next cluster.
//!
//! # FAT12 cluster chain
//!
//! Each FAT entry is 12 bits.  For cluster N:
//! - Byte offset in FAT = N × 3 / 2
//! - If N is even: entry = low 12 bits of the 16-bit word at that offset
//! - If N is odd:  entry = high 12 bits (shift right by 4)
//! - Entry values: 0x000 = free, 0xFF8–0xFFF = end of chain, else = next
//!   cluster
//!
//! # Usage
//!
//! ```rust,ignore
//! // Iterate all files:
//! let mut dir = DirReader::open().await?;
//! while let Some((name, file)) = dir.next().await? {
//!     defmt::info!("file: {:?} size={}", &name[..8], file.size);
//! }
//!
//! // Find by name:
//! let file = fat12::find_file(&fat12::to_8_3("CONFIG.TXT").unwrap()).await?;
//! fat12::read_file(&file, 0, &mut buf).await?;
//!
//! // Load the 3rd PCX file:
//! let mut dir = DirReader::open().await?;
//! let file = dir.nth_by_ext(b"PCX", 2).await?.ok_or(FatError::FileNotFound)?;
//! fat12::read_file(&file, 128, &mut pixel_buf).await?;
//! ```

use crate::fw::flash;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
pub enum FatError {
    /// No valid FAT12 boot sector found (bad jump byte or zero sector size).
    NoFilesystem,
    /// File not found in root directory.
    FileNotFound,
    /// Flash read failed (QSPI hardware error).
    FlashError,
    /// Corrupt FAT chain or directory entry (unexpected end-of-chain or bad
    /// cluster).
    Corrupt,
}

// ---------------------------------------------------------------------------
// FileRef — lightweight file handle (6 bytes)
// ---------------------------------------------------------------------------

/// Handle to a file on the filesystem.  Stores only the first cluster
/// number and the file size — enough to read the entire file by following
/// the FAT chain.  Obtained from [`DirReader`] or [`find_file`].
///
/// Can be copied, stored in arrays, and reused for multiple reads at
/// different offsets without re-scanning the directory.
#[derive(Clone, Copy)]
pub struct FileRef {
    /// First cluster in the FAT chain (cluster 2 = first data cluster).
    pub(crate) first_cluster: u16,
    /// File size in bytes.
    pub size: u32,
}

impl FileRef {
    /// Empty/invalid handle, used for array initialization.
    pub const EMPTY: Self = Self {
        first_cluster: 0,
        size: 0,
    };
}

// ---------------------------------------------------------------------------
// FatParams — boot sector parameters (lives on the stack, not stored)
// ---------------------------------------------------------------------------

/// Geometry parsed from the BPB (BIOS Parameter Block) in the boot sector.
/// Re-read from flash each time it's needed — 64 bytes, one DMA transfer.
struct FatParams {
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    num_fats: u8,
    root_entry_count: u16,
    sectors_per_fat: u16,
}

impl FatParams {
    /// Bytes per cluster (sector_size × sectors_per_cluster).
    fn cluster_bytes(&self) -> u32 {
        self.sectors_per_cluster as u32 * self.bytes_per_sector as u32
    }

    /// Absolute flash address of the first FAT table.
    /// Layout: [boot sector(s)] [FAT #1] [FAT #2] [root dir] [data]
    fn fat_offset(&self) -> u32 {
        flash::FAT_OFFSET + self.reserved_sectors as u32 * self.bytes_per_sector as u32
    }

    /// Absolute flash address of the root directory.
    fn root_dir_offset(&self) -> u32 {
        let fat_size = self.num_fats as u32 * self.sectors_per_fat as u32;
        flash::FAT_OFFSET + (self.reserved_sectors as u32 + fat_size) * self.bytes_per_sector as u32
    }

    /// Number of sectors occupied by the root directory.
    fn root_dir_sectors(&self) -> u32 {
        (self.root_entry_count as u32 * 32).div_ceil(self.bytes_per_sector as u32)
    }

    /// Absolute flash address of the data region (cluster 2 starts here).
    fn data_region_offset(&self) -> u32 {
        self.root_dir_offset() + self.root_dir_sectors() * self.bytes_per_sector as u32
    }

    /// Absolute flash address of the given cluster's data.
    /// Clusters are numbered starting at 2 (0 and 1 are reserved in FAT).
    fn cluster_addr(&self, cluster: u16) -> u32 {
        self.data_region_offset() + (cluster as u32 - 2) * self.cluster_bytes()
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Read and parse the boot sector from the FAT partition.
/// Returns the filesystem geometry needed for all other operations.
async fn read_params() -> Result<FatParams, FatError> {
    let mut buf = [0u8; 64];
    flash::read(flash::FAT_OFFSET, &mut buf)
        .await
        .map_err(|_| FatError::FlashError)?;

    // The first byte of a valid FAT boot sector is a jump instruction:
    // 0xEB (short jump) or 0xE9 (near jump).
    if buf[0] != 0xEB && buf[0] != 0xE9 {
        return Err(FatError::NoFilesystem);
    }
    let bps = u16::from_le_bytes([buf[11], buf[12]]);
    if bps == 0 || buf[13] == 0 {
        return Err(FatError::NoFilesystem);
    }

    Ok(FatParams {
        bytes_per_sector: bps,        // BPB offset 11: usually 512
        sectors_per_cluster: buf[13], // BPB offset 13: e.g. 8 for 4K clusters
        reserved_sectors: u16::from_le_bytes([buf[14], buf[15]]), // BPB offset 14
        num_fats: buf[16],            // BPB offset 16: usually 2
        root_entry_count: u16::from_le_bytes([buf[17], buf[18]]), // BPB offset 17
        sectors_per_fat: u16::from_le_bytes([buf[22], buf[23]]), // BPB offset 22
    })
}

/// Follow the FAT12 chain: given a cluster number, return the next cluster.
///
/// FAT12 packs two 12-bit entries into 3 bytes:
///   byte_offset = cluster × 3 / 2
///   even cluster: low 12 bits of u16 at byte_offset
///   odd cluster:  high 12 bits (u16 >> 4)
///
/// Returns `None` for end-of-chain (0xFF8–0xFFF) or free (0x000).
async fn next_cluster(params: &FatParams, cluster: u16) -> Result<Option<u16>, FatError> {
    let fat_addr = params.fat_offset();
    let byte_offset = (cluster as u32 * 3) / 2;
    let mut pair = [0u8; 2];
    flash::read(fat_addr + byte_offset, &mut pair)
        .await
        .map_err(|_| FatError::FlashError)?;

    let val = if cluster & 1 == 0 {
        // Even cluster: take low 12 bits.
        u16::from_le_bytes(pair) & 0x0FFF
    } else {
        // Odd cluster: take high 12 bits.
        u16::from_le_bytes(pair) >> 4
    };

    // 0xFF8..=0xFFF = end of chain, 0x000 = free, 0xFF7 = bad sector.
    if val >= 0xFF8 || val == 0 {
        Ok(None)
    } else {
        Ok(Some(val))
    }
}

/// Parse a 32-byte directory entry into a filename and file handle.
///
/// Directory entry layout:
///   [0..11]  8.3 filename (8 name + 3 ext, space-padded, uppercase)
///   [11]     attributes (bit flags: read-only, hidden, system, volume, dir,
/// archive)   [26..28] first cluster number (little-endian u16)
///   [28..32] file size in bytes (little-endian u32)
fn parse_entry(raw: &[u8; 32]) -> ([u8; 11], FileRef) {
    let mut name = [0u8; 11];
    name.copy_from_slice(&raw[0..11]);
    let file = FileRef {
        first_cluster: u16::from_le_bytes([raw[26], raw[27]]),
        size: u32::from_le_bytes([raw[28], raw[29], raw[30], raw[31]]),
    };
    (name, file)
}

// ---------------------------------------------------------------------------
// DirReader — cursor over root directory entries
// ---------------------------------------------------------------------------

/// Cursor over root directory entries.  Reads one 32-byte entry from flash
/// per [`next()`](DirReader::next) call.  Lives entirely on the stack —
/// no heap allocation, no buffers that grow with file count.
///
/// The root directory is a fixed-size array at a known flash address
/// (computed from the boot sector).  Each entry is 32 bytes.  The cursor
/// simply increments an index and reads the next entry on demand.
pub struct DirReader {
    /// Absolute flash address of the first directory entry.
    root_addr: u32,
    /// Maximum number of entries in the root directory.
    total: u16,
    /// Next entry index to read (0-based).
    index: u16,
}

impl DirReader {
    /// Open the root directory for iteration.
    ///
    /// Reads the boot sector (64 bytes) to determine the directory's
    /// flash address and entry count.
    pub async fn open() -> Result<Self, FatError> {
        let params = read_params().await?;
        Ok(Self {
            root_addr: params.root_dir_offset(),
            total: params.root_entry_count,
            index: 0,
        })
    }

    /// Advance to the next valid file entry.
    ///
    /// Each call reads one 32-byte entry from flash.  Automatically skips:
    /// - `0xE5` prefix: deleted entry
    /// - `0x0F` attribute: long filename (LFN) entry
    /// - Volume label and subdirectory entries
    ///
    /// Returns `Ok(None)` when all entries in the root directory have
    /// been scanned.  Skips empty (`0x00`), deleted (`0xE5`), LFN, and
    /// volume / directory entries.
    ///
    /// The FAT spec defines `0x00` as the end-of-directory marker, but
    /// some hosts leave stale `0x00` slots between valid entries when
    /// files are added/removed via USB mass storage.  Stopping at the
    /// first `0x00` would hide every valid entry past such a hole, so
    /// we walk the full pre-allocated range instead and just skip the
    /// empty slots.
    pub async fn next(&mut self) -> Result<Option<([u8; 11], FileRef)>, FatError> {
        let mut raw = [0u8; 32];
        while self.index < self.total {
            let addr = self.root_addr + self.index as u32 * 32;
            self.index += 1;

            flash::read(addr, &mut raw)
                .await
                .map_err(|_| FatError::FlashError)?;

            if raw[0] == 0x00 {
                continue;
            } // empty slot — keep scanning, more files may follow
            if raw[0] == 0xE5 {
                continue;
            } // deleted
            if raw[11] & 0x0F == 0x0F {
                continue;
            } // LFN fragment
            if raw[11] & 0x18 != 0 {
                continue;
            } // volume label or directory

            return Ok(Some(parse_entry(&raw)));
        }
        Ok(None)
    }

    /// Advance to the next entry whose extension matches `ext` (3 bytes).
    ///
    /// The extension occupies bytes 8–10 of the 8.3 name and is always
    /// uppercase in the directory entry.
    pub async fn next_by_ext(&mut self, ext: &[u8; 3]) -> Result<Option<FileRef>, FatError> {
        while let Some((name, file)) = self.next().await? {
            if &name[8..11] == ext {
                return Ok(Some(file));
            }
        }
        Ok(None)
    }

    /// Skip to the Nth matching entry (0-based) with the given extension.
    ///
    /// Equivalent to calling [`next_by_ext`](Self::next_by_ext) N+1 times.
    pub async fn nth_by_ext(&mut self, ext: &[u8; 3], n: u16) -> Result<Option<FileRef>, FatError> {
        let mut count = 0u16;
        while let Some(file) = self.next_by_ext(ext).await? {
            if count == n {
                return Ok(Some(file));
            }
            count += 1;
        }
        Ok(None)
    }

    /// Reset the cursor to the first directory entry.
    pub fn rewind(&mut self) {
        self.index = 0;
    }
}

// ---------------------------------------------------------------------------
// File lookup by name
// ---------------------------------------------------------------------------

/// Find a file by its 8.3 name.
///
/// Opens a [`DirReader`] and scans until a matching name is found.
/// Use [`to_8_3`] to convert a human-readable name like `"CONFIG.TXT"`
/// to the 11-byte 8.3 format.
pub async fn find_file(name_8_3: &[u8; 11]) -> Result<FileRef, FatError> {
    let mut dir = DirReader::open().await?;
    while let Some((name, file)) = dir.next().await? {
        if &name == name_8_3 {
            return Ok(file);
        }
    }
    Err(FatError::FileNotFound)
}

/// Convert a human-readable filename to 8.3 format.
///
/// `"HELLO.TXT"` → `b"HELLO   TXT"` (space-padded, uppercase).
/// Returns `None` if the name is too long or has no dot.
pub fn to_8_3(name: &str) -> Option<[u8; 11]> {
    let mut result = [b' '; 11];
    let bytes = name.as_bytes();
    let dot = bytes.iter().position(|&b| b == b'.')?;
    if dot > 8 || bytes.len() - dot - 1 > 3 {
        return None;
    }
    for (i, &b) in bytes[..dot].iter().enumerate() {
        result[i] = b.to_ascii_uppercase();
    }
    for (i, &b) in bytes[dot + 1..].iter().enumerate() {
        result[8 + i] = b.to_ascii_uppercase();
    }
    Some(result)
}

// ---------------------------------------------------------------------------
// File reading
// ---------------------------------------------------------------------------

/// Read `buf.len()` bytes from a file starting at byte `offset`.
///
/// Follows the FAT12 cluster chain from the file's first cluster,
/// skipping clusters until `offset` is reached, then reading data
/// into `buf` across cluster boundaries as needed.
///
/// Returns the number of bytes actually read (less than `buf.len()` if
/// the file is shorter than `offset + buf.len()`).
///
/// The [`FileRef`] is not consumed — reuse it for multiple reads at
/// different offsets without re-scanning the directory.
pub async fn read_file(file: &FileRef, offset: u32, buf: &mut [u8]) -> Result<usize, FatError> {
    let params = read_params().await?;
    let cluster_bytes = params.cluster_bytes();

    let remaining = file.size.saturating_sub(offset) as usize;
    let to_read = buf.len().min(remaining);
    if to_read == 0 {
        return Ok(0);
    }

    // Walk the cluster chain to skip past `offset` bytes.
    // Each cluster holds `cluster_bytes` bytes of file data.
    let mut cluster = file.first_cluster;
    let mut skip = offset;
    while skip >= cluster_bytes {
        cluster = next_cluster(&params, cluster)
            .await?
            .ok_or(FatError::Corrupt)?;
        skip -= cluster_bytes;
    }

    // Read data from the current cluster (starting at `skip` offset within it),
    // then follow the chain to subsequent clusters until `to_read` bytes are read.
    let mut bytes_read = 0usize;
    while bytes_read < to_read {
        let addr = params.cluster_addr(cluster) + skip;
        let chunk = (to_read - bytes_read).min((cluster_bytes - skip) as usize);
        flash::read(addr, &mut buf[bytes_read..bytes_read + chunk])
            .await
            .map_err(|_| FatError::FlashError)?;
        bytes_read += chunk;
        skip = 0; // only the first cluster has a skip offset

        if bytes_read < to_read {
            cluster = next_cluster(&params, cluster)
                .await?
                .ok_or(FatError::Corrupt)?;
        }
    }

    Ok(bytes_read)
}

// ---------------------------------------------------------------------------
// Format — create a fresh FAT12 filesystem
// ---------------------------------------------------------------------------

/// Format the FAT partition as FAT12 with a per-badge unique volume label
/// (`CYBR` + 4 hex chars of the MCU device ID) and serial.
///
/// Per-badge uniqueness matters when multiple badges are plugged in
/// concurrently for mass-flashing: `udisks2` picks the mount point from
/// the label (or falls back to the serial), so identical values across
/// badges collide.
///
/// Matches the geometry produced by `mkfs.fat -F 12` on a 1 MiB device:
///   512 bytes/sector, 4 sectors/cluster (2 KiB), 2 FATs × 2 sectors,
///   512 root directory entries (32 sectors), 502 data clusters.
///
/// Erases and writes sectors 0–36 (boot sector, 2 FATs, root directory).
/// The data region is left as-is (erased flash = 0xFF = free clusters).
pub async fn format() -> Result<(), FatError> {
    // Per-badge unique identifiers derived from the MCU device ID.
    // Label: "CYBR" + 4 hex chars of device ID (e.g., "CYBRA3F7   ").
    // Serial: device ID in the low 16 bits so OS shows e.g. "0000-A3F7".
    let id_hex = crate::fw::device_id::get_bytes(); // 4 ASCII hex chars
    let [id0, id1] = crate::fw::device_id::get();
    let mut label = [b' '; 11];
    label[..4].copy_from_slice(b"CYBR");
    label[4..8].copy_from_slice(&id_hex);
    let serial = u32::from_le_bytes([id0, id1, 0x00, 0x00]);

    // -- Boot sector (sector 0) ------------------------------------------
    let mut boot = [0u8; 512];

    // Jump instruction + NOP (required for a valid FAT boot sector).
    boot[0] = 0xEB;
    boot[1] = 0x3C;
    boot[2] = 0x90;
    // OEM name.
    boot[3..11].copy_from_slice(b"mkfs.fat");
    // BPB (BIOS Parameter Block).
    boot[11..13].copy_from_slice(&512u16.to_le_bytes()); // bytes per sector
    boot[13] = 4; // sectors per cluster
    boot[14..16].copy_from_slice(&1u16.to_le_bytes()); // reserved sectors
    boot[16] = 2; // number of FATs
    boot[17..19].copy_from_slice(&512u16.to_le_bytes()); // root entry count
    boot[19..21].copy_from_slice(&2048u16.to_le_bytes()); // total sectors (1 MiB / 512)
    boot[21] = 0xF8; // media descriptor (hard disk)
    boot[22..24].copy_from_slice(&2u16.to_le_bytes()); // sectors per FAT
    boot[24..26].copy_from_slice(&2u16.to_le_bytes()); // sectors per track (dummy)
    boot[26..28].copy_from_slice(&1u16.to_le_bytes()); // number of heads (dummy)
    // Extended boot record (FAT12/16).
    boot[36] = 0x80; // drive number
    boot[38] = 0x29; // extended boot signature
    boot[39..43].copy_from_slice(&serial.to_le_bytes()); // volume serial
    boot[43..54].copy_from_slice(&label); // volume label (11 bytes)
    boot[54..62].copy_from_slice(b"FAT12   "); // filesystem type
    // Boot signature.
    boot[510] = 0x55;
    boot[511] = 0xAA;

    // Erase and write boot sector.
    flash::erase(flash::FAT_OFFSET)
        .await
        .map_err(|_| FatError::FlashError)?;
    flash::write(flash::FAT_OFFSET, &boot)
        .await
        .map_err(|_| FatError::FlashError)?;

    // -- FAT tables (sectors 1–4: FAT1 at sector 1, FAT2 at sector 3) ---
    // Each FAT is 2 sectors (1024 bytes).  First two entries are reserved:
    //   entry 0 = media descriptor (0xFF8), entry 1 = end-of-chain (0xFFF).
    //   Packed as 3 bytes: F8 FF FF.
    let mut fat = [0u8; 1024];
    fat[0] = 0xF8;
    fat[1] = 0xFF;
    fat[2] = 0xFF;
    // Rest is 0x00 = free clusters.

    let fat1_addr = flash::FAT_OFFSET + 512; // sector 1
    let fat2_addr = fat1_addr + 1024; // sector 3

    // Erase the sectors covering both FATs (sectors 1–4 = 2048 bytes = 1 erase page
    // on 4K flash, but our sectors are 512 bytes and erase granularity is 4K;
    // sector 0 already erased above). FAT1 starts at byte 512, FAT2 ends at
    // byte 2560.  That's within the first 4K page (already erased for the boot
    // sector).  But the root dir starts at sector 5 = byte 2560 which spans
    // into the next 4K page. Let's erase page-by-page for the full range:
    // sectors 0–36 = 18944 bytes = 5 pages (0–4).
    for page in 1..5 {
        flash::erase(flash::FAT_OFFSET + page * flash::PAGE_SIZE as u32)
            .await
            .map_err(|_| FatError::FlashError)?;
    }

    flash::write(fat1_addr, &fat)
        .await
        .map_err(|_| FatError::FlashError)?;
    flash::write(fat2_addr, &fat)
        .await
        .map_err(|_| FatError::FlashError)?;

    // -- Root directory (sectors 5–36: 16384 bytes = 512 entries) ----------
    // Erased flash is 0xFF, but the FAT driver interprets 0xFF first-byte
    // entries as occupied.  Zero every root-dir entry so they read as 0x00
    // (end-of-directory / free).  The first 32 bytes hold the volume-label
    // entry — build it into the first sector's buffer up-front so the label
    // bytes are written alongside the zeros in a single flash pass.  Writing
    // the label afterwards would be a no-op on NOR flash (can't flip 0-bits
    // back to 1 without an erase).
    let root_addr = flash::FAT_OFFSET + 5 * 512; // sector 5
    let root_size = 512u32 * 32; // 512 entries × 32 bytes = 16384 bytes

    // First sector: volume-label entry at offset 0, zeros for the rest.
    let mut first_sector = [0u8; 512];
    first_sector[0..11].copy_from_slice(&label);
    first_sector[11] = 0x08; // volume label attribute
    flash::write(root_addr, &first_sector)
        .await
        .map_err(|_| FatError::FlashError)?;

    // Remaining sectors: all zeros.
    let zeros = [0u8; 512];
    for sector in 1..(root_size / 512) {
        flash::write(root_addr + sector * 512, &zeros)
            .await
            .map_err(|_| FatError::FlashError)?;
    }

    defmt::info!(
        "FAT12: formatted partition as {=[u8]:a} (serial {=u32:08x})",
        label,
        serial,
    );
    Ok(())
}

/// Format the partition if it doesn't contain a valid FAT12 filesystem.
///
/// Call at startup before any file operations.  If the boot sector is
/// valid, this is a no-op (one 64-byte flash read).
pub async fn format_if_needed() -> Result<(), FatError> {
    match read_params().await {
        Ok(_) => Ok(()), // already formatted
        Err(FatError::NoFilesystem) => format().await,
        Err(e) => Err(e),
    }
}
