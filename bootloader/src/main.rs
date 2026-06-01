#![no_std]
#![no_main]

mod board;
mod nvmc;
#[cfg(feature = "dfu")]
mod dfu;
#[cfg(feature = "dfu")]
mod flash;
#[cfg(feature = "dfu")]
mod msc;
#[cfg(feature = "dfu")]
mod storage;

#[cfg(feature = "defmt")]
use defmt_rtt as _;
use cortex_m_rt::entry;

// ---------------------------------------------------------------------------
// Linker symbol — set in memory.x
// ---------------------------------------------------------------------------

unsafe extern "C" {
    static APP_START: u32;
}

// ---------------------------------------------------------------------------
// App validation
// ---------------------------------------------------------------------------

/// Number of leading app bytes probed to decide if the slot is blank.
const PROBE_BYTES: u32 = 64;

/// Returns true if the vector table at `app_addr` looks like a valid Cortex-M
/// image: SP within RAM and reset vector is an odd (Thumb) address in flash.
fn app_is_valid(app_addr: u32) -> bool {
    let sp = unsafe { core::ptr::read_volatile(app_addr as *const u32) };
    let rv = unsafe { core::ptr::read_volatile((app_addr + 4) as *const u32) };
    // Top of 256 KB RAM is 0x2004_0000, which is the typical initial SP value.
    let sp_ok = (0x2000_0000..=0x2004_0000).contains(&sp);
    let rv_ok = rv & 1 == 1 && (app_addr..0x0010_0000).contains(&(rv & !1));
    sp_ok && rv_ok
}

/// Returns true if the app slot looks erased: the first [`PROBE_BYTES`] of the
/// boot vector are all `0xFF` (blank flash). A badge with no firmware should
/// drop straight into DFU mode on power-up so it can be flashed.
fn app_is_empty(app_addr: u32) -> bool {
    (0..PROBE_BYTES).all(|i| unsafe { core::ptr::read_volatile((app_addr + i) as *const u8) } == 0xFF)
}

// ---------------------------------------------------------------------------
// Jump to app
// ---------------------------------------------------------------------------

/// Relocate VTOR, load the app's initial SP, and branch to its reset handler.
/// Never returns.
unsafe fn jump_to_app(app_addr: u32) -> ! {
    let sp = unsafe { core::ptr::read_volatile(app_addr as *const u32) };
    let rv = unsafe { core::ptr::read_volatile((app_addr + 4) as *const u32) };
    unsafe {
        // Point VTOR at the app's vector table.
        core::ptr::write_volatile(0xE000_ED08 as *mut u32, app_addr);
        // DSB + ISB: ensure the VTOR write completes and the pipeline is
        // flushed before we change MSP. VTOR already points to the app, so
        // any interrupt that fires here is handled by the app's table —
        // CPSID is intentionally omitted to avoid masking interrupts in the app.
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
// Entry point
// ---------------------------------------------------------------------------

#[entry]
fn main() -> ! {
    let p = embassy_nrf::init(Default::default());

    let app_start = unsafe { &APP_START as *const u32 as u32 };

    #[cfg(feature = "dfu")]
    {
        use embassy_nrf::gpio::{Input, Pull};

        let btn_exe = Input::new(board!(p, btn_exe), Pull::Up);

        #[cfg(feature = "with-qspi-flash")]
        let factory_reset = {
            let btn_can  = Input::new(board!(p, btn_can),  Pull::Up);
            let joy_fire = Input::new(board!(p, joy_fire), Pull::Up);
            let combo = btn_exe.is_low() && btn_can.is_low() && joy_fire.is_low();
            drop(btn_can);
            drop(joy_fire);
            combo
        };
        #[cfg(not(feature = "with-qspi-flash"))]
        let factory_reset = false;

        // Enter DFU if the execute button is held OR the app slot is blank
        // (first 64 bytes all 0xFF → no firmware to boot). Checked on every
        // power-up so a freshly-rolled-out badge auto-enters flash mode, while
        // staying out of the way for any badge that already has firmware
        // (including SWD/probe-rs flashes). Factory-reset combo wins.
        let app_empty = app_is_empty(app_start);
        let dfu_requested = (btn_exe.is_low() || app_empty) && !factory_reset;
        drop(btn_exe);

        #[cfg(feature = "with-qspi-flash")]
        if factory_reset {
            defmt::info!("btn_exe + btn_can + joy_fire held — factory reset");
            dfu::factory_reset_and_reset(
                p.QSPI,
                p.P0_21, p.P0_25, p.P0_20, p.P0_24, p.P0_22, p.P0_23,
                board!(p, led_red).into(),
            );
        }

        if dfu_requested {
            if app_empty {
                defmt::info!("app slot empty (0xFF boot vector) — entering USB DFU mode");
            } else {
                defmt::info!("btn_exe held — entering USB DFU mode");
            }
            use static_cell::StaticCell;
            static EXECUTOR: StaticCell<embassy_executor::Executor> = StaticCell::new();
            let executor = EXECUTOR.init(embassy_executor::Executor::new());
            executor.run(|spawner| {
                spawner
                    .spawn(dfu::dfu_task(
                        p.USBD,
                        app_start,
                        board!(p, led_red).into(),
                        board!(p, led_blue).into(),
                        board!(p, led_green).into(),
                        p.QSPI,
                        p.P0_21, p.P0_25, p.P0_20, p.P0_24, p.P0_22, p.P0_23,
                    ))
                    .unwrap();
            });
            // executor.run() never returns.
        }
    }

    // Normal boot: validate app vector table and jump.
    if app_is_valid(app_start) {
        defmt::info!("Booting app at 0x{:08X}", app_start);
        unsafe { jump_to_app(app_start) }
    } else {
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
