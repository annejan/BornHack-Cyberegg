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
/// SSD1675 panel wiring + tri-colour OTP/partial refresh machinery.
#[cfg(feature = "ssd1675-driver")]
pub mod epd;
/// SSD1675 [`crate::epd_driver::EpdDriver`] adapter over [`epd::EpdGfx`].
#[cfg(feature = "ssd1675-driver")]
pub mod epd_1675_driver;
/// SSD1680 [`crate::epd_driver::EpdDriver`] — 152×152 canvas composed into the
/// 200×200 panel, custom delta LUT for non-flashing partial updates.
#[cfg(feature = "ssd1680-driver")]
pub mod epd_1680_driver;
pub mod factory_test;
pub mod fat12;
pub mod flash;
pub mod health;
pub mod iso14443;
pub mod kv;
pub mod qwiic;
pub mod i2c_keyboard;
pub mod led;
/// Screen lock — Cancel-hold toggles a global input lock + red padlock overlay.
pub mod lock;
pub mod nfct;
/// Buck/boost converter power-mode arbiter (`PS_SYNC` pin).
pub mod power;
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
