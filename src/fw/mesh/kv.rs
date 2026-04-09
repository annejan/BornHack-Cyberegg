//! Async-safe key-value store backed by ekv on external QSPI flash.
//!
//! One ekv `Database` instance lives as a `'static` singleton; all tasks can
//! call it concurrently because `Database` manages its own internal mutex.
//! Callers obtain a [`KvNamespace`] handle; all keys are prefixed with
//! `"<namespace>:"` so stores from different modules never collide.
//!
//! # Usage
//!
//! ```rust
//! // Once at startup in main:
//! kv::init(p.QSPI, ...).await.expect("QSPI flash not found");
//!
//! // From any async task — free to create on demand, zero cost:
//! let store = kv::namespace("game");
//! store.set("health", &[100u8], true).await?;
//!
//! let mut buf = [0u8; 4];
//! let n = store.get("health", &mut buf).await?;
//! ```

use embassy_nrf::{Peri, peripherals, qspi};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::once_lock::OnceLock;
use ekv::{Config, Database, MountError};
use static_cell::StaticCell;

use super::storage::{KV_PAGE_COUNT, QspiFlash, QspiIrqs, init_qspi};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, defmt::Format, PartialEq)]
pub enum KvError {
    NotFound,
    /// Returned by `set(..., update: false)` when the key already exists.
    KeyExists,
    StoreFull,
    Corrupted,
    NotInitialised,
    BufferTooSmall,
    KeyTooLong,
    Other,
}

// ---------------------------------------------------------------------------
// Singleton Database
// ---------------------------------------------------------------------------

type Db = Database<QspiFlash, CriticalSectionRawMutex>;

static DB_CELL: StaticCell<Db> = StaticCell::new();
static DB: OnceLock<&'static Db> = OnceLock::new();

fn get_db() -> Result<&'static Db, KvError> {
    DB.try_get().copied().ok_or(KvError::NotInitialised)
}

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

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

    let flash = QspiFlash { qspi };

    let mut config = Config::default();
    // Random seed for wear leveling.  We use a fixed value since we don't have
    // an RNG available at init time; any non-zero constant is fine.
    config.random_seed = 0xDEAD_BEEF;

    let db = DB_CELL.init(Database::new(flash, config));

    match db.mount().await {
        Ok(()) => {}
        Err(MountError::Corrupted) => {
            defmt::warn!(
                "KV: store corrupted or not formatted — formatting {} pages",
                KV_PAGE_COUNT
            );
            db.format().await.ok();
            if db.mount().await.is_err() {
                defmt::error!("KV: mount after format failed — resetting");
                cortex_m::peripheral::SCB::sys_reset();
            }
        }
        Err(MountError::Flash(_)) => {
            defmt::error!("KV: flash error during mount — resetting");
            cortex_m::peripheral::SCB::sys_reset();
        }
    }

    DB.init(db).ok();

    defmt::info!("KV store ready ({} KiB, {} pages × 4 KiB)", KV_PAGE_COUNT * 4, KV_PAGE_COUNT);
    Ok(())
}

// ---------------------------------------------------------------------------
// Erase and reset
// ---------------------------------------------------------------------------

/// Format the KV store and trigger a system reset.
///
/// Call when persistent flash corruption is detected at runtime.  The firmware
/// restarts with a clean store on the next boot.
pub async fn wipe_and_reset() -> ! {
    defmt::error!("KV: wiping store and resetting");
    if let Ok(db) = get_db() {
        db.format().await.ok();
    }
    cortex_m::peripheral::SCB::sys_reset()
}

// ---------------------------------------------------------------------------
// Namespaced key derivation
// ---------------------------------------------------------------------------

/// Build `"<namespace>:<key>"` as a stack-allocated byte string.
///
/// Maximum combined length is 63 bytes (namespace + ':' + key ≤ 63).
/// Returns `None` if the combined key would exceed that limit.
fn namespaced_key<'a>(namespace: &str, key: &str, buf: &'a mut [u8; 64]) -> Option<&'a [u8]> {
    let total = namespace.len() + 1 + key.len();
    if total > 63 {
        return None;
    }
    let nb = namespace.as_bytes();
    let kb = key.as_bytes();
    buf[..nb.len()].copy_from_slice(nb);
    buf[nb.len()] = b':';
    buf[nb.len() + 1..nb.len() + 1 + kb.len()].copy_from_slice(kb);
    Some(&buf[..total])
}

// ---------------------------------------------------------------------------
// KvNamespace — the public handle callers use
// ---------------------------------------------------------------------------

