#![no_std]
#![no_main]

mod board;

use core::cell::RefCell;

use cortex_m_rt::{entry, exception};
#[cfg(feature = "defmt")]
use defmt_rtt as _;
use embassy_boot_nrf::*;
use embassy_nrf::nvmc::Nvmc;
use embassy_nrf::wdt::{self, HaltConfig, SleepConfig};
use embassy_sync::blocking_mutex::Mutex;

// USB DFU interrupt bindings — only compiled when the dfu feature is enabled.
#[cfg(feature = "dfu")]
embassy_nrf::bind_interrupts!(struct Irqs {
    USBD => embassy_nrf::usb::InterruptHandler<embassy_nrf::peripherals::USBD>;
    CLOCK_POWER => embassy_nrf::usb::vbus_detect::InterruptHandler;
});

/// Async task that runs USB DFU in the bootloader.
///
/// The red LED blinks at 5 Hz to indicate DFU mode. The USB DFU class
/// receives the new firmware, writes it to the DFU flash partition, marks it
/// for swap, and resets the device. On the next boot the bootloader swaps the
/// partitions and starts the new application.
#[cfg(feature = "dfu")]
#[embassy_executor::task]
async fn dfu_task(
    usbd: embassy_nrf::Peri<'static, embassy_nrf::peripherals::USBD>,
    nvmc: embassy_nrf::Peri<'static, embassy_nrf::peripherals::NVMC>,
    led_red_pin: embassy_nrf::Peri<'static, embassy_nrf::gpio::AnyPin>,
    led_blue_pin: embassy_nrf::Peri<'static, embassy_nrf::gpio::AnyPin>,
    led_green_pin: embassy_nrf::Peri<'static, embassy_nrf::gpio::AnyPin>,
) {
    use core::sync::atomic::{AtomicBool, Ordering};
    use embassy_boot_nrf::{AlignedBuffer, BlockingFirmwareUpdater, FirmwareUpdaterConfig};
    use embassy_nrf::gpio::{Level, Output, OutputDrive};
    use embassy_nrf::usb::Driver;
    use embassy_nrf::usb::vbus_detect::HardwareVbusDetect;
    use embassy_sync::blocking_mutex::raw::NoopRawMutex;
    use embassy_time::Timer;
    use embassy_usb::Builder;
    use embassy_usb::control::{OutResponse, Recipient, Request, RequestType};
    use embassy_usb_dfu::consts::DfuAttributes;
    use embassy_usb_dfu::{Control, usb_dfu};

    // Set when the first DFU_DNLOAD block arrives — switches LED from red to blue.
    static DOWNLOAD_ACTIVE: AtomicBool = AtomicBool::new(false);
    // Set by DeferredReset when the download is complete.
    static RESET_REQUESTED: AtomicBool = AtomicBool::new(false);
    // Set when dfu-util sends DFU_CLRSTATUS (0x04), which happens only after dfuERROR.
    static ERROR_OCCURRED: AtomicBool = AtomicBool::new(false);

    struct DeferredReset;
    impl embassy_usb_dfu::Reset for DeferredReset {
        fn sys_reset(&self) {
            RESET_REQUESTED.store(true, Ordering::Release);
        }
    }

    // Spy handler: watches for DFU_DNLOAD and DFU_CLRSTATUS.  Returns None so
    // the DFU control handler still processes the request normally.
    struct DfuSpy;
    impl embassy_usb::Handler for DfuSpy {
        fn control_out(&mut self, req: Request, _data: &[u8]) -> Option<OutResponse> {
            if req.request_type == RequestType::Class && req.recipient == Recipient::Interface {
                match req.request {
                    0x01 => DOWNLOAD_ACTIVE.store(true, Ordering::Release), // DFU_DNLOAD
                    0x04 => ERROR_OCCURRED.store(true, Ordering::Release),  // DFU_CLRSTATUS
                    _ => {}
                }
            }
            None
        }
    }

    defmt::info!("Bootloader DFU mode — waiting for firmware via USB");

    let mut led_red = Output::new(led_red_pin, Level::High, OutputDrive::Standard);
    let mut led_blue = Output::new(led_blue_pin, Level::High, OutputDrive::Standard);
    let mut led_green = Output::new(led_green_pin, Level::High, OutputDrive::Standard);

    // Set up BlockingFirmwareUpdater backed by NVMC. Partition addresses come
    // from the linker symbols defined in memory.x.
    let flash = Mutex::<NoopRawMutex, _>::new(RefCell::new(Nvmc::new(nvmc)));
    let config = FirmwareUpdaterConfig::from_linkerfile_blocking(&flash, &flash);
    let mut aligned = AlignedBuffer([0u8; 4]);
    let updater = BlockingFirmwareUpdater::new(config, &mut aligned.0);
    let mut control =
        Control::<_, _, _, 4096>::new(updater, DfuAttributes::CAN_DOWNLOAD, DeferredReset);

    // Build the USB device with a single DFU interface.
    let driver = Driver::new(usbd, Irqs, HardwareVbusDetect::new(Irqs));

    let mut usb_config = embassy_usb::Config::new(0x1915, 0x521f);
    usb_config.manufacturer = Some("Badge.Team");
    usb_config.product = Some("CyberAegg Bootloader");
    usb_config.max_packet_size_0 = 64;

    let mut config_descriptor = [0u8; 256];
    let mut bos_descriptor = [0u8; 256];
    // Must be at least BLOCK_SIZE (4096) to receive DFU_DNLOAD payloads on EP0.
    let mut control_buf = [0u8; 4096];

    // Spy must be declared before builder so it outlives it (drop order is reversed).
    // It must also be registered before usb_dfu so it sees requests first.
    let mut spy = DfuSpy;

    let mut builder = Builder::new(
        driver,
        usb_config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [],
        &mut control_buf,
    );

    builder.handler(&mut spy);
    usb_dfu::<_, _, _, _, 4096>(&mut builder, &mut control, |_| {});

    let mut usb = builder.build();

    // Run USB + LED until the DFU download completes.
    // When DeferredReset::sys_reset() fires it sets RESET_REQUESTED and the
    // watcher future returns, causing select to drop the USB future.  The USB
    // peripheral stops, the host receives a clean disconnect, then we reset.
    let result = embassy_futures::select::select(
        embassy_futures::join::join(usb.run(), async {
            loop {
                if DOWNLOAD_ACTIVE.load(Ordering::Acquire) {
                    // Solid blue during download.
                    led_red.set_high();
                    led_blue.set_low();
                    Timer::after_millis(100).await;
                } else {
                    // Blink red while waiting for dfu-util to connect.
                    led_blue.set_high();
                    led_red.set_low();
                    Timer::after_millis(500).await;
                    led_red.set_high();
                    Timer::after_millis(500).await;
                }
            }
        }),
        async {
            loop {
                if RESET_REQUESTED.load(Ordering::Acquire) {
                    // Let the final status response flush before dropping USB.
                    Timer::after_millis(50).await;
                    break true; // success
                }
                if ERROR_OCCURRED.load(Ordering::Acquire) {
                    Timer::after_millis(50).await;
                    break false; // error
                }
                Timer::after_millis(10).await;
            }
        },
    )
    .await;

    // USB future dropped — peripheral is stopped, host sees disconnect.
    led_blue.set_high();
    let success = matches!(result, embassy_futures::select::Either::Second(true));
    if success {
        defmt::info!("DFU complete — resetting");
        for _ in 0..3 {
            led_green.set_low();
            Timer::after_millis(150).await;
            led_green.set_high();
            Timer::after_millis(150).await;
        }
    } else {
        defmt::warn!("DFU error — resetting");
        for _ in 0..3 {
            led_red.set_low();
            Timer::after_millis(150).await;
            led_red.set_high();
            Timer::after_millis(150).await;
        }
    }
    cortex_m::peripheral::SCB::sys_reset();
}

