//! BLE peripheral using TrouBLE (trouble-host) over nrf-sdc/nrf-mpsl.
//!
//! Exposes a Nordic UART Service (NUS) for MeshCore companion app connectivity.
//! Bonding keys are persisted to QSPI flash via `flash_task`; see flash.rs.

use embassy_executor::Spawner;
use embassy_nrf::{Peri, bind_interrupts, mode::Blocking, peripherals, rng};
use nrf_mpsl::MultiprotocolServiceLayer;
use nrf_sdc::{self as sdc, SoftdeviceController};
use static_cell::StaticCell;
use trouble_host::prelude::*;

use crate::fw::bonds::{BOND_CMD_CHANNEL, INITIAL_BONDS, BondCmd};

// ---------------------------------------------------------------------------
// Interrupt bindings for MPSL + RNG
// ---------------------------------------------------------------------------

bind_interrupts!(pub struct BleIrqs {
    EGU0_SWI0   => nrf_mpsl::LowPrioInterruptHandler;
    CLOCK_POWER => nrf_mpsl::ClockInterruptHandler;
    RADIO       => nrf_mpsl::HighPrioInterruptHandler;
    TIMER0      => nrf_mpsl::HighPrioInterruptHandler;
    RTC0        => nrf_mpsl::HighPrioInterruptHandler;
    RNG         => rng::InterruptHandler<peripherals::RNG>;
});

// ---------------------------------------------------------------------------
// MPSL task
// ---------------------------------------------------------------------------

static MPSL: StaticCell<MultiprotocolServiceLayer<'static>> = StaticCell::new();

#[embassy_executor::task]
async fn mpsl_task(mpsl: &'static MultiprotocolServiceLayer<'static>) -> ! {
    mpsl.run().await
}

// ---------------------------------------------------------------------------
// MPSL + SDC initialisation
// ---------------------------------------------------------------------------

/// Initialise the Multiprotocol Service Layer and SoftDevice Controller.
///
/// Returns the SDC and a blocking RNG — keep both alive in main's scope.
/// Initialise MPSL + SDC.  Called from `#[embassy_executor::main]` where all
/// `Peri<'d, T>` tokens are `'static`, so the `'d: 'static` bound is satisfied.
pub fn init_ble(
    spawner: &Spawner,
    // MPSL
    rtc0:     Peri<'static, peripherals::RTC0>,
    timer0:   Peri<'static, peripherals::TIMER0>,
    temp:     Peri<'static, peripherals::TEMP>,
    ppi_ch19: Peri<'static, peripherals::PPI_CH19>,
    ppi_ch30: Peri<'static, peripherals::PPI_CH30>,
    ppi_ch31: Peri<'static, peripherals::PPI_CH31>,
    // SDC
    ppi_ch17: Peri<'static, peripherals::PPI_CH17>,
    ppi_ch18: Peri<'static, peripherals::PPI_CH18>,
    ppi_ch20: Peri<'static, peripherals::PPI_CH20>,
    ppi_ch21: Peri<'static, peripherals::PPI_CH21>,
    ppi_ch22: Peri<'static, peripherals::PPI_CH22>,
    ppi_ch23: Peri<'static, peripherals::PPI_CH23>,
    ppi_ch24: Peri<'static, peripherals::PPI_CH24>,
    ppi_ch25: Peri<'static, peripherals::PPI_CH25>,
    ppi_ch26: Peri<'static, peripherals::PPI_CH26>,
    ppi_ch27: Peri<'static, peripherals::PPI_CH27>,
    ppi_ch28: Peri<'static, peripherals::PPI_CH28>,
    ppi_ch29: Peri<'static, peripherals::PPI_CH29>,
    // RNG
    rng_periph: Peri<'static, peripherals::RNG>,
    sdc_mem: &'static mut sdc::Mem<4096>,
) -> SoftdeviceController<'static> {
    // 32 kHz crystal fitted on the board.
    let lfclk_cfg = nrf_mpsl::raw::mpsl_clock_lfclk_cfg_t {
        source: nrf_mpsl::raw::MPSL_CLOCK_LF_SRC_XTAL as u8,
        rc_ctiv: 0,
        rc_temp_ctiv: 0,
        accuracy_ppm: 20,
        skip_wait_lfclk_started: false,
    };

    let mpsl_p = nrf_mpsl::Peripherals::new(rtc0, timer0, temp, ppi_ch19, ppi_ch30, ppi_ch31);
    let mpsl = MPSL.init(
        nrf_mpsl::MultiprotocolServiceLayer::new(mpsl_p, BleIrqs, lfclk_cfg).unwrap(),
    );
    spawner.must_spawn(mpsl_task(mpsl));

    let sdc_p = sdc::Peripherals::new(
        ppi_ch17, ppi_ch18, ppi_ch20, ppi_ch21, ppi_ch22, ppi_ch23,
        ppi_ch24, ppi_ch25, ppi_ch26, ppi_ch27, ppi_ch28, ppi_ch29,
    );

    // nrf-sdc 0.4: build() takes `rng: &'static mut Rng` and stores a raw pointer to it
    // in a global for use by the SDC's random callback.  StaticCell gives us the 'static
    // storage; the peripheral token is already 'static so no unsafe is needed.
    static RNG_STORAGE: StaticCell<rng::Rng<'static, Blocking>> = StaticCell::new();
    let rng_ref = RNG_STORAGE.init(rng::Rng::new_blocking(rng_periph));

    // In nrf-sdc 0.4, support_adv/support_peripheral return Self directly (not Result).
    let sdc = sdc::Builder::new()
        .unwrap()
        .support_adv()
        .support_peripheral()
        .peripheral_count(1)
        .unwrap()
        .build(sdc_p, rng_ref, mpsl, sdc_mem)
        .unwrap();

    defmt::info!("BLE: MPSL + SDC initialised");
    sdc
}

