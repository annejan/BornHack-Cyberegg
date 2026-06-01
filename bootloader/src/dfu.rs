//! USB DFU mode and QSPI factory reset for the CyberAegg bootloader.
//!
//! # DFU mode
//! Triggered by holding the execute button at boot. Implements the DFU 1.1
//! download state machine directly on embassy-usb, writing firmware blocks
//! straight to the app partition via raw NVMC writes (no swap partition).
//!
//! # Factory reset
//! Triggered by holding execute + cancel + fire at boot. Erases the entire
//! QSPI flash chip (ZD25WQ16C) using blocking SPI commands, then resets.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use crate::nvmc;

// ---------------------------------------------------------------------------
// Interrupt bindings
// ---------------------------------------------------------------------------

embassy_nrf::bind_interrupts!(pub struct UsbIrqs {
    USBD        => embassy_nrf::usb::InterruptHandler<embassy_nrf::peripherals::USBD>;
    CLOCK_POWER => embassy_nrf::usb::vbus_detect::InterruptHandler;
});

// ---------------------------------------------------------------------------
// DFU state shared between the USB handler and the monitor async block
// ---------------------------------------------------------------------------

/// Current DFU state (raw u8 matching `DfuState` repr).
pub static DFU_STATE: AtomicU8 = AtomicU8::new(2 /* Idle */);
/// Set by the handler when the manifest GETSTATUS is sent; signals reset.
pub static DFU_RESET_PENDING: AtomicBool = AtomicBool::new(false);
/// Incremented by the MSC backend on every FAT12 block write.  The LED
/// monitor samples it to detect "files are being copied" (solid blue).
pub static MSC_WRITE_TICK: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// DFU state machine types (DFU 1.1 spec §A.1 / §A.2)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum DfuState {
    Idle              = 2,
    DnloadSync        = 3,
    DnloadIdle        = 5,
    ManifestSync      = 6,
    ManifestWaitReset = 8,
    Error             = 10,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
#[repr(u8)]
enum DfuStatus {
    Ok         = 0x00,
    ErrWrite   = 0x03,
    ErrUnknown = 0x0E,
}

/// Maximum DFU block transfer size — one nRF52840 flash page (4 KiB).
pub const BLOCK_SIZE: usize = 4096;

// ---------------------------------------------------------------------------
// DfuHandler — embassy-usb Handler impl
// ---------------------------------------------------------------------------

pub struct DfuHandler {
    state:      DfuState,
    err_status: DfuStatus,
    write_addr: u32,
    buf:        [u8; BLOCK_SIZE],
    buf_len:    usize,
    app_start:  u32,
}

impl DfuHandler {
    pub fn new(app_start: u32) -> Self {
        Self {
            state: DfuState::Idle,
            err_status: DfuStatus::Ok,
            write_addr: app_start,
            buf: [0u8; BLOCK_SIZE],
            buf_len: 0,
            app_start,
        }
    }

    fn set_state(&mut self, s: DfuState) {
        self.state = s;
        DFU_STATE.store(s as u8, Ordering::Release);
    }

    /// Erase the current flash page and write the buffered block into it.
    /// Returns `true` on success.
    fn program_block(&mut self) -> bool {
        let addr = self.write_addr;
        if addr < self.app_start {
            return false; // refuse to overwrite the bootloader
        }
        // Pad the payload to a 4-byte boundary with 0xFF (erased-flash value).
        let padded = (self.buf_len + 3) & !3;
        self.buf[self.buf_len..padded].fill(0xFF);

        unsafe {
            nvmc::erase_page(addr);
            nvmc::write(addr, &self.buf[..padded]);
        }
        self.write_addr += nvmc::PAGE_SIZE;
        true
    }
}