#[entry]
fn main() -> ! {
    let p = embassy_nrf::init(Default::default());

    // Check whether the execute button is held at boot.
    // If so, enter USB DFU mode instead of booting the application.
    #[cfg(feature = "dfu")]
    {
        use embassy_nrf::gpio::{Input, Pull};
        use static_cell::StaticCell;

        let led_red = board!(p, led_red).into();
        let led_blue = board!(p, led_blue).into();
        let led_green = board!(p, led_green).into();

        let btn_exe = Input::new(board!(p, btn_exe), Pull::Up);
        let dfu_requested = btn_exe.is_low();
        drop(btn_exe);

        if dfu_requested {
            defmt::info!("btn_exe held — entering DFU mode");
            static EXECUTOR: StaticCell<embassy_executor::Executor> = StaticCell::new();
            let executor = EXECUTOR.init(embassy_executor::Executor::new());
            // executor.run() never returns; device resets after DFU completes.
            executor.run(|spawner| {
                spawner
                    .spawn(dfu_task(p.USBD, p.NVMC, led_red, led_blue, led_green))
                    .unwrap();
            });
        }
    }

    // Normal boot path — start watchdog and boot the active firmware.
    let mut wdt_config = wdt::Config::default();
    wdt_config.timeout_ticks = 32768 * 20; // 20 s timeout
    wdt_config.action_during_sleep = SleepConfig::RUN;
    wdt_config.action_during_debug_halt = HaltConfig::PAUSE;

    let flash = WatchdogFlash::start(Nvmc::new(p.NVMC), p.WDT, wdt_config);
    let flash = Mutex::new(RefCell::new(flash));

    let config = BootLoaderConfig::from_linkerfile_blocking(&flash, &flash, &flash);
    let active_offset = config.active.offset();
    let bl: BootLoader = BootLoader::prepare(config);

    unsafe { bl.load(active_offset) }
}

#[unsafe(no_mangle)]
#[cfg_attr(target_os = "none", unsafe(link_section = ".HardFault.user"))]
unsafe extern "C" fn HardFault() {
    cortex_m::peripheral::SCB::sys_reset();
}

#[exception]
unsafe fn DefaultHandler(_: i16) -> ! {
    const SCB_ICSR: *const u32 = 0xE000_ED04 as *const u32;
    let irqn = unsafe { core::ptr::read_volatile(SCB_ICSR) } as u8 as i16 - 16;
    panic!("DefaultHandler #{:?}", irqn);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    cortex_m::asm::udf();
}

#[cfg(feature = "defmt")]
#[unsafe(no_mangle)]
fn _defmt_panic() -> ! {
    cortex_m::asm::udf();
}

#[cfg(feature = "defmt")]
defmt::timestamp!("");