// ---------------------------------------------------------------------------
// Nordic UART Service (NUS) GATT definition
// ---------------------------------------------------------------------------

/// NUS service UUID: 6E400001-B5A3-F393-E0A9-E50E24DCCA9E
#[gatt_service(uuid = "6e400001-b5a3-f393-e0a9-e50e24dcca9e")]
pub struct NusService {
    /// RX characteristic — phone writes frames to the badge.
    #[characteristic(
        uuid = "6e400002-b5a3-f393-e0a9-e50e24dcca9e",
        write,
        write_without_response
    )]
    pub rx: [u8; 20],

    /// TX characteristic — badge notifies frames to the phone.
    #[characteristic(uuid = "6e400003-b5a3-f393-e0a9-e50e24dcca9e", notify)]
    pub tx: [u8; 20],
}

#[gatt_server]
pub struct NusServer {
    pub nus: NusService,
}

// ---------------------------------------------------------------------------
// BLE peripheral runner
// ---------------------------------------------------------------------------

const DEVICE_ADDR: [u8; 6] = [0xC0, 0xFF, 0xEE, 0xBA, 0xBE, 0x01];

type BleResources = HostResources<DefaultPacketPool, 1, 2>;

#[embassy_executor::task]
pub async fn run_ble_peripheral(sdc: SoftdeviceController<'static>) {
    static RESOURCES: StaticCell<BleResources> = StaticCell::new();
    let resources = RESOURCES.init(BleResources::new());

    // TODO: seed the security manager PRNG properly before enabling bonding.
    // The nrf-sdc 0.4 API borrows `rng` for the SDC lifetime, making it unavailable here.
    // See Cargo.toml comment on dev-disable-csprng-seed-requirement for context.
    let stack = trouble_host::new(sdc, resources)
        .set_random_address(Address::random(DEVICE_ADDR));

    stack.set_io_capabilities(IoCapabilities::NoInputNoOutput);

    // Restore bonds loaded from flash by flash_task.
    // Spin briefly if flash_task hasn't populated INITIAL_BONDS yet.
    loop {
        if let Some(bonds) = INITIAL_BONDS.try_get() {
            for bond in bonds.iter() {
                let _ = stack.add_bond_information(bond.clone());
            }
            defmt::info!("BLE: restored {} bond(s)", bonds.len());
            break;
        }
        embassy_time::Timer::after_millis(1).await;
    }

    let Host { mut peripheral, mut runner, .. } = stack.build();

    let bond_tx = BOND_CMD_CHANNEL.sender();

    // Run the HCI runner in parallel with the advertising loop.
    embassy_futures::join::join(
        async { loop { if runner.run().await.is_err() {} } },
        nus_peripheral_loop(&mut peripheral, bond_tx),
    )
    .await;
}

