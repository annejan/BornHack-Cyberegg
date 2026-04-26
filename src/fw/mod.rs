pub mod battery;
pub mod board;
pub mod button;
pub mod buzzer;
pub mod device_id;
pub mod epd;
pub mod fat12;
pub mod flash;
pub mod health;
pub mod iso14443;
pub mod kv;
pub mod led;
pub mod nfct;
pub mod sponsors;
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
