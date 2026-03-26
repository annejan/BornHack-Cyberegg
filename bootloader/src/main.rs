#![no_std]
#![no_main]

mod board;

#[cfg(feature = "defmt")]
use defmt_rtt as _;

use cortex_m_rt::entry;

// ---------------------------------------------------------------------------
// Raw NVMC register programming
// ---------------------------------------------------------------------------

const NVMC_BASE: u32 = 0x4001_E000;
const NVMC_READY: *const u32 = (NVMC_BASE + 0x400) as *const u32;
const NVMC_CONFIG: *mut u32 = (NVMC_BASE + 0x504) as *mut u32;
const NVMC_ERASEPAGE: *mut u32 = (NVMC_BASE + 0x508) as *mut u32;
const PAGE_SIZE: u32 = 4096;

unsafe fn nvmc_wait_ready() {
    while unsafe { core::ptr::read_volatile(NVMC_READY) } == 0 {}
}

unsafe fn nvmc_erase_page(addr: u32) {
    unsafe {
        nvmc_wait_ready();
        core::ptr::write_volatile(NVMC_CONFIG, 2); // ERASEEN
        nvmc_wait_ready();
        core::ptr::write_volatile(NVMC_ERASEPAGE, addr);
        nvmc_wait_ready();
        core::ptr::write_volatile(NVMC_CONFIG, 0); // REN (read-only)
    }
}

/// Write a 4-byte-aligned slice to flash. `addr` and `data.len()` must be
/// multiples of 4; the page must already be erased.
unsafe fn nvmc_write(addr: u32, data: &[u8]) {
    debug_assert!(addr % 4 == 0);
    debug_assert!(data.len() % 4 == 0);
    unsafe {
        nvmc_wait_ready();
        core::ptr::write_volatile(NVMC_CONFIG, 1); // WEN
        nvmc_wait_ready();
        for (i, chunk) in data.chunks_exact(4).enumerate() {
            let word = u32::from_le_bytes(chunk.try_into().unwrap());
            let ptr = (addr + i as u32 * 4) as *mut u32;
            core::ptr::write_volatile(ptr, word);
            nvmc_wait_ready();
        }
        core::ptr::write_volatile(NVMC_CONFIG, 0); // REN
    }
}

// ---------------------------------------------------------------------------
// App validation + jump
// ---------------------------------------------------------------------------

unsafe extern "C" {
    static APP_START: u32;
}

/// Returns true if the vector table at `app_addr` looks like a valid Cortex-M
/// image: SP in RAM and reset vector is an odd (Thumb) address in flash.
fn app_is_valid(app_addr: u32) -> bool {
    let sp = unsafe { core::ptr::read_volatile(app_addr as *const u32) };
    let rv = unsafe { core::ptr::read_volatile((app_addr + 4) as *const u32) };
    // Top of 256 KB RAM = 0x2004_0000, which is the typical initial SP value.
    let sp_ok = (0x2000_0000..=0x2004_0000).contains(&sp);
    let rv_ok = rv & 1 == 1 && (app_addr..0x0010_0000).contains(&(rv & !1));
    sp_ok && rv_ok
}

/// Set VTOR, load SP from the vector table, and branch to the reset vector.
/// Never returns.
unsafe fn jump_to_app(app_addr: u32) -> ! {
    let sp = unsafe { core::ptr::read_volatile(app_addr as *const u32) };
    let rv = unsafe { core::ptr::read_volatile((app_addr + 4) as *const u32) };
    unsafe {
        // Relocate vector table to the app.
        core::ptr::write_volatile(0xE000_ED08 as *mut u32, app_addr);
        // DSB + ISB ensure the VTOR write is visible and the pipeline is
        // flushed before we change the stack pointer and branch.
        // VTOR already points to the app, so any interrupt that fires after
        // this point will be handled by the app's vector table — CPSID is
        // not needed and would leave the app running with interrupts masked.
        core::arch::asm!("DSB", "ISB", options(nostack, nomem));
        core::arch::asm!(
            "MSR   MSP, {sp}",
            "ISB",
            "BX    {rv}",
            sp = in(reg) sp,
            rv = in(reg) rv,
            options(noreturn),
        );
    }
}

// ---------------------------------------------------------------------------
// QSPI factory reset (blocking — no executor needed)
// ---------------------------------------------------------------------------