impl embassy_usb::Handler for DfuHandler {
    fn control_out(
        &mut self,
        req: embassy_usb::control::Request,
        data: &[u8],
    ) -> Option<embassy_usb::control::OutResponse> {
        use embassy_usb::control::{OutResponse, Recipient, RequestType};
        if req.request_type != RequestType::Class || req.recipient != Recipient::Interface {
            return None;
        }
        match req.request {
            // DFU_DNLOAD
            1 => {
                if data.is_empty() {
                    // Empty transfer signals end of download.
                    match self.state {
                        DfuState::DnloadIdle => {
                            self.set_state(DfuState::ManifestSync);
                            Some(OutResponse::Accepted)
                        }
                        _ => {
                            self.set_state(DfuState::Error);
                            self.err_status = DfuStatus::ErrUnknown;
                            Some(OutResponse::Rejected)
                        }
                    }
                } else {
                    match self.state {
                        DfuState::Idle | DfuState::DnloadIdle => {
                            let len = data.len().min(BLOCK_SIZE);
                            self.buf[..len].copy_from_slice(&data[..len]);
                            self.buf_len = len;
                            self.set_state(DfuState::DnloadSync);
                            Some(OutResponse::Accepted)
                        }
                        _ => {
                            self.set_state(DfuState::Error);
                            self.err_status = DfuStatus::ErrUnknown;
                            Some(OutResponse::Rejected)
                        }
                    }
                }
            }
            // DFU_CLRSTATUS
            4 => {
                if self.state == DfuState::Error {
                    self.set_state(DfuState::Idle);
                    self.err_status = DfuStatus::Ok;
                }
                Some(OutResponse::Accepted)
            }
            // DFU_ABORT
            6 => {
                if matches!(self.state, DfuState::Idle | DfuState::DnloadIdle) {
                    self.set_state(DfuState::Idle);
                }
                Some(OutResponse::Accepted)
            }
            _ => None,
        }
    }

