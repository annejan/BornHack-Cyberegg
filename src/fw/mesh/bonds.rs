//! BLE bond persistence via the shared KV store.
//!
//! All bond data is stored under a single key `"bonds:all"` (namespace
//! `"bonds"`, key `"all"`) as a flat array of fixed-size 42-byte records — one
//! per bonded peer. No separate index is needed; loading reads the whole array,
//! saving rewrites it.
//!
//! `bond_task` owns a `BondStore` and services [`BondCmd`] messages from the
//! BLE task.  The KV store must be initialised with `kv::init()` before this
//! task is spawned.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::once_lock::OnceLock;
use heapless::Vec;
use trouble_host::prelude::{BdAddr, SecurityLevel};
use trouble_host::{BondInformation, Identity, IdentityResolvingKey, LongTermKey};

use crate::fw::kv::{self, KvError, KvNamespace};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_BONDS: usize = 4;

// ---------------------------------------------------------------------------
// BondInformation serialization (fixed 42-byte layout)
// ---------------------------------------------------------------------------
//
//  [0..16]  LTK (u128 little-endian)
//  [16..22] bd_addr (6 bytes)
//  [22]     irk_present (0 or 1)
//  [23..39] IRK (u128 little-endian, zeroed if absent)
//  [39]     is_bonded (0 or 1)
//  [40]     security_level (0=NoEncryption, 1=Encrypted,
// 2=EncryptedAuthenticated)  [41]     reserved

const BOND_SIZE: usize = 42;

fn security_level_to_u8(sl: &SecurityLevel) -> u8 {
    match sl {
        SecurityLevel::NoEncryption => 0,
        SecurityLevel::Encrypted => 1,
        SecurityLevel::EncryptedAuthenticated => 2,
    }
}

fn security_level_from_u8(b: u8) -> SecurityLevel {
    match b {
        1 => SecurityLevel::Encrypted,
        2 => SecurityLevel::EncryptedAuthenticated,
        _ => SecurityLevel::NoEncryption,
    }
}

fn serialize_bond(info: &BondInformation) -> [u8; BOND_SIZE] {
    let mut buf = [0u8; BOND_SIZE];
    buf[0..16].copy_from_slice(&info.ltk.0.to_le_bytes());
    let addr = info.identity.bd_addr.into_inner();
    buf[16..22].copy_from_slice(&addr);
    if let Some(irk) = info.identity.irk {
        buf[22] = 1;
        buf[23..39].copy_from_slice(&irk.0.to_le_bytes());
    }
    buf[39] = info.is_bonded as u8;
    buf[40] = security_level_to_u8(&info.security_level);
    buf
}

fn deserialize_bond(buf: &[u8; BOND_SIZE]) -> BondInformation {
    let ltk = LongTermKey(u128::from_le_bytes(buf[0..16].try_into().unwrap()));
    let mut addr = [0u8; 6];
    addr.copy_from_slice(&buf[16..22]);
    let irk = if buf[22] != 0 {
        Some(IdentityResolvingKey(u128::from_le_bytes(
            buf[23..39].try_into().unwrap(),
        )))
    } else {
        None
    };
    let identity = Identity {
        bd_addr: BdAddr::new(addr),
        irk,
    };
    BondInformation::new(identity, ltk, security_level_from_u8(buf[40]), buf[39] != 0)
}

// ---------------------------------------------------------------------------
// BondStore
// ---------------------------------------------------------------------------

struct BondStore {
    kv: KvNamespace,
}

impl BondStore {
    fn new() -> Self {
        Self {
            kv: kv::namespace("bonds"),
        }
    }