/// Erase the entire QSPI flash chip, then reset the device.
/// Button combo: execute + cancel + fire held at boot.
/// Never returns.
#[cfg(feature = "dfu")]
fn factory_reset_and_reset(
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

    let mut cfg = qspi::Config::default();
    cfg.capacity = 2 * 1024 * 1024;
    cfg.read_opcode = qspi::ReadOpcode::FASTREAD;
    cfg.write_opcode = qspi::WriteOpcode::PP;

    // QspiIrqs binding — we only use blocking_custom_instruction so the
    // interrupt is never actually fired, but the type is still required.
    embassy_nrf::bind_interrupts!(struct QspiIrqs {
        QSPI => embassy_nrf::qspi::InterruptHandler<embassy_nrf::peripherals::QSPI>;
    });

    let mut qspi = qspi::Qspi::new(qspi_periph, QspiIrqs, sck, csn, io0, io1, io2, io3, cfg);

    // Write Enable (WREN)
    let _ = qspi.blocking_custom_instruction(0x06, &[], &mut []);
    // Chip Erase (CE) — ZD25WQ16C: ~40 s worst case
    let _ = qspi.blocking_custom_instruction(0xC7, &[], &mut []);

    // Poll WIP (Write In Progress) bit of status register until clear.
    loop {
        let mut sr = [0u8; 1];
        let _ = qspi.blocking_custom_instruction(0x05, &[], &mut sr);
        if sr[0] & 0x01 == 0 {
            break;
        }
        // Blink red slowly while erasing.
        led_red.toggle();
    }

    defmt::info!("Factory reset complete — resetting");
    cortex_m::peripheral::SCB::sys_reset()
}

// ---------------------------------------------------------------------------
// USB DFU handler
// ---------------------------------------------------------------------------

#[cfg(feature = "dfu")]
embassy_nrf::bind_interrupts!(struct UsbIrqs {
    USBD        => embassy_nrf::usb::InterruptHandler<embassy_nrf::peripherals::USBD>;
    CLOCK_POWER => embassy_nrf::usb::vbus_detect::InterruptHandler;
});

/// DFU state machine states (DFU 1.1 spec §A.1).
#[cfg(feature = "dfu")]
#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
enum DfuState {
    Idle        = 2,
    DnloadSync  = 3,
    DnloadIdle  = 5,
    ManifestSync = 6,
    ManifestWaitReset = 8,
    Error       = 10,
}

/// DFU status codes (DFU 1.1 spec §A.2).
#[cfg(feature = "dfu")]
#[allow(dead_code)]
#[derive(Clone, Copy)]
#[repr(u8)]
enum DfuStatus {
    Ok          = 0x00,
    ErrWrite    = 0x03,
    ErrVerify   = 0x04,
    ErrAddress  = 0x08,
    ErrUnknown  = 0x0E,
}

/// Maximum DFU block transfer size — one nRF52840 flash page.
const DFU_BLOCK_SIZE: usize = 4096;

// Atomics shared between the USB handler (called from usb.run()) and the
// monitor async block — avoids a borrow conflict after builder.handler().
#[cfg(feature = "dfu")]
static DFU_STATE_ATOMIC: core::sync::atomic::AtomicU8 =
    core::sync::atomic::AtomicU8::new(2 /* Idle */);
#[cfg(feature = "dfu")]
static DFU_RESET_PENDING: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Custom DFU class handler.
///
/// Implements the DFU download state machine and writes firmware blocks
/// directly to the app partition starting at APP_START using raw NVMC writes.
/// No embassy-boot partition swap machinery is involved.
#[cfg(feature = "dfu")]
struct DfuHandler {
    state:      DfuState,
    err_status: DfuStatus,
    block_num:  u16,
    write_addr: u32,
    buf:        [u8; DFU_BLOCK_SIZE],
    buf_len:    usize,
}

#[cfg(feature = "dfu")]
impl DfuHandler {
    fn new(app_start: u32) -> Self {
        Self {
            state: DfuState::Idle,
            err_status: DfuStatus::Ok,
            block_num: 0,
            write_addr: app_start,
            buf: [0u8; DFU_BLOCK_SIZE],
            buf_len: 0,
        }
    }

    fn set_state(&mut self, s: DfuState) {
        self.state = s;
        DFU_STATE_ATOMIC.store(s as u8, core::sync::atomic::Ordering::Release);
    }

