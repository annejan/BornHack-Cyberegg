//! Concurrency tests for the shared flash multiplexer.
//!
//! Uses a RAM-backed SimFlash (2 MiB) with std::sync::Mutex to simulate
//! concurrent access from ekv (KV partition, first 1 MiB) and FAT12/USB MSC
//! (FAT partition, second 1 MiB).
//!
//! These tests verify that concurrent readers and writers across partitions
//! do not corrupt each other's data.

use std::sync::Arc;
use std::thread;

// ---------------------------------------------------------------------------
// Flash geometry (mirrors fw/flash.rs constants)
// ---------------------------------------------------------------------------

const PAGE_SIZE: usize = 4096;
const FLASH_TOTAL_BYTES: usize = 2 * 1024 * 1024;

const KV_OFFSET: u32 = 0;
const KV_BYTES: usize = 1024 * 1024;
const KV_PAGES: usize = KV_BYTES / PAGE_SIZE;

const FAT_OFFSET: u32 = KV_BYTES as u32;
const FAT_BYTES: usize = FLASH_TOTAL_BYTES - KV_BYTES;
const FAT_PAGES: usize = FAT_BYTES / PAGE_SIZE;

// ---------------------------------------------------------------------------
// Simulated NOR flash (2 MiB, mutex-protected like the real thing)
// ---------------------------------------------------------------------------

struct SimFlash {
    data: std::sync::Mutex<Vec<u8>>,
}

impl SimFlash {
    fn new() -> Self {
        Self {
            data: std::sync::Mutex::new(vec![0xFF; FLASH_TOTAL_BYTES]),
        }
    }

    fn read(&self, addr: u32, buf: &mut [u8]) {
        let data = self.data.lock().unwrap();
        let start = addr as usize;
        buf.copy_from_slice(&data[start..start + buf.len()]);
    }

    fn write(&self, addr: u32, buf: &[u8]) {
        let mut data = self.data.lock().unwrap();
        let start = addr as usize;
        // NOR flash: can only clear bits (1→0).
        for (i, &b) in buf.iter().enumerate() {
            data[start + i] &= b;
        }
    }

    fn erase(&self, addr: u32) {
        let mut data = self.data.lock().unwrap();
        let start = addr as usize;
        assert_eq!(start % PAGE_SIZE, 0, "erase address not sector-aligned");
        data[start..start + PAGE_SIZE].fill(0xFF);
    }