/// A lightweight namespaced handle to the KV store.
///
/// All keys are transparently prefixed with `"<namespace>:"` before storage,
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
        let mut kbuf = [0u8; 64];
        let k = namespaced_key(self.prefix, key, &mut kbuf).ok_or(KvError::KeyTooLong)?;
        let db = get_db()?;
        let rtx = db.read_transaction().await;
        match rtx.read(k, buf).await {
            Ok(n) => Ok(n),
            Err(ekv::ReadError::KeyNotFound) => Err(KvError::NotFound),
            Err(ekv::ReadError::BufferTooSmall) => Err(KvError::BufferTooSmall),
            Err(ekv::ReadError::KeyTooBig) => Err(KvError::KeyTooLong),
            Err(ekv::ReadError::Corrupted) => Err(KvError::Corrupted),
            Err(ekv::ReadError::Flash(_)) => Err(KvError::Other),
        }
    }

    /// Write `data` under `key`.
    ///
    /// - `update: true`  — create if absent, overwrite if it exists.
    /// - `update: false` — create only; returns [`KvError::KeyExists`] if the
    ///                     key is already present.
    pub async fn set(&self, key: &str, data: &[u8], update: bool) -> Result<(), KvError> {
        let mut kbuf = [0u8; 64];
        let k = namespaced_key(self.prefix, key, &mut kbuf).ok_or(KvError::KeyTooLong)?;
        let db = get_db()?;

        if !update {
            // Check existence first; return KeyExists if found.
            let rtx = db.read_transaction().await;
            match rtx.read(k, &mut []).await {
                Ok(_) | Err(ekv::ReadError::BufferTooSmall) => return Err(KvError::KeyExists),
                Err(ekv::ReadError::KeyNotFound) => {}
                Err(_) => {}
            }
        }

        let mut wtx = db.write_transaction().await;
        match wtx.write(k, data).await {
            Ok(()) => {}
            Err(ekv::WriteError::Full) => return Err(KvError::StoreFull),
            Err(ekv::WriteError::Corrupted) => return Err(KvError::Corrupted),
            Err(_) => return Err(KvError::Other),
        }
        match wtx.commit().await {
            Ok(()) => Ok(()),
            Err(ekv::CommitError::Corrupted) => Err(KvError::Corrupted),
            Err(_) => Err(KvError::Other),
        }
    }

    /// Delete the value for `key`.  Returns `Ok(())` even if the key did not exist.
    pub async fn delete(&self, key: &str) -> Result<(), KvError> {
        let mut kbuf = [0u8; 64];
        let k = namespaced_key(self.prefix, key, &mut kbuf).ok_or(KvError::KeyTooLong)?;
        let db = get_db()?;
        let mut wtx = db.write_transaction().await;
        match wtx.delete(k).await {
            Ok(()) => {}
            Err(ekv::WriteError::Corrupted) => return Err(KvError::Corrupted),
            Err(_) => return Err(KvError::Other),
        }
        match wtx.commit().await {
            Ok(()) => Ok(()),
            Err(ekv::CommitError::Corrupted) => Err(KvError::Corrupted),
            Err(_) => Err(KvError::Other),
        }
    }

    /// Returns `true` if the key exists in the store.
    pub async fn exists(&self, key: &str) -> Result<bool, KvError> {
        let mut kbuf = [0u8; 64];
        let k = namespaced_key(self.prefix, key, &mut kbuf).ok_or(KvError::KeyTooLong)?;
        let db = get_db()?;
        let rtx = db.read_transaction().await;
        match rtx.read(k, &mut []).await {
            Ok(_) | Err(ekv::ReadError::BufferTooSmall) => Ok(true),
            Err(ekv::ReadError::KeyNotFound) => Ok(false),
            Err(ekv::ReadError::Corrupted) => Err(KvError::Corrupted),
            Err(_) => Err(KvError::Other),
        }
    }
}

/// Obtain a namespaced handle to the KV store.
///
/// Free to call at any time — creates a zero-cost handle with no allocation.
/// The KV store must be initialised with [`init()`] before any operations
/// are issued through the returned handle.
pub fn namespace(prefix: &'static str) -> KvNamespace {
    KvNamespace { prefix }
}

/// Write a known value and read it back to confirm the KV store is functional.
///
/// Call once at startup after [`init()`].
pub async fn smoke_test() {
    const MAGIC: [u8; 4] = [0xCA, 0xFE, 0xBA, 0xBE];
    let kv = namespace("_test");

    if let Err(e) = kv.set("smoke", &MAGIC, true).await {
        defmt::warn!("KV smoke test: write failed: {:?}", e);
        return;
    }

    let mut buf = [0u8; 4];
    match kv.get("smoke", &mut buf).await {
        Ok(n) if n == MAGIC.len() && buf == MAGIC => {
            defmt::info!("KV smoke test OK");
        }
        Ok(n) => {
            defmt::warn!(
                "KV smoke test: read back {} bytes, got {:02x} expected {:02x}",
                n,
                buf,
                MAGIC
            );
        }
        Err(e) => {
            defmt::warn!("KV smoke test: read failed: {:?}", e);
        }
    }
}
