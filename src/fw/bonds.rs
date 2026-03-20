//! BLE bond persistence via the shared KV store.
//!
//! All bond data lives under the `"bonds"` namespace in the KV store.
//! Keys:
//!   - `"index"`           — raw concatenation of bonded peer addresses (6 B each)
//!   - `"<AABBCCDDEEFF>"` — serialised `BondInformation` for each peer
//!
//! `bond_task` owns a `BondStore` and services [`BondCmd`] messages from the
//! BLE task.  The KV store must be initialised with `kv::init()` before this
//! task is spawned.

use core::fmt::Write as _;

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
//  [40]     security_level (0=NoEncryption, 1=Encrypted, 2=EncryptedAuthenticated)
//  [41]     reserved

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
    let identity = Identity { bd_addr: BdAddr::new(addr), irk };
    BondInformation::new(identity, ltk, security_level_from_u8(buf[40]), buf[39] != 0)
}

/// Convert a 6-byte BLE address to a 12-character uppercase hex key string.
fn addr_key(addr: &[u8; 6]) -> heapless::String<12> {
    let mut s = heapless::String::new();
    for b in addr {
        write!(s, "{:02X}", b).ok();
    }
    s
}

// ---------------------------------------------------------------------------
// BondStore — KvNamespace wrapper with an address index for enumeration
// ---------------------------------------------------------------------------

struct BondStore {
    kv: KvNamespace,
}

impl BondStore {
    fn new() -> Self {
        Self { kv: kv::namespace("bonds") }
    }

    async fn load_all(&self) -> Vec<BondInformation, MAX_BONDS> {
        let mut out = Vec::new();
        let mut index_buf = [0u8; MAX_BONDS * 6];
        let n = match self.kv.get("index", &mut index_buf).await {
            Ok(n) => n,
            Err(KvError::NotFound) => return out,
            Err(e) => {
                defmt::warn!("BondStore: index read: {:?}", e);
                return out;
            }
        };
        for chunk in index_buf[..n].chunks_exact(6) {
            let mut addr = [0u8; 6];
            addr.copy_from_slice(chunk);
            let mut buf = [0u8; BOND_SIZE];
            match self.kv.get(&addr_key(&addr), &mut buf).await {
                Ok(_) => { let _ = out.push(deserialize_bond(&buf)); }
                Err(e) => defmt::warn!("BondStore: bond read: {:?}", e),
            }
        }
        out
    }

    async fn save(&self, info: &BondInformation) {
        let addr = info.identity.bd_addr.into_inner();
        let data = serialize_bond(info);
        if let Err(e) = self.kv.set(&addr_key(&addr), &data, true).await {
            defmt::warn!("BondStore::save: {:?}", e);
            return;
        }
        self.update_index().await;
    }

    async fn remove(&self, addr: &[u8; 6]) {
        let _ = self.kv.delete(&addr_key(addr)).await;
        self.update_index().await;
    }

    /// Rebuild the index from the addresses that still have a valid bond entry.
    async fn update_index(&self) {
        let mut addrs: Vec<[u8; 6], MAX_BONDS> = Vec::new();
        let mut index_buf = [0u8; MAX_BONDS * 6];
        if let Ok(n) = self.kv.get("index", &mut index_buf).await {
            for chunk in index_buf[..n].chunks_exact(6) {
                let mut addr = [0u8; 6];
                addr.copy_from_slice(chunk);
                let mut tmp = [0u8; BOND_SIZE];
                if self.kv.get(&addr_key(&addr), &mut tmp).await.is_ok() {
                    let _ = addrs.push(addr);
                }
            }
        }
        let mut flat = [0u8; MAX_BONDS * 6];
        for (i, addr) in addrs.iter().enumerate() {
            flat[i * 6..(i + 1) * 6].copy_from_slice(addr);
        }
        if let Err(e) = self.kv.set("index", &flat[..addrs.len() * 6], true).await {
            defmt::warn!("BondStore: index write: {:?}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// IPC: BLE task → bond_task
// ---------------------------------------------------------------------------

pub enum BondCmd {
    Save(BondInformation),
    Remove([u8; 6]),
}

pub static BOND_CMD_CHANNEL: Channel<CriticalSectionRawMutex, BondCmd, 4> = Channel::new();

/// Populated by `bond_task` at startup; the BLE task waits on this before advertising.
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
        }
    }
}