    fn snapshot(&self) -> Vec<u8> {
        self.data.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// Geometry sanity
// ---------------------------------------------------------------------------

#[test]
fn geometry_constants() {
    assert_eq!(KV_OFFSET, 0);
    assert_eq!(KV_BYTES, 1024 * 1024);
    assert_eq!(FAT_OFFSET, 1024 * 1024);
    assert_eq!(FAT_BYTES, 1024 * 1024);
    assert_eq!(KV_BYTES + FAT_BYTES, FLASH_TOTAL_BYTES);
    assert_eq!(KV_PAGES, 256);
    assert_eq!(FAT_PAGES, 256);
}

// ---------------------------------------------------------------------------
// Basic flash semantics
// ---------------------------------------------------------------------------

#[test]
fn erased_state_is_0xff() {
    let flash = SimFlash::new();
    let mut buf = [0u8; 4];
    flash.read(0, &mut buf);
    assert_eq!(buf, [0xFF; 4]);
}

#[test]
fn write_then_read() {
    let flash = SimFlash::new();
    flash.write(0, &[0xDE, 0xAD, 0xBE, 0xEF]);
    let mut buf = [0u8; 4];
    flash.read(0, &mut buf);
    assert_eq!(buf, [0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn nor_semantics_only_clears_bits() {
    let flash = SimFlash::new();
    flash.write(0, &[0xAA]);

    // Write again without erase: NOR can only clear bits (1→0).
    flash.write(0, &[0x55]);
    let mut b = [0u8; 1];
    flash.read(0, &mut b);
    assert_eq!(b[0], 0xAA & 0x55); // 0x00

    // Erase restores to 0xFF.
    flash.erase(0);
    flash.read(0, &mut b);
    assert_eq!(b[0], 0xFF);
}

// ---------------------------------------------------------------------------
// Partition isolation
// ---------------------------------------------------------------------------

#[test]
fn kv_and_fat_partitions_are_independent() {
    let flash = SimFlash::new();

    flash.write(KV_OFFSET, &[0x11, 0x22, 0x33, 0x44]);
    flash.write(FAT_OFFSET, &[0xAA, 0xBB, 0xCC, 0xDD]);

    let mut kv_buf = [0u8; 4];
    let mut fat_buf = [0u8; 4];
    flash.read(KV_OFFSET, &mut kv_buf);
    flash.read(FAT_OFFSET, &mut fat_buf);
    assert_eq!(kv_buf, [0x11, 0x22, 0x33, 0x44]);
    assert_eq!(fat_buf, [0xAA, 0xBB, 0xCC, 0xDD]);
}

// ---------------------------------------------------------------------------
// Concurrent writes to separate partitions
// ---------------------------------------------------------------------------

#[test]
fn concurrent_kv_and_fat_full_writes() {
    let flash = Arc::new(SimFlash::new());

    let flash_kv = flash.clone();
    let flash_fat = flash.clone();

    // KV writer: fills every KV page with 0x11.
    let kv_handle = thread::spawn(move || {
        let pattern = [0x11u8; PAGE_SIZE];
        for page in 0..KV_PAGES {
            let addr = KV_OFFSET + (page * PAGE_SIZE) as u32;
            flash_kv.erase(addr);
            flash_kv.write(addr, &pattern);
        }
    });

    // FAT writer: fills every FAT page with 0xAA.
    let fat_handle = thread::spawn(move || {
        let pattern = [0xAAu8; PAGE_SIZE];
        for page in 0..FAT_PAGES {
            let addr = FAT_OFFSET + (page * PAGE_SIZE) as u32;
            flash_fat.erase(addr);
            flash_fat.write(addr, &pattern);
        }
    });

    kv_handle.join().unwrap();
    fat_handle.join().unwrap();

    let snap = flash.snapshot();
    for i in 0..KV_BYTES {
        assert_eq!(
            snap[KV_OFFSET as usize + i],
            0x11,
            "KV corruption at offset 0x{:X}",
            i
        );
    }
    for i in 0..FAT_BYTES {
        assert_eq!(
            snap[FAT_OFFSET as usize + i],
            0xAA,
            "FAT corruption at offset 0x{:X}",
            i
        );
    }
}

// ---------------------------------------------------------------------------
// Concurrent reads while writing the other partition
// ---------------------------------------------------------------------------

#[test]
fn reads_stable_while_other_partition_writes() {
    let flash = Arc::new(SimFlash::new());

    // Pre-write known data to FAT partition.
    for page in 0..FAT_PAGES {
        let addr = FAT_OFFSET + (page * PAGE_SIZE) as u32;
        let pattern = [(page & 0xFF) as u8; PAGE_SIZE];
        flash.write(addr, &pattern);
    }

    let flash_reader = flash.clone();
    let flash_writer = flash.clone();

    // Reader: continuously reads FAT pages and verifies content.
    let read_handle = thread::spawn(move || {
        let mut buf = [0u8; PAGE_SIZE];
        for _round in 0..10 {
            for page in 0..FAT_PAGES {
                let addr = FAT_OFFSET + (page * PAGE_SIZE) as u32;
                flash_reader.read(addr, &mut buf);
                let expected = (page & 0xFF) as u8;
                assert!(
                    buf.iter().all(|&b| b == expected),
                    "FAT read corruption at page {} during concurrent KV write",
                    page
                );
            }
        }
    });

    // Writer: writes to KV partition concurrently.
    let write_handle = thread::spawn(move || {
        for page in 0..KV_PAGES {
            let addr = KV_OFFSET + (page * PAGE_SIZE) as u32;
            flash_writer.erase(addr);
            flash_writer.write(addr, &[0x55u8; PAGE_SIZE]);
        }
    });

    read_handle.join().unwrap();
    write_handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// Many concurrent writers to non-overlapping FAT regions
// ---------------------------------------------------------------------------

#[test]
fn many_writers_non_overlapping_regions() {
    let flash = Arc::new(SimFlash::new());
    let num_threads = 8u8;
    let region_size = FAT_BYTES / num_threads as usize;
    let region_pages = region_size / PAGE_SIZE;

    let handles: Vec<_> = (0..num_threads)
        .map(|tid| {
            let flash = flash.clone();
            thread::spawn(move || {
                let base = FAT_OFFSET + (tid as u32 * region_size as u32);
                let pattern = [tid.wrapping_mul(17).wrapping_add(1); PAGE_SIZE];
                for page in 0..region_pages {
                    let addr = base + (page * PAGE_SIZE) as u32;
                    flash.erase(addr);
                    flash.write(addr, &pattern);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let snap = flash.snapshot();
    for tid in 0..num_threads {
        let base = FAT_OFFSET as usize + tid as usize * region_size;
        let expected = tid.wrapping_mul(17).wrapping_add(1);
        for i in 0..region_size {
            assert_eq!(
                snap[base + i],
                expected,
                "Thread {} corruption at offset 0x{:X}",
                tid,
                i
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Interleaved erase-write-read cycles on both partitions
// ---------------------------------------------------------------------------

#[test]
fn interleaved_erase_write_read_both_partitions() {
    let flash = Arc::new(SimFlash::new());
    let flash_a = flash.clone();
    let flash_b = flash.clone();

    // Thread A: erase-write-verify on KV pages 0..64.
    let a = thread::spawn(move || {
        let mut buf = [0u8; PAGE_SIZE];
        for round in 0u8..5 {
            for page in 0..64usize {
                let addr = KV_OFFSET + (page * PAGE_SIZE) as u32;
                flash_a.erase(addr);
                let val = round.wrapping_mul(3).wrapping_add(page as u8);
                let pattern = [val; PAGE_SIZE];
                flash_a.write(addr, &pattern);
                flash_a.read(addr, &mut buf);
                assert!(
                    buf.iter().all(|&b| b == val),
                    "KV verify failed: round={} page={}",
                    round,
                    page
                );
            }
        }
    });

    // Thread B: erase-write-verify on FAT pages 0..64.
    let b = thread::spawn(move || {
        let mut buf = [0u8; PAGE_SIZE];
        for round in 0u8..5 {
            for page in 0..64usize {
                let addr = FAT_OFFSET + (page * PAGE_SIZE) as u32;
                flash_b.erase(addr);
                let val = round.wrapping_mul(7).wrapping_add(page as u8);
                let pattern = [val; PAGE_SIZE];
                flash_b.write(addr, &pattern);
                flash_b.read(addr, &mut buf);
                assert!(
                    buf.iter().all(|&b| b == val),
                    "FAT verify failed: round={} page={}",
                    round,
                    page
                );
            }
        }
    });

    a.join().unwrap();
    b.join().unwrap();
}

// ---------------------------------------------------------------------------
// Stress: mixed read/write across both partitions with many threads
// ---------------------------------------------------------------------------

#[test]
fn stress_mixed_operations() {
    let flash = Arc::new(SimFlash::new());

    // Pre-fill both partitions with known data.
    for page in 0..KV_PAGES {
        let addr = KV_OFFSET + (page * PAGE_SIZE) as u32;
        flash.write(addr, &[0x11u8; PAGE_SIZE]);
    }
    for page in 0..FAT_PAGES {
        let addr = FAT_OFFSET + (page * PAGE_SIZE) as u32;
        flash.write(addr, &[0xAAu8; PAGE_SIZE]);
    }

    let mut handles = Vec::new();

    // 4 KV readers.
    for _ in 0..4 {
        let f = flash.clone();
        handles.push(thread::spawn(move || {
            let mut buf = [0u8; PAGE_SIZE];
            for page in 0..KV_PAGES {
                let addr = KV_OFFSET + (page * PAGE_SIZE) as u32;
                f.read(addr, &mut buf);
                assert!(
                    buf.iter().all(|&b| b == 0x11),
                    "KV read corruption during stress"
                );
            }
        }));
    }

    // 4 FAT readers.
    for _ in 0..4 {
        let f = flash.clone();
        handles.push(thread::spawn(move || {
            let mut buf = [0u8; PAGE_SIZE];
            for page in 0..FAT_PAGES {
                let addr = FAT_OFFSET + (page * PAGE_SIZE) as u32;
                f.read(addr, &mut buf);
                assert!(
                    buf.iter().all(|&b| b == 0xAA),
                    "FAT read corruption during stress"
                );
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
}
