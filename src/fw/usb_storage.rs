//! USB Mass Storage — exposes the FAT12 flash partition via USB.
//!
//! When a USB cable is connected (VBUS detected), the nRF52840 enumerates
//! as a USB mass storage device.  The host sees a 1 MiB block device that
//! it can format as FAT12 and use for drag-and-drop file transfer.
//!
//! The FAT12 partition occupies the second 1 MiB of external QSPI flash
//! (0x100000–0x1FFFFF).  All flash access goes through [`crate::fw::flash`]
//! which serializes with the ekv KV store on the first 1 MiB.

use embassy_nrf::usb::Driver;
use embassy_nrf::usb::vbus_detect::SoftwareVbusDetect;
use embassy_nrf::{Peri, bind_interrupts, peripherals, usb};
use embassy_usb::Builder;
use static_cell::StaticCell;

use crate::fw::flash;
use crate::fw::usb_msc::{BlockDevice, MscClass, MscState};

// ---------------------------------------------------------------------------
// Interrupt binding (USBD only — VBUS detection is software-polled to avoid
// conflicting with the CLOCK_POWER binding used by MPSL for BLE)
// ---------------------------------------------------------------------------

bind_interrupts!(struct UsbIrqs {
    USBD => usb::InterruptHandler<peripherals::USBD>;
});

// ---------------------------------------------------------------------------
// Block device over FAT partition
// ---------------------------------------------------------------------------

/// Maps 512-byte logical blocks to the FAT12 partition on QSPI flash.
///
/// The FAT partition is 1 MiB = 256 × 4 KiB pages = 2048 × 512-byte blocks.
/// Each 4 KiB flash page holds 8 logical blocks.
pub struct FatBlockDevice;

impl FatBlockDevice {
    /// Number of 512-byte blocks in the FAT partition.
    pub const BLOCK_COUNT: u32 = (flash::FAT_BYTES / 512) as u32;
}

impl BlockDevice for FatBlockDevice {
    fn block_count(&self) -> u32 {
        Self::BLOCK_COUNT
    }

    async fn read_block(&self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        // Reject a host-supplied LBA outside the partition: without this the
        // address wraps modulo the 2 MiB chip and can read/overwrite the ekv
        // KV store (save data) that lives after the FAT region.
        if lba >= Self::BLOCK_COUNT {
            return Err(());
        }
        let addr = flash::FAT_OFFSET + lba * 512;
        flash::read(addr, buf).await.map_err(|_| ())
    }

    async fn write_block(&self, lba: u32, buf: &[u8]) -> Result<(), ()> {
        if lba >= Self::BLOCK_COUNT {
            return Err(());
        }
        // Blink the blue LED on every block write so the operator can see at
        // a glance which badge is still receiving data during mass-flashing.
        // `BlinkOnce` auto-resets after ~50 ms, so a stream of writes shows
        // up as flicker without needing a "turn off when idle" timer.
        crate::fw::led::set_led(
            &crate::fw::led::LED_BLUE,
            crate::fw::led::LedState::BlinkOnce,
        );

        let addr = flash::FAT_OFFSET + lba * 512;

        // NOR flash requires erase before write.  We erase the containing
        // 4 KiB sector, read-modify-write the 512-byte block within it.
        let sector_addr = addr & !(flash::PAGE_SIZE as u32 - 1);
        let offset_in_sector = (addr - sector_addr) as usize;

        let mut sector_buf = [0u8; flash::PAGE_SIZE];
        flash::read(sector_addr, &mut sector_buf)
            .await
            .map_err(|_| ())?;
        sector_buf[offset_in_sector..offset_in_sector + 512].copy_from_slice(buf);
        flash::erase(sector_addr).await.map_err(|_| ())?;
        flash::write(sector_addr, &sector_buf).await.map_err(|_| ())
    }
}

// ---------------------------------------------------------------------------
// USB task
// ---------------------------------------------------------------------------

/// Spawn-ready wrapper around [`run`] for use with `Spawner::must_spawn`.
/// Lets the USB stack come up early in `main()` and stay running alongside
/// the sponsor slideshow, first-boot screens, and the main display loop.
#[embassy_executor::task]
pub async fn usb_storage_task(usbd: Peri<'static, peripherals::USBD>) {
    run(usbd).await;
}

/// Run the USB mass storage device.  Returns only on unrecoverable error.
///
/// VBUS detection is handled automatically by the nRF52840 POWER peripheral:
/// the USB PHY powers up when a cable is connected and shuts down when removed.
pub async fn run(usbd: Peri<'_, peripherals::USBD>) {
    // Software VBUS detect — avoids the CLOCK_POWER interrupt conflict
    // with MPSL (BLE).  The USB task is only started when VBUS is present,
    // so we set detected=true, power_ready=true.
    // TODO: switch to HardwareVbusDetect when interrupt sharing is resolved.
    static VBUS_DETECT: StaticCell<SoftwareVbusDetect> = StaticCell::new();
    let vbus: &SoftwareVbusDetect = VBUS_DETECT.init(SoftwareVbusDetect::new(true, true));
    let driver = Driver::new(usbd, UsbIrqs, vbus);

    static MSC_STATE: StaticCell<MscState> = StaticCell::new();
    let msc_state = MSC_STATE.init(MscState::new());

    // USB config.
    let mut config = embassy_usb::Config::new(0x1209, 0x0001); // pid.codes test VID/PID
    config.manufacturer = Some("BornHack");
    config.product = Some("CyberEgg Storage");
    config.serial_number = Some("0001");
    config.max_power = 100; // 100 mA
    config.max_packet_size_0 = 64;

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        config,
        CONFIG_DESC.init([0; 256]),
        BOS_DESC.init([0; 256]),
        &mut [], // no msos descriptors
        CONTROL_BUF.init([0; 64]),
    );

    let mut msc = MscClass::new(&mut builder, msc_state, 64);

    let mut usb_dev = builder.build();

    let dev = FatBlockDevice;

    // Run USB device stack and MSC class concurrently.
    let usb_fut = usb_dev.run();
    let msc_fut = msc.run(&dev);

    embassy_futures::join::join(usb_fut, msc_fut).await;
}
