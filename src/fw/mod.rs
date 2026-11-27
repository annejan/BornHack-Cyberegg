pub mod battery;
#[macro_use]
pub mod board;
pub mod button;
pub mod buzzer;
pub mod device_id;
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
/// `EPD.CFG` waveform tuning loader (SSD1680 and SSD1675 drivers).
#[cfg(any(feature = "ssd1680-driver", feature = "ssd1675-driver"))]
pub mod epd_cfg;
pub mod factory_test;
pub mod fat12;
pub mod flash;
pub mod health;
pub mod iso14443;
pub mod kv;
pub mod qwiic;
pub mod i2c_keyboard;
pub mod led;
pub mod nfct;
/// Buck/boost converter power-mode arbiter (`PS_SYNC` pin).
pub mod power;
/// Device-wide `"settings"` KV helpers + timezone and EPD tuning persistence.
pub mod settings;
pub mod storage;
pub mod temperature;
/// nRF52840 hardware TRNG register reader (`seed32`).
pub mod trng;

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
