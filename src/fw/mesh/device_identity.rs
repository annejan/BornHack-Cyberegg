//! MeshCore device identity: Ed25519 keypair backed by QSPI KV flash.
//!
//! On first boot a random 32-byte seed is generated from the nRF52840's
//! on-chip TRNG and persisted under the `"meshcore"` namespace.  On every
//! subsequent boot the stored seed is loaded and the keypair is re-derived
//! without touching the flash.
//!
//! # Usage
//!
//! ```rust
//! // In embassy.rs, before spawning the LoRa task:
//! let identity = device_identity::load_or_create(kv::namespace("meshcore")).await;
//! // Pass to run_meshcore_listener(..., identity).
//! ```

use crate::fw::kv::{KvError, KvNamespace};

/// KV key under which the 32-byte seed is stored.
const KV_SEED_KEY: &str = "identity_seed";

// ---------------------------------------------------------------------------
// Public type
// ---------------------------------------------------------------------------

/// The device's Ed25519 identity, derived from a persistent seed.
pub struct DeviceIdentity {
    /// Ed25519 public key (32 bytes).  Broadcast in advert packets.
    pub pub_key: [u8; meshcore::PUB_KEY_SIZE],
    /// Ed25519 secret key (64 bytes: seed || public_key).
    /// Never leaves the device — used only to sign outgoing adverts.
    pub sec_key: [u8; meshcore::PRV_KEY_SIZE],
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load the device identity from flash, or generate a new one if none exists.
///
/// Call once at startup, before any task that needs to sign or verify adverts.
pub async fn load_or_create(kv: KvNamespace) -> DeviceIdentity {
    let mut seed = [0u8; 32];

    match kv.get(KV_SEED_KEY, &mut seed).await {
        Ok(32) => {
            defmt::info!("MeshCore identity: loaded existing keypair from flash");
        }
        Ok(n) => {
            defmt::warn!(
                "MeshCore identity: seed in flash has wrong length ({=usize}), regenerating",
                n
            );
            seed = crate::fw::trng::seed32();
            persist_seed(&kv, &seed).await;
        }
        Err(KvError::NotFound) => {
            defmt::info!("MeshCore identity: no keypair found, generating new one");
            seed = crate::fw::trng::seed32();
            persist_seed(&kv, &seed).await;
        }
        Err(e) => {
            defmt::warn!(
                "MeshCore identity: KV read error ({:?}), using ephemeral keypair",
                e
            );
            seed = crate::fw::trng::seed32();
            // Don't persist — storage may be broken; an ephemeral key is
            // better than a boot loop.
        }
    }

    derive_identity(&seed)
}

/// Generate a brand-new random identity and overwrite the stored seed.
///
/// Call this when the user requests a key rotation via the menu.
pub async fn regenerate(kv: KvNamespace) -> DeviceIdentity {
    let seed = crate::fw::trng::seed32();
    persist_seed(&kv, &seed).await;
    defmt::info!("MeshCore identity: keypair regenerated");
    derive_identity(&seed)
}

/// Delete the stored seed from flash.
///
/// The next call to [`load_or_create`] will generate a fresh identity.
pub async fn delete(kv: KvNamespace) {
    match kv.delete(KV_SEED_KEY).await {
        Ok(()) => defmt::info!("MeshCore identity: seed deleted from flash"),
        Err(e) => defmt::warn!("MeshCore identity: failed to delete seed: {:?}", e),
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn derive_identity(seed: &[u8; 32]) -> DeviceIdentity {
    let (pub_key, sec_key) = meshcore::identity::keypair_from_seed(seed);
    DeviceIdentity { pub_key, sec_key }
}

async fn persist_seed(kv: &KvNamespace, seed: &[u8; 32]) {
    if let Err(e) = kv.set(KV_SEED_KEY, seed, true).await {
        defmt::warn!("MeshCore identity: failed to persist seed: {:?}", e);
    }
}