    /// Read all bonds from `"bonds:all"`.
    async fn load_all(&self) -> Vec<BondInformation, MAX_BONDS> {
        let mut buf = [0u8; MAX_BONDS * BOND_SIZE];
        let n = match self.kv.get("all", &mut buf).await {
            Ok(n) => n,
            Err(KvError::NotFound) => return Vec::new(),
            Err(e) => {
                defmt::warn!("BondStore: load: {:?}", e);
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for chunk in buf[..n].chunks_exact(BOND_SIZE) {
            let arr: &[u8; BOND_SIZE] = chunk.try_into().unwrap();
            let _ = out.push(deserialize_bond(arr));
        }
        out
    }

    /// Write `bonds` back to `"bonds:all"` as a flat array.
    async fn store_all(&self, bonds: &Vec<BondInformation, MAX_BONDS>) {
        let mut buf = [0u8; MAX_BONDS * BOND_SIZE];
        for (i, bond) in bonds.iter().enumerate() {
            buf[i * BOND_SIZE..(i + 1) * BOND_SIZE].copy_from_slice(&serialize_bond(bond));
        }
        if let Err(e) = self
            .kv
            .set("all", &buf[..bonds.len() * BOND_SIZE], true)
            .await
        {
            defmt::warn!("BondStore: store: {:?}", e);
        }
    }

    /// Add or replace the bond for this peer, then persist the full list.
    async fn save(&self, info: &BondInformation) {
        let mut bonds = self.load_all().await;
        let addr = info.identity.bd_addr.into_inner();
        match bonds
            .iter()
            .position(|b| b.identity.bd_addr.into_inner() == addr)
        {
            Some(i) => bonds[i] = info.clone(),
            None => {
                let _ = bonds.push(info.clone());
            }
        }
        self.store_all(&bonds).await;
    }

    /// Remove the bond for `addr` and persist.
    async fn remove(&self, addr: &[u8; 6]) {
        let mut bonds = self.load_all().await;
        bonds.retain(|b| b.identity.bd_addr.into_inner() != *addr);
        self.store_all(&bonds).await;
    }

    /// Delete the `"bonds:all"` key entirely (cleaner than writing a 0-length
    /// value).
    async fn clear_all(&self) {
        if let Err(e) = self.kv.delete("all").await {
            defmt::warn!("BondStore: clear: {:?}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// IPC: BLE task → bond_task
// ---------------------------------------------------------------------------

pub enum BondCmd {
    Save(BondInformation),
    Remove([u8; 6]),
    ClearAll,
}

pub static BOND_CMD_CHANNEL: Channel<CriticalSectionRawMutex, BondCmd, 4> = Channel::new();

/// Populated by `bond_task` at startup; the BLE task waits on this before
/// advertising.
pub static INITIAL_BONDS: OnceLock<Vec<BondInformation, MAX_BONDS>> = OnceLock::new();

// ---------------------------------------------------------------------------
// bond_task
// ---------------------------------------------------------------------------

/// Manages BLE bond persistence.
///
/// Loads all stored bonds into [`INITIAL_BONDS`] at startup, then services
/// [`BondCmd`] messages from the BLE task indefinitely.
///
/// **Requires** `kv::init()` to have been called before this task is spawned.
#[embassy_executor::task]
pub async fn bond_task() {
    kv::smoke_test().await;

    let store = BondStore::new();

    let bonds = store.load_all().await;
    defmt::info!("BondStore: loaded {} bond(s)", bonds.len());
    let _ = INITIAL_BONDS.init(bonds);

    let rx = BOND_CMD_CHANNEL.receiver();
    loop {
        match rx.receive().await {
            BondCmd::Save(info) => {
                defmt::info!("BondStore: saving bond for {:?}", info.identity.bd_addr);
                store.save(&info).await;
            }
            BondCmd::Remove(addr) => {
                defmt::info!("BondStore: removing bond");
                store.remove(&addr).await;
            }
            BondCmd::ClearAll => {
                defmt::info!("BondStore: clearing all bonds — rebooting");
                store.clear_all().await;
                cortex_m::peripheral::SCB::sys_reset();
            }
        }
    }
}
