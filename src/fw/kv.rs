//! Async-safe key-value store backed by TicKV on external QSPI flash.
//!
//! One TicKV instance lives behind an async Mutex so multiple tasks can share
//! it safely.  Callers obtain a [`KvNamespace`] handle; all keys are prefixed
//! with `"<namespace>:"` before hashing so stores from different modules never
//! collide even if they use the same key string.
//!
//! # Usage
//!
//! ```rust
//! // Once at startup in main:
//! kv::init(p.QSPI, ...).await.expect("QSPI flash not found");
//!
//! // From any async task — free to create on demand, zero cost:
//! let store = kv::namespace("game");
//! store.set("health", &[100u8]).await?;
//!
//! let mut buf = [0u8; 4];
//! let n = store.get("health", &mut buf).await?;
//! ```

use embassy_nrf::{Peri, peripherals, qspi};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use static_cell::StaticCell;
use tickv::{ErrorCode, TicKV};

use crate::fw::storage::{AlignedBuf, QspiFlashController, QspiIrqs, REGION_SIZE, fnv1a, init_qspi};

// ---------------------------------------------------------------------------
// Flash layout
// ---------------------------------------------------------------------------

/// Number of 4 KiB TicKV regions reserved for the KV store (256 KiB total).
///
/// Using the full chip (512 regions) is tempting but the erase-all recovery
/// path (triggered on a MAIN_KEY version bump) would block the executor for
/// ~30 s at 60 ms/sector, firing the 5-second watchdog.  64 regions × 60 ms
/// = ~4 s typical; the watchdog is fed between sectors (see init) so even the
/// 200 ms/sector worst case is safe.  Expand if storage needs grow.
const NUM_REGIONS: usize = 64;

/// Seed key that identifies this firmware's KV schema version.
/// Change this string when the on-flash layout becomes incompatible with an
/// older firmware; the store will be erased and re-initialised on next boot.
const MAIN_KEY: u64 = fnv1a(b"cyberaegg_kv_v1");

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, defmt::Format, PartialEq)]
pub enum KvError {
    NotFound,
    /// Returned by `set(..., update: false)` when the key already exists.
    KeyExists,
    StoreFull,
    WriteFail,
    ReadFail,
    EraseFail,
    NotInitialised,
    BufferTooSmall,
    Other,
}

impl From<ErrorCode> for KvError {
    fn from(e: ErrorCode) -> Self {
        match e {
            ErrorCode::KeyNotFound => KvError::NotFound,
            ErrorCode::BufferTooSmall(_) => KvError::BufferTooSmall,
            ErrorCode::FlashFull | ErrorCode::RegionFull => KvError::StoreFull,
            ErrorCode::WriteFail => KvError::WriteFail,
            ErrorCode::ReadFail => KvError::ReadFail,
            ErrorCode::EraseFail => KvError::EraseFail,
            _ => KvError::Other,
        }
    }
}

// ---------------------------------------------------------------------------
// KvStore — thin wrapper around TicKV
// ---------------------------------------------------------------------------

struct KvStore {
    tickv: TicKV<'static, QspiFlashController, REGION_SIZE>,
}

// Safety: KvStore is only accessed through the async Mutex, never concurrently.
unsafe impl Send for KvStore {}

impl KvStore {
    fn get(&mut self, key: u64, buf: &mut [u8]) -> Result<usize, KvError> {
        match self.tickv.get_key(key, buf) {
            Ok((_code, len)) => Ok(len),
            Err(e) => Err(e.into()),
        }
    }