    fn control_in<'a>(
        &'a mut self,
        req: embassy_usb::control::Request,
        buf: &'a mut [u8],
    ) -> Option<embassy_usb::control::InResponse<'a>> {
        use embassy_usb::control::{InResponse, Recipient, RequestType};
        if req.request_type != RequestType::Class || req.recipient != Recipient::Interface {
            return None;
        }
        match req.request {
            // DFU_GETSTATUS — 6-byte response: [bStatus, poll_ms×3, bState, iString]
            3 => {
                let (status, state) = match self.state {
                    DfuState::DnloadSync => {
                        // Program synchronously. The USB SIE auto-NAKs IN
                        // tokens while the CPU is blocked in NVMC, so the
                        // host simply retries until we respond.
                        if self.program_block() {
                            self.set_state(DfuState::DnloadIdle);
                            (DfuStatus::Ok, DfuState::DnloadIdle)
                        } else {
                            self.set_state(DfuState::Error);
                            self.err_status = DfuStatus::ErrWrite;
                            (DfuStatus::ErrWrite, DfuState::Error)
                        }
                    }
                    DfuState::ManifestSync => {
                        self.set_state(DfuState::ManifestWaitReset);
                        DFU_RESET_PENDING.store(true, Ordering::Release);
                        (DfuStatus::Ok, DfuState::ManifestWaitReset)
                    }
                    other => (DfuStatus::Ok, other),
                };
                if buf.len() < 6 {
                    return Some(InResponse::Rejected);
                }
                buf[0] = status as u8;
                buf[1] = 0; // wPollTimeout (3 bytes, little-endian)
                buf[2] = 0;
                buf[3] = 0;
                buf[4] = state as u8;
                buf[5] = 0; // iString
                Some(InResponse::Accepted(&buf[..6]))
            }
            // DFU_GETSTATE — 1-byte state
            5 => {
                if buf.is_empty() {
                    return Some(InResponse::Rejected);
                }
                buf[0] = self.state as u8;
                Some(InResponse::Accepted(&buf[..1]))
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// DFU async task
// ---------------------------------------------------------------------------

#[embassy_executor::task]
pub async fn dfu_task(
    usbd: embassy_nrf::Peri<'static, embassy_nrf::peripherals::USBD>,
    app_start: u32,
    led_red_pin:   embassy_nrf::Peri<'static, embassy_nrf::gpio::AnyPin>,
    led_blue_pin:  embassy_nrf::Peri<'static, embassy_nrf::gpio::AnyPin>,
    led_green_pin: embassy_nrf::Peri<'static, embassy_nrf::gpio::AnyPin>,
    // QSPI flash for the concurrent USB MSC interface (FAT12 provisioning).
    qspi: embassy_nrf::Peri<'static, embassy_nrf::peripherals::QSPI>,
    sck:  embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_21>,
    csn:  embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_25>,
    io0:  embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_20>,
    io1:  embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_24>,
    io2:  embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_22>,
    io3:  embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_23>,
) {
    use embassy_nrf::gpio::{Level, Output, OutputDrive};
    use embassy_nrf::usb::Driver;
    use embassy_nrf::usb::vbus_detect::HardwareVbusDetect;
    use embassy_time::Timer;
    use embassy_usb::Builder;

    defmt::info!("Entering USB DFU mode — waiting for host");

    let mut led_red   = Output::new(led_red_pin,   Level::High, OutputDrive::Standard);
    let mut led_blue  = Output::new(led_blue_pin,  Level::High, OutputDrive::Standard);
    let mut led_green = Output::new(led_green_pin, Level::High, OutputDrive::Standard);

    // Bring up the QSPI flash and ensure the FAT12 partition exists, so the
    // MSC interface exposes the same volume the app uses (CYBR + device-ID
    // label).  On failure the MSC endpoints still enumerate but I/O errors —
    // DFU is on separate endpoints and unaffected.
    match crate::flash::init(qspi, sck, csn, io0, io1, io2, io3).await {
        Ok(()) => {
            if let Err(e) = crate::storage::format_if_needed().await {
                defmt::warn!("FAT12 format_if_needed failed: {:?}", e);
            }
        }
        Err(id) => defmt::warn!(
            "QSPI init failed (JEDEC {=[u8]:02X}) — MSC volume unavailable",
            id
        ),
    }

    let driver = Driver::new(usbd, UsbIrqs, HardwareVbusDetect::new(UsbIrqs));

    let mut usb_config = embassy_usb::Config::new(0x1915, 0x521f);
    usb_config.manufacturer = Some("Badge.Team");
    usb_config.product = Some("CyberAegg Bootloader");
    usb_config.max_packet_size_0 = 64;
    // Composite device: DFU (interface 0) + MSC (interface 1).  Use the
    // IAD/Misc device class so hosts bind both function drivers.
    usb_config.device_class = 0xEF;
    usb_config.device_sub_class = 0x02;
    usb_config.device_protocol = 0x01;
    usb_config.composite_with_iads = true;

    let mut config_descriptor = [0u8; 256];
    let mut bos_descriptor    = [0u8; 256];
    let mut control_buf       = [0u8; BLOCK_SIZE + 64];

    let mut handler = DfuHandler::new(app_start);
    // Declared before `builder` so it outlives the MSC class it backs.
    let mut msc_state = crate::msc::MscState::new();

    let mut builder = Builder::new(
        driver,
        usb_config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [],
        &mut control_buf,
    );

    // Register the DFU interface (class=0xFE Application Specific,
    // subclass=0x01 DFU, protocol=0x02 DFU mode).
    {
        let mut func = builder.function(0xFE, 0x01, 0x02);
        let mut iface = func.interface();
        let mut alt = iface.alt_setting(0xFE, 0x01, 0x02, None);
        // DFU Functional Descriptor (DFU 1.1 §4.1.3)
        alt.descriptor(
            0x21, // DFU_FUNCTIONAL
            &[
                0x0B,       // bmAttributes: WillDetach | CanUpload | CanDnload
                0xFF, 0xFF, // wDetachTimeOut = 65535 ms
                (BLOCK_SIZE & 0xFF) as u8,
                ((BLOCK_SIZE >> 8) & 0xFF) as u8,
                0x10, 0x01, // bcdDFUVersion = 1.10
            ],
        );
    }

    // Register the USB Mass Storage interface (BOT/SCSI) for FAT12 access
    // concurrently with DFU.  Bulk endpoints — no clash with DFU's ep0.
    let mut msc = crate::msc::MscClass::new(&mut builder, &mut msc_state, 64);
    let block_dev = crate::storage::FatBlockDevice;

    builder.handler(&mut handler);
    let mut usb = builder.build();

    // LED monitor (LEDs are active-low: set_low() = on, set_high() = off):
    //   - solid BLUE  while DFU is downloading firmware OR files are being
    //                 copied to the FAT12 partition
    //   - solid GREEN once DFU has finished and copying has gone idle
    //                 (provisioning complete — power-cycle to boot the app)
    //   - rapid RED   blink on DFU error
    //   - slow  RED   blink while idle/waiting (nothing flashed or copied yet)
    //
    // We deliberately do NOT auto-reset on DFU completion: a reset would tear
    // down the USB mass-storage volume mid-copy.  The badge holds solid green
    // and is power-cycled by the operator once provisioning is done.
    let monitor = async {
        const LOOP_MS: u64 = 100;
        // ~500 ms with no MSC writes counts as "copy idle".
        const COPY_IDLE_TICKS: u32 = 5;

        let mut last_tick = MSC_WRITE_TICK.load(Ordering::Acquire);
        let mut copy_idle = COPY_IDLE_TICKS;
        let mut dfu_done = false;

        loop {
            // FAT12 copy activity since the last sample?
            let tick = MSC_WRITE_TICK.load(Ordering::Acquire);
            if tick != last_tick {
                last_tick = tick;
                copy_idle = 0;
            } else if copy_idle < COPY_IDLE_TICKS {
                copy_idle += 1;
            }
            let copying = copy_idle < COPY_IDLE_TICKS;

            let dfu = DFU_STATE.load(Ordering::Acquire);
            if DFU_RESET_PENDING.load(Ordering::Acquire) || dfu == 6 || dfu == 8 {
                dfu_done = true;
            }
            let dfu_active = dfu == 3 || dfu == 5;

            if dfu == 10 {
                // Error — rapid red blink.
                led_blue.set_high();
                led_green.set_high();
                led_red.toggle();
                Timer::after_millis(100).await;
            } else if dfu_active || copying {
                // Flashing firmware or copying files — solid blue.
                led_red.set_high();
                led_green.set_high();
                led_blue.set_low();
                Timer::after_millis(LOOP_MS).await;
            } else if dfu_done {
                // Provisioning finished, nothing copying — solid green.
                led_red.set_high();
                led_blue.set_high();
                led_green.set_low();
                Timer::after_millis(LOOP_MS).await;
            } else {
                // Idle/waiting — slow red blink.
                led_blue.set_high();
                led_green.set_high();
                led_red.toggle();
                Timer::after_millis(500).await;
            }
        }
    };

    // Run the USB device stack, the MSC class, and the LED monitor forever.
    // None of these futures completes, so the task never returns (no reset).
    embassy_futures::join::join3(usb.run(), msc.run(&block_dev), monitor).await;
}

// ---------------------------------------------------------------------------
// QSPI factory reset
// ---------------------------------------------------------------------------

/// Erase the entire QSPI flash chip, then reset.
#[cfg(feature = "with-qspi-flash")]
/// Triggered by holding execute + cancel + fire at boot. Never returns.
pub fn factory_reset_and_reset(
    qspi_periph: embassy_nrf::Peri<'static, embassy_nrf::peripherals::QSPI>,
    sck: embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_21>,
    csn: embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_25>,
    io0: embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_20>,
    io1: embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_24>,
    io2: embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_22>,
    io3: embassy_nrf::Peri<'static, embassy_nrf::peripherals::P0_23>,
    led_red_pin: embassy_nrf::Peri<'static, embassy_nrf::gpio::AnyPin>,
) -> ! {
    use embassy_nrf::gpio::{Level, Output, OutputDrive};
    use embassy_nrf::qspi;

    defmt::info!("Factory reset: erasing QSPI flash…");

    let mut led_red = Output::new(led_red_pin, Level::Low, OutputDrive::Standard);

    // Reuse the single QSPI interrupt binding from the flash module — binding
    // QSPI twice would emit a duplicate `QSPI` ISR symbol (link error).  Only
    // blocking_custom_instruction is used here, so the ISR never fires.
    use crate::flash::QspiIrqs;

    let mut cfg = qspi::Config::default();
    cfg.capacity = 2 * 1024 * 1024; // ZD25WQ16C = 2 MiB
    cfg.read_opcode = qspi::ReadOpcode::FASTREAD;
    cfg.write_opcode = qspi::WriteOpcode::PP;

    let mut qspi = qspi::Qspi::new(qspi_periph, QspiIrqs, sck, csn, io0, io1, io2, io3, cfg);

    let _ = qspi.blocking_custom_instruction(0x06, &[], &mut []); // WREN
    let _ = qspi.blocking_custom_instruction(0xC7, &[], &mut []); // CE (chip erase, ~40 s)

    // Poll WIP (Write In Progress) bit until clear, blinking red while waiting.
    loop {
        let mut sr = [0u8; 1];
        let _ = qspi.blocking_custom_instruction(0x05, &[], &mut sr); // RDSR
        if sr[0] & 0x01 == 0 {
            break;
        }
        led_red.toggle();
    }

    defmt::info!("Factory reset complete — resetting");
    cortex_m::peripheral::SCB::sys_reset()
}