async fn nus_peripheral_loop<C>(
    peripheral: &mut Peripheral<'_, C, DefaultPacketPool>,
    bond_tx: embassy_sync::channel::Sender<'static, embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, BondCmd, 4>,
) where
    C: Controller,
{
    // Build the device name: "Cyber Ægg XXYY" where XXYY is the two-byte device ID in hex.
    // Flags (3 B) + name (16 B) = 19 B — fits within the 31-byte adv packet limit.
    // The 128-bit NUS UUID (18 B) goes into scan_data so the total doesn't overflow.
    // "Cyber Ægg XXYY" — Æ (U+00C6) is 0xC3 0x86 in UTF-8, total 15 bytes.
    let id = crate::fw::device_id::get_bytes();
    let name: [u8; 15] = [
        b'C', b'y', b'b', b'e', b'r', b' ',
        0xC3, 0x86, b'g', b'g', b' ',
        id[0], id[1], id[2], id[3],
    ];
    // Safety: all bytes are valid UTF-8 (ASCII + the two-byte Æ sequence above).
    let name_str = unsafe { core::str::from_utf8_unchecked(&name) };

    let mut adv_buf = [0u8; 31];
    let adv_len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::CompleteLocalName(&name),
        ],
        &mut adv_buf,
    ).unwrap();

    let mut scan_buf = [0u8; 31];
    let scan_len = AdStructure::encode_slice(
        &[AdStructure::ServiceUuids128(&[
            [0x9e, 0xca, 0xdc, 0x24, 0x0e, 0xe5, 0xa9, 0xe0,
             0x93, 0xf3, 0xa3, 0xb5, 0x01, 0x00, 0x40, 0x6e],
        ])],
        &mut scan_buf,
    ).unwrap();

    let server = NusServer::new_default(name_str).unwrap();

    loop {
        defmt::debug!("BLE: advertising…");

        let advertiser = match peripheral
            .advertise(
                &Default::default(),
                Advertisement::ConnectableScannableUndirected {
                    adv_data:  &adv_buf[..adv_len],
                    scan_data: &scan_buf[..scan_len],
                },
            )
            .await
        {
            Ok(a) => a,
            Err(e) => {
                defmt::warn!("BLE: advertise error: {:?}", defmt::Debug2Format(&e));
                embassy_time::Timer::after_millis(500).await;
                continue;
            }
        };

        let conn = match advertiser.accept().await {
            Ok(c) => c,
            Err(e) => {
                defmt::warn!("BLE: accept error: {:?}", defmt::Debug2Format(&e));
                continue;
            }
        };

        defmt::info!("BLE: connected");

        let gatt_conn = match conn.with_attribute_server(&server.server) {
            Ok(c) => c,
            Err(e) => {
                defmt::warn!("BLE: gatt setup error: {:?}", defmt::Debug2Format(&e));
                continue;
            }
        };

        loop {
            match gatt_conn.next().await {
                GattConnectionEvent::Disconnected { reason } => {
                    defmt::info!("BLE: disconnected (reason {:?})", defmt::Debug2Format(&reason));
                    break;
                }
                GattConnectionEvent::PairingComplete { bond: Some(info), .. } => {
                    defmt::info!("BLE: pairing complete — persisting bond");
                    let _ = bond_tx.try_send(BondCmd::Save(info));
                }
                GattConnectionEvent::PairingFailed(e) => {
                    defmt::warn!("BLE: pairing failed: {:?}", defmt::Debug2Format(&e));
                }
                GattConnectionEvent::Gatt { event: GattEvent::Write(write) } => {
                    if write.handle() == server.nus.rx.handle {
                        // Hand the received frame to the application layer.
                        // TODO: forward to MeshCore companion protocol handler.
                        defmt::debug!("NUS RX: {} bytes", write.data().len());
                    }
                    if let Ok(reply) = write.accept() {
                        reply.send().await;
                    }
                }
                _ => {}
            }
        }
    }
}
