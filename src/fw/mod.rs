pub mod battery;
pub mod board;
/// BORNPETS.CFG parser — overrides game balance thresholds from the USB-MSC
/// partition. Game-only: it reads `crate::game` threshold types.
#[cfg(feature = "game")]
pub mod bornpets_cfg;
pub mod button;
pub mod buzzer;
pub mod device_id;
pub mod emoji;
pub mod epd;
pub mod factory_test;
pub mod fat12;
pub mod flash;
pub mod health;
pub mod iso14443;
pub mod kv;
pub mod qwiic;
pub mod led;
pub mod nfct;
/// PETS.CFG parser — registers custom pets from the USB-MSC partition.
/// Game-only: it reads `crate::game` pet-registry types.
#[cfg(feature = "game")]
pub mod pets_cfg;
pub mod storage;
pub mod temperature;

/// MeshCore networking stack (LoRa radio, BLE companion, contacts, channels, KV
/// store).
#[cfg(feature = "mesh")]
pub mod mesh;

/// USB Mass Storage class (Bulk-Only Transport + SCSI).
#[cfg(feature = "usb-storage")]
pub mod usb_msc;
/// USB storage task — exposes FAT12 partition via USB.
#[cfg(feature = "usb-storage")]
pub mod usb_storage;