    /// Erase the current page and write `buf[..buf_len]` to it, padded to a
    /// word boundary with 0xFF. Returns true on success.
    fn program_block(&mut self) -> bool {
        let addr = self.write_addr;
        if addr < unsafe { &APP_START as *const u32 as u32 } {
            return false; // refuse to overwrite bootloader
        }
        // Pad to word boundary
        let padded = (self.buf_len + 3) & !3;
        self.buf[self.buf_len..padded].fill(0xFF);

        unsafe {
            nvmc_erase_page(addr);
            nvmc_write(addr, &self.buf[..padded]);
        }
        self.write_addr += PAGE_SIZE;
        self.block_num = self.block_num.wrapping_add(1);
        true
    }
}

#[cfg(feature = "dfu")]
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
                    // wLength == 0 → end of download
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
                            let len = data.len().min(DFU_BLOCK_SIZE);
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
                match self.state {
                    DfuState::Idle | DfuState::DnloadIdle => {
                        self.set_state(DfuState::Idle);
                    }
                    _ => {}
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
            // DFU_GETSTATUS → 6-byte response: [bStatus, poll_ms×3, bState, iString]
            3 => {
                let (status, state) = match self.state {
                    DfuState::DnloadSync => {
                        // Program the buffered block synchronously.
                        // USB SIE auto-NAKs IN tokens while CPU is in NVMC,
                        // so the host will simply wait until we respond.
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
                        DFU_RESET_PENDING.store(true, core::sync::atomic::Ordering::Release);
                        (DfuStatus::Ok, DfuState::ManifestWaitReset)
                    }
                    other => (DfuStatus::Ok, other),
                };
                if buf.len() < 6 {
                    return Some(InResponse::Rejected);
                }
                buf[0] = status as u8;
                buf[1] = 0; // wPollTimeout low
                buf[2] = 0;
                buf[3] = 0; // wPollTimeout high
                buf[4] = state as u8;
                buf[5] = 0; // iString
                Some(InResponse::Accepted(&buf[..6]))
            }
            // DFU_GETSTATE → 1-byte state
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
// DFU async task (executor runs only when DFU mode is requested)
// ---------------------------------------------------------------------------

#[cfg(feature = "dfu")]
#[embassy_executor::task]
async fn dfu_task(
    usbd: embassy_nrf::Peri<'static, embassy_nrf::peripherals::USBD>,
    app_start: u32,
    led_red_pin: embassy_nrf::Peri<'static, embassy_nrf::gpio::AnyPin>,
    led_blue_pin: embassy_nrf::Peri<'static, embassy_nrf::gpio::AnyPin>,
    led_green_pin: embassy_nrf::Peri<'static, embassy_nrf::gpio::AnyPin>,
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

    let driver = Driver::new(usbd, UsbIrqs, HardwareVbusDetect::new(UsbIrqs));

    let mut usb_config = embassy_usb::Config::new(0x1915, 0x521f);
    usb_config.manufacturer = Some("Badge.Team");
    usb_config.product = Some("CyberAegg Bootloader");
    usb_config.max_packet_size_0 = 64;

    let mut config_descriptor = [0u8; 256];
    let mut bos_descriptor    = [0u8; 256];
    let mut control_buf       = [0u8; DFU_BLOCK_SIZE + 64];

    let mut dfu_handler = DfuHandler::new(app_start);

    let mut builder = Builder::new(
        driver,
        usb_config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [],
        &mut control_buf,
    );

    // Add DFU interface (class=0xFE Application Specific, subclass=0x01 DFU,
    // protocol=0x02 DFU mode — as distinct from 0x01 Runtime).
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
                // wTransferSize = 4096
                (DFU_BLOCK_SIZE & 0xFF) as u8,
                ((DFU_BLOCK_SIZE >> 8) & 0xFF) as u8,
                0x10, 0x01, // bcdDFUVersion = 1.10
            ],
        );
    }

    builder.handler(&mut dfu_handler);

    let mut usb = builder.build();

    // Run USB until the manifest phase completes, then reset.
    embassy_futures::select::select(
        usb.run(),
        async {
            loop {
                if DFU_RESET_PENDING.load(core::sync::atomic::Ordering::Acquire) {
                    // Give the host a moment to see the final GETSTATUS response.
                    Timer::after_millis(100).await;
                    break;
                }
                // Update LED feedback based on current state.
                match DFU_STATE_ATOMIC.load(core::sync::atomic::Ordering::Acquire) {
                    3 | 5 => {
                        // DnloadSync | DnloadIdle — solid blue during download.
                        led_red.set_high();
                        led_blue.set_low();
                    }
                    6 | 8 => {
                        // ManifestSync | ManifestWaitReset — solid green.
                        led_blue.set_high();
                        led_green.set_low();
                    }
                    10 => {
                        // Error — rapid red blink.
                        led_blue.set_high();
                        led_green.set_high();
                        led_red.toggle();
                        Timer::after_millis(100).await;
                    }
                    _ => {
                        // Idle/waiting — slow red blink.
                        led_blue.set_high();
                        led_red.toggle();
                        Timer::after_millis(500).await;
                    }
                }
                Timer::after_millis(10).await;
            }
        },
    )
    .await;

    defmt::info!("DFU complete — resetting");

    // Green blink × 3 before reset.
    led_blue.set_high();
    led_red.set_high();
    for _ in 0..3 {
        led_green.set_low();
        Timer::after_millis(150).await;
        led_green.set_high();
        Timer::after_millis(150).await;
    }

    cortex_m::peripheral::SCB::sys_reset();
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[entry]
fn main() -> ! {
    let p = embassy_nrf::init(Default::default());

    // Determine the app start address from the linker symbol.
    let app_start = unsafe { &APP_START as *const u32 as u32 };

    #[cfg(feature = "dfu")]
    {
        use embassy_nrf::gpio::{Input, Pull};

        let btn_exe  = Input::new(board!(p, btn_exe),  Pull::Up);
        let btn_can  = Input::new(board!(p, btn_can),  Pull::Up);
        let joy_fire = Input::new(board!(p, joy_fire), Pull::Up);

        let factory_reset = btn_exe.is_low() && btn_can.is_low() && joy_fire.is_low();
        let dfu_requested = btn_exe.is_low() && !factory_reset;

        drop(btn_exe);
        drop(btn_can);
        drop(joy_fire);

        if factory_reset {
            defmt::info!("btn_exe + btn_can + joy_fire held — factory reset");
            // Consume peripherals; this branch is -> ! so no double-move.
            factory_reset_and_reset(
                p.QSPI,
                p.P0_21, p.P0_25, p.P0_20, p.P0_24, p.P0_22, p.P0_23,
                board!(p, led_red).into(),
            );
        }

        if dfu_requested {
            defmt::info!("btn_exe held — entering USB DFU mode");
            use static_cell::StaticCell;
            static EXECUTOR: StaticCell<embassy_executor::Executor> = StaticCell::new();
            let executor = EXECUTOR.init(embassy_executor::Executor::new());
            let led_red   = board!(p, led_red).into();
            let led_blue  = board!(p, led_blue).into();
            let led_green = board!(p, led_green).into();
            executor.run(|spawner| {
                spawner
                    .spawn(dfu_task(p.USBD, app_start, led_red, led_blue, led_green))
                    .unwrap();
            });
            // executor.run() never returns.
        }
    }

    // Normal boot path: validate the app vector table and jump.
    if app_is_valid(app_start) {
        defmt::info!("Booting app at 0x{:08X}", app_start);
        unsafe { jump_to_app(app_start) }
    } else {
        // No valid app: blink red and wait for DFU via USB.
        defmt::warn!("No valid app found — enter DFU mode by holding execute button and power-cycling");
        loop {
            cortex_m::asm::wfe();
        }
    }
}

// ---------------------------------------------------------------------------
// Fault handlers
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
#[cfg_attr(target_os = "none", unsafe(link_section = ".HardFault.user"))]
unsafe extern "C" fn HardFault() {
    cortex_m::peripheral::SCB::sys_reset();
}

#[cortex_m_rt::exception]
unsafe fn DefaultHandler(_: i16) -> ! {
    cortex_m::peripheral::SCB::sys_reset()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    cortex_m::peripheral::SCB::sys_reset()
}

#[cfg(feature = "defmt")]
#[unsafe(no_mangle)]
fn _defmt_panic() -> ! {
    cortex_m::peripheral::SCB::sys_reset()
}

#[cfg(feature = "defmt")]
defmt::timestamp!("");