    fn set(&mut self, key: u64, data: &[u8], update: bool) -> Result<(), KvError> {
        // Probe existence: get_key with a zero-length buffer returns KeyNotFound
        // only when the key is absent; any other result means the key exists.
        let exists = !matches!(
            self.tickv.get_key(key, &mut []),
            Err(ErrorCode::KeyNotFound)
        );

        if exists && !update {
            return Err(KvError::KeyExists);
        }

        // Invalidate any existing entry before appending the new one.
        if exists {
            match self.tickv.invalidate_key(key) {
                Ok(_) | Err(ErrorCode::KeyNotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        self.tickv.append_key(key, data).map(|_| ()).map_err(Into::into)
    }

    fn delete(&mut self, key: u64) -> Result<(), KvError> {
        match self.tickv.invalidate_key(key) {
            Ok(_) | Err(ErrorCode::KeyNotFound) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Singleton
// ---------------------------------------------------------------------------

static STORE: Mutex<CriticalSectionRawMutex, Option<KvStore>> = Mutex::new(None);

/// Initialise the KV store.  Call once from the main task before spawning
/// any task that uses [`namespace()`].
///
/// Returns `Err([u8; 3])` with the raw JEDEC ID bytes if the QSPI flash chip
/// cannot be reached (all-0xFF = no device, all-0x00 = bus fault).
pub async fn init<'d>(
    qspi_periph: Peri<'d, peripherals::QSPI>,
    sck: Peri<'d, peripherals::P0_21>,
    csn: Peri<'d, peripherals::P0_25>,
    io0: Peri<'d, peripherals::P0_20>,
    io1: Peri<'d, peripherals::P0_24>,
    io2: Peri<'d, peripherals::P0_22>,
    io3: Peri<'d, peripherals::P0_23>,
) -> Result<(), [u8; 3]> {
    let qspi = init_qspi(qspi_periph, QspiIrqs, sck, csn, io0, io1, io2, io3)?;

    // Safety: init() is called from main() which never returns, so 'static is valid.
    let qspi: qspi::Qspi<'static> = unsafe { core::mem::transmute(qspi) };

    static TICKV_BUF: StaticCell<AlignedBuf> = StaticCell::new();
    let buf = TICKV_BUF.init(AlignedBuf([0u8; REGION_SIZE]));

    let store = KvStore {
        tickv: TicKV::new(QspiFlashController::new(qspi), &mut buf.0, NUM_REGIONS * REGION_SIZE),
    };

    match store.tickv.initialise(MAIN_KEY) {
        Ok(_) | Err(ErrorCode::KeyNotFound) => {}
        Err(ErrorCode::UnsupportedVersion) => {
            defmt::warn!("KV store: incompatible schema, erasing {} regions", NUM_REGIONS);
            for r in 0..NUM_REGIONS {
                let _ = <QspiFlashController as tickv::FlashController<REGION_SIZE>>::erase_region(
                    &store.tickv.controller,
                    r,
                );
                // The erase loop is synchronous and blocks the executor.
                // Feed watchdog channel 0 every sector so it never expires.
                embassy_nrf::pac::WDT
                    .rr(0)
                    .write(|w| w.set_rr(embassy_nrf::pac::wdt::vals::Rr::RELOAD));
            }
            match store.tickv.initialise(MAIN_KEY) {
                Ok(_) | Err(ErrorCode::KeyNotFound) => {}
                Err(e) => defmt::warn!("KV store re-init failed: {:?}", defmt::Debug2Format(&e)),
            }
        }
        Err(e) => defmt::warn!("KV store init: {:?}", defmt::Debug2Format(&e)),
    }

    *STORE.lock().await = Some(store);
    defmt::info!(
        "KV store ready ({} KiB, {} regions)",
        NUM_REGIONS * REGION_SIZE / 1024,
        NUM_REGIONS
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Namespaced key derivation
// ---------------------------------------------------------------------------

/// Hash `"<namespace>:<key>"` in one FNV-1a pass with no heap allocation.
fn namespaced_key(namespace: &str, key: &str) -> u64 {
    const OFFSET: u64 = 14_695_981_039_346_656_037;
    const PRIME: u64 = 1_099_511_628_211;
    let mut h = OFFSET;
    for &b in namespace.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h ^= b':' as u64;
    h = h.wrapping_mul(PRIME);
    for &b in key.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

// ---------------------------------------------------------------------------
// KvNamespace — the public handle callers use
// ---------------------------------------------------------------------------

/// A lightweight namespaced handle to the KV store.
///
/// All keys are transparently prefixed with `"<namespace>:"` before hashing,
/// so modules never step on each other's keys.  Cheap to copy and recreate —
/// just a pointer to a static string.
///
/// Obtain one with [`namespace()`].
#[derive(Clone, Copy)]
pub struct KvNamespace {
    prefix: &'static str,
}

impl KvNamespace {
    /// Read the value for `key` into `buf`.
    /// Returns the number of bytes written on success.
    pub async fn get(&self, key: &str, buf: &mut [u8]) -> Result<usize, KvError> {
        STORE
            .lock()
            .await
            .as_mut()
            .ok_or(KvError::NotInitialised)?
            .get(namespaced_key(self.prefix, key), buf)
    }

    /// Write `data` under `key`.
    ///
    /// - `update: true`  — create the record if absent, overwrite if it exists.
    /// - `update: false` — create only; returns [`KvError::KeyExists`] if the
    ///                     key is already present.
    pub async fn set(&self, key: &str, data: &[u8], update: bool) -> Result<(), KvError> {
        STORE
            .lock()
            .await
            .as_mut()
            .ok_or(KvError::NotInitialised)?
            .set(namespaced_key(self.prefix, key), data, update)
    }

    /// Delete the value for `key`.  Returns `Ok(())` even if the key did not exist.
    pub async fn delete(&self, key: &str) -> Result<(), KvError> {
        STORE
            .lock()
            .await
            .as_mut()
            .ok_or(KvError::NotInitialised)?
            .delete(namespaced_key(self.prefix, key))
    }
}

/// Obtain a namespaced handle to the KV store.
///
/// This is free to call at any time — it creates a zero-cost handle with no
/// allocation.  The KV store must be initialised with [`init()`] before any
/// operations are issued through the returned handle.
pub fn namespace(prefix: &'static str) -> KvNamespace {
    KvNamespace { prefix }
}
