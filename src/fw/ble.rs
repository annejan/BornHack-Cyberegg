//! BLE peripheral using TrouBLE (trouble-host) over nrf-sdc/nrf-mpsl.
//!
//! Exposes a Nordic UART Service (NUS) for MeshCore companion app connectivity.
//! Bonding keys are persisted to QSPI flash via `flash_task`; see flash.rs.

use core::sync::atomic::Ordering;

use rand_core::{CryptoRng, RngCore};

/// Minimal RNG that yields TRNG entropy bytes directly.
///
/// `trouble-host` calls `fill_bytes` exactly once on the RNG passed to
/// `set_random_generator_seed` — just to extract 32 bytes of seed material.
/// Since `prng_seed` already contains raw TRNG output we can hand it over
/// directly, avoiding the full ChaCha20 implementation from `rand_chacha`.
struct TrngSeed([u8; 32]);
impl RngCore for TrngSeed {
    fn next_u32(&mut self) -> u32 { u32::from_le_bytes(self.0[..4].try_into().unwrap()) }
    fn next_u64(&mut self) -> u64 { u64::from_le_bytes(self.0[..8].try_into().unwrap()) }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for (i, b) in dest.iter_mut().enumerate() { *b = self.0[i % 32]; }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}
impl CryptoRng for TrngSeed {}

use meshcore_companion as companion;
use meshcore;

use embassy_executor::Spawner;
use embassy_nrf::{Peri, bind_interrupts, mode::Blocking, peripherals, rng};
use nrf_mpsl::MultiprotocolServiceLayer;
use nrf_sdc::{self as sdc, SoftdeviceController};
use static_cell::StaticCell;
use trouble_host::prelude::*;

use crate::fw::bonds::{BOND_CMD_CHANNEL, INITIAL_BONDS, BondCmd};
use crate::fw::{channels, contacts, msg_queue, settings};

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
// SDC configuration constants
// ---------------------------------------------------------------------------

/// L2CAP TX/RX queue depth per link.
/// Must match the value passed to `sdc::Builder::buffer_cfg`.
const L2CAP_TXQ: u8 = 3;
const L2CAP_RXQ: u8 = 3;

/// SDC heap size in bytes.
///
/// Sized for one peripheral link with `buffer_cfg(MTU=251, MTU=251, TXQ=3, RXQ=3)`.
/// Matches the official embassy-rs/trouble nrf52 examples (`Mem::<4720>`).
/// All callers must use this constant so the value stays in sync with `buffer_cfg`.
pub const SDC_MEM_SIZE: usize = 4720;

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
    sdc_mem: &'static mut sdc::Mem<SDC_MEM_SIZE>,
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

    // buffer_cfg tells the controller how large each L2CAP PDU buffer should be
    // and how many TX/RX slots to allocate per link.  Without this call the
    // controller defaults to 27-byte (bare PDU) buffers, which forces the host
    // to fragment every packet into tiny pieces and reliably drops connections.
    // Values must match DefaultPacketPool::MTU (251) used by trouble-host so
    // the host packet pool and the controller slots agree on the max frame size.
    // L2CAP_TXQ / L2CAP_RXQ = 3 matches the official nrf52 trouble examples.
    let sdc = sdc::Builder::new()
        .unwrap()
        .support_adv()
        .support_peripheral()
        .peripheral_count(1)
        .unwrap()
        .buffer_cfg(
            DefaultPacketPool::MTU as u16,
            DefaultPacketPool::MTU as u16,
            L2CAP_TXQ,
            L2CAP_RXQ,
        )
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
    /// Max size 244 B = ATT MTU 247 − 3 overhead.  SetChannel alone needs
    /// 1 (index) + 32 (name) + 16 (key) = 49 B, so 20 B is too small.
    #[characteristic(
        uuid = "6e400002-b5a3-f393-e0a9-e50e24dcca9e",
        write,
        write_without_response
    )]
    pub rx: heapless::Vec<u8, 244>,

    /// TX characteristic — badge notifies frames to the phone.
    /// Up to 244 bytes (ATT MTU 247 − 3 overhead) in a single notification.
    #[characteristic(uuid = "6e400003-b5a3-f393-e0a9-e50e24dcca9e", notify)]
    pub tx: heapless::Vec<u8, 244>,
}

#[gatt_server]
pub struct NusServer {
    pub nus: NusService,
}

// ---------------------------------------------------------------------------
// Companion protocol context + helpers
// ---------------------------------------------------------------------------

/// Device information snapshot passed to the companion protocol handler.
/// Filled in by `embassy.rs` at startup from the device identity.
/// Radio parameters are loaded from [`settings`] directly inside the BLE task.
pub struct CompanionContext {
    /// Ed25519 public key (32 bytes).
    pub pub_key: [u8; 32],
}

/// Send `data` as a single BLE notification.
///
/// With ATT MTU 247 (negotiated after connection) up to 244 bytes fit in one
/// notification — larger than any response we produce (max 128 B).  Sending
/// as one frame matches the MeshCore reference firmware which does a single
/// `bleuart.write(buf, len)` and lets the BLE stack handle fragmentation.
///
/// A 2-second timeout prevents a dropped connection from causing a permanent
/// hang inside the GATT write handler.
/// A pre-serialised notification payload ready to hand to `tx.notify`.
type OutboxEntry = ([u8; companion::MAX_RESPONSE_LEN], usize);

/// Static backing store for the per-connection outbox.
///
/// Keeping this in `.bss` rather than on the BLE task stack avoids a ~4 KiB
/// stack allocation that was overflowing and corrupting embassy-sync internals.
static OUTBOX_STORAGE: StaticCell<heapless::Deque<OutboxEntry, 4>> = StaticCell::new();

/// Encode a [`companion::Response`] and push it onto the outbox.
/// Drops the entry with a warning if the outbox is full.
fn enqueue_notify(outbox: &mut heapless::Deque<OutboxEntry, 4>, response: &companion::Response<'_>) {
    let mut entry: OutboxEntry = ([0u8; companion::MAX_RESPONSE_LEN], 0);
    entry.1 = companion::encode(response, &mut entry.0);
    if outbox.push_back(entry).is_err() {
        defmt::warn!("companion: outbox full, dropping notification");
    }
}


// ---------------------------------------------------------------------------
// BLE peripheral runner
// ---------------------------------------------------------------------------

type BleResources = HostResources<DefaultPacketPool, 1, 2>;

#[embassy_executor::task]
pub async fn run_ble_peripheral(sdc: SoftdeviceController<'static>, ctx: CompanionContext, prng_seed: [u8; 32]) {
    static RESOURCES: StaticCell<BleResources> = StaticCell::new();
    let resources = RESOURCES.init(BleResources::new());

    // Seed the security manager PRNG from TRNG entropy collected at startup
    // (before the RNG peripheral was consumed by the SDC).
    let mut prng = TrngSeed(prng_seed);

    let stack = trouble_host::new(sdc, resources)
        .set_random_address(Address::random(crate::fw::device_id::get_ble_addr()))
        .set_random_generator_seed(&mut prng);

    // DisplayOnly: badge shows a 6-digit passkey on screen; the phone user enters it.
    // This matches MeshCore's setIOCaps(true, false, false) and enables MITM protection.
    stack.set_io_capabilities(IoCapabilities::DisplayOnly);

    // Restore bonds loaded from flash by flash_task.
    // Spin briefly if flash_task hasn't populated INITIAL_BONDS yet.
    loop {
        if let Some(bonds) = INITIAL_BONDS.try_get() {
            for (i, bond) in bonds.iter().enumerate() {
                let addr = bond.identity.bd_addr.into_inner();
                match stack.add_bond_information(bond.clone()) {
                    Ok(()) => defmt::debug!(
                        "BLE: restored bond[{}] addr={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                        i, addr[0], addr[1], addr[2], addr[3], addr[4], addr[5]
                    ),
                    Err(e) => defmt::warn!(
                        "BLE: failed to restore bond[{}]: {:?}",
                        i, defmt::Debug2Format(&e)
                    ),
                }
            }
            defmt::info!("BLE: restored {} bond(s) from flash", bonds.len());
            break;
        }
        embassy_time::Timer::after_millis(1).await;
    }

    let Host { mut peripheral, mut runner, .. } = stack.build();

    let bond_tx = BOND_CMD_CHANNEL.sender();
    channels::init().await;
    defmt::info!("BLE: channel store ready ({} active)", channels::count_active().await);

    // Run the HCI runner in parallel with the advertising loop.
    embassy_futures::join::join(
        async { loop { if runner.run().await.is_err() {} } },
        nus_peripheral_loop(&mut peripheral, bond_tx, &ctx),
    )
    .await;
}

async fn nus_peripheral_loop<C>(
    peripheral: &mut Peripheral<'_, C, DefaultPacketPool>,
    bond_tx: embassy_sync::channel::Sender<'static, embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, BondCmd, 4>,
    ctx: &CompanionContext,
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

    // Initialise static outbox storage once — cleared on each new connection.
    let outbox: &mut heapless::Deque<OutboxEntry, 4> = OUTBOX_STORAGE.init(heapless::Deque::new());

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

    // Load the persisted node name (set via CMD_SET_ADVERT_NAME 0x08).
    // Falls back to the 4-byte hardware device-ID hex string on first boot.
    let mut node_name = [0u8; settings::MAX_NODE_NAME];
    let mut node_name_len: usize = {
        let n = settings::get_node_name(&mut node_name).await;
        if n == 0 {
            let fb = crate::fw::device_id::get_bytes();
            node_name[..4].copy_from_slice(&fb);
            4
        } else {
            n
        }
    };
    crate::update_node_name(&node_name[..node_name_len]);

    // Load the persisted boost-RX flag and timezone offset.
    crate::BOOSTED_RX_GAIN.store(settings::get_boost_rx().await, core::sync::atomic::Ordering::Relaxed);
    crate::TIMEZONE_OFFSET.store(settings::get_timezone().await, core::sync::atomic::Ordering::Relaxed);

    // Load the persisted radio parameters (set via CMD_SET_RADIO_PARAMS 0x0B /
    // CMD_SET_RADIO_TX_POWER 0x0C).  Falls back to EU/UK narrow band defaults.
    let mut radio_params = settings::get_radio_params_or_default().await;

    // Load the persisted GPS position (set via CMD_SET_ADVERT_LATLON 0x0E).
    // Falls back to (0, 0) on first boot (meaning "no position set").
    let mut position = settings::get_position_or_default().await;

    // Load other params (set via CMD_SET_OTHER_PARAMS 0x26).
    // Falls back to all-zero defaults on first boot.
    let mut other_params = settings::get_other_params().await.unwrap_or(settings::OtherParams {
        manual_add_contacts: 0,
        telemetry_mode_base: 0,
        telemetry_mode_loc:  0,
        telemetry_mode_env:  0,
        advert_loc_policy:   0,
        multi_acks:          0,
    });

    loop {
        // Handle channel reset request from the menu (fires between connections).
        if crate::CHANNEL_RESET_SIGNAL.signaled() {
            crate::CHANNEL_RESET_SIGNAL.reset();
            channels::reset().await;
        }
        // Persist boost-RX flag when toggled from the menu.
        if crate::BOOST_RX_CHANGED_SIGNAL.signaled() {
            crate::BOOST_RX_CHANGED_SIGNAL.reset();
            let enabled = crate::BOOSTED_RX_GAIN.load(core::sync::atomic::Ordering::Relaxed);
            match settings::set_boost_rx(enabled).await {
                Ok(()) => defmt::info!("settings: boost_rx={} persisted", enabled),
                Err(e) => defmt::warn!("settings: boost_rx persist failed: {:?}", e),
            }
        }
        // Persist timezone offset when changed from the menu.
        if crate::TZ_CHANGED_SIGNAL.signaled() {
            crate::TZ_CHANGED_SIGNAL.reset();
            let offset = crate::TIMEZONE_OFFSET.load(core::sync::atomic::Ordering::Relaxed);
            match settings::set_timezone(offset).await {
                Ok(()) => defmt::info!("settings: timezone={} persisted", offset),
                Err(e) => defmt::warn!("settings: timezone persist failed: {:?}", e),
            }
        }
        // Clear all contacts when requested from the menu.
        if crate::CONTACT_RESET_SIGNAL.signaled() {
            crate::CONTACT_RESET_SIGNAL.reset();
            defmt::info!("settings: clearing all contacts");
            crate::fw::contacts::ContactStore::new().clear_all().await;
        }

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

        // Enable bonding so the security manager hands us the LTK after pairing
        // and sets the bonding bit in the local AuthReq.  Without this,
        // storage.bondable stays false (the connection-manager default) and
        // PairingComplete always returns bond: None.
        if let Err(e) = conn.set_bondable(true) {
            defmt::warn!("BLE: set_bondable failed: {:?}", defmt::Debug2Format(&e));
        }

        let gatt_conn = match conn.with_attribute_server(&server.server) {
            Ok(c) => c,
            Err(e) => {
                defmt::warn!("BLE: gatt setup error: {:?}", defmt::Debug2Format(&e));
                continue;
            }
        };

        // Clear any stale signal, then re-arm if there are undelivered messages.
        crate::MESSAGES_WAITING_SIGNAL.reset();
        if !msg_queue::is_empty() {
            crate::MESSAGES_WAITING_SIGNAL.signal(());
        }

        // Per-connection outbound notification queue (backed by static storage).
        // Cleared on each new connection; drained one entry per loop iteration,
        // raced against incoming GATT events so we never block event handling
        // waiting for an L2CAP TX credit.
        outbox.clear();

        // Lazy contact streaming state: (next slot index to probe, contacts
        // remaining to send).  Populated when GET_CONTACTS is received and
        // drained one KV read per loop iteration.
        let mut contacts_stream: Option<(usize, u16)> = None;

        loop {
            use embassy_futures::select::{Either, Either6, select, select6};

            // ---------------------------------------------------------------
            // Lazy contact streaming: emit one slot per iteration into outbox.
            // ---------------------------------------------------------------
            if let Some((ref mut slot, ref mut remaining)) = contacts_stream {
                if *slot >= contacts::MAX_CONTACTS || *remaining == 0 {
                    enqueue_notify(outbox, &companion::Response::ContactEnd);
                    defmt::info!("companion: GET_CONTACTS complete");
                    contacts_stream = None;
                } else {
                    let c = contacts::ContactStore::new().read_slot(*slot).await;
                    *slot += 1;
                    if let Some(c) = c {
                        let mut prefix = [0u8; 6];
                        prefix.copy_from_slice(&c.pub_key[..6]);
                        let name_end = c.name.iter().position(|&b| b == 0).unwrap_or(32);
                        enqueue_notify(outbox, &companion::Response::Contact(companion::response::Contact {
                            pub_key_prefix: prefix,
                            flags:     c.flags,
                            last_seen: c.last_advert_ts,
                            name:      &c.name[..name_end],
                        }));
                        *remaining = remaining.saturating_sub(1);
                    }
                }
            }

            // ---------------------------------------------------------------
            // Race: drain outbox front vs handle incoming events.
            // If the outbox is empty we just wait for events.
            // ---------------------------------------------------------------
            let incoming = if let Some((buf, len)) = outbox.front().copied() {
                let mut vec: heapless::Vec<u8, 244> = heapless::Vec::new();
                let _ = vec.extend_from_slice(&buf[..len.min(244)]);
                match select(
                    server.nus.tx.notify(&gatt_conn, &vec),
                    select6(
                        gatt_conn.next(),
                        crate::MESSAGES_WAITING_SIGNAL.wait(),
                        crate::RAW_PKT_CHANNEL.receive(),
                        crate::ADVERT_BLE_CHANNEL.receive(),
                        crate::TRACE_RESULT_CHANNEL.receive(),
                        select(
                            crate::LOGIN_RESULT_CHANNEL.receive(),
                            select(
                                crate::STATUS_RESULT_CHANNEL.receive(),
                                select(
                                    crate::ACK_EVENT_CHANNEL.receive(),
                                    crate::TELEM_RESULT_CHANNEL.receive(),
                                ),
                            ),
                        ),
                    ),
                ).await {
                    Either::First(r) => {
                        if let Err(e) = r {
                            defmt::warn!("companion: notify failed: {:?}", defmt::Debug2Format(&e));
                        }
                        outbox.pop_front();
                        continue;
                    }
                    Either::Second(ev) => ev,
                }
            } else {
                select6(
                    gatt_conn.next(),
                    crate::MESSAGES_WAITING_SIGNAL.wait(),
                    crate::RAW_PKT_CHANNEL.receive(),
                    crate::ADVERT_BLE_CHANNEL.receive(),
                    crate::TRACE_RESULT_CHANNEL.receive(),
                    select(
                        crate::LOGIN_RESULT_CHANNEL.receive(),
                        select(
                            crate::STATUS_RESULT_CHANNEL.receive(),
                            select(
                                crate::ACK_EVENT_CHANNEL.receive(),
                                crate::TELEM_RESULT_CHANNEL.receive(),
                            ),
                        ),
                    ),
                ).await
            };

            match incoming {
                // -----------------------------------------------------------
                // GATT event
                // -----------------------------------------------------------
                Either6::First(event) => match event {
                    GattConnectionEvent::Disconnected { reason } => {
                        defmt::info!("BLE: disconnected (reason {:?})", defmt::Debug2Format(&reason));
                        crate::BLE_PASSKEY.store(u32::MAX, Ordering::Relaxed);
                        crate::BLE_PAIRING_SIGNAL.signal(());
                        break;
                    }
                    GattConnectionEvent::PassKeyDisplay(key) => {
                        defmt::info!("BLE: pairing passkey: {:06}", key.value());
                        crate::BLE_PASSKEY.store(key.value(), Ordering::Relaxed);
                        crate::BLE_PAIRING_SIGNAL.signal(());
                    }
                    GattConnectionEvent::PairingComplete { bond, security_level } => {
                        defmt::info!("BLE: pairing complete (level {:?})", defmt::Debug2Format(&security_level));
                        crate::BLE_PASSKEY.store(u32::MAX, Ordering::Relaxed);
                        crate::BLE_PAIRING_SIGNAL.signal(());
                        if let Some(info) = bond {
                            defmt::info!("BLE: new bond — persisting");
                            let _ = bond_tx.try_send(BondCmd::Save(info));
                        }
                    }
                    GattConnectionEvent::PairingFailed(e) => {
                        defmt::warn!("BLE: pairing failed: {:?}", defmt::Debug2Format(&e));
                        crate::BLE_PASSKEY.store(u32::MAX, Ordering::Relaxed);
                        crate::BLE_PAIRING_SIGNAL.signal(());
                    }
                    GattConnectionEvent::Gatt { event: GattEvent::Write(write) } => {
                        if write.handle() == server.nus.rx.handle {
                            let sec = gatt_conn.raw().security_level().unwrap_or(SecurityLevel::NoEncryption);
                            if !sec.authenticated() {
                                defmt::warn!("companion: unauthenticated write — sending INSUFFICIENT_AUTHENTICATION");
                                if let Ok(reply) = write.reject(AttErrorCode::INSUFFICIENT_AUTHENTICATION) {
                                    reply.send().await;
                                }
                                continue;
                            }

                            let data = write.data();

                            // Pre-pop for SYNC_NEXT_MESSAGE (0x0A) before building the
                            // response, so the owned message outlives the match.
                            let popped = if data.first() == Some(&0x0A) {
                                msg_queue::pop().await
                            } else {
                                None
                            };

                            // Pre-read contact count for GET_CONTACTS (0x04) before the
                            // match so we can pick ContactStart vs NoMoreMsgs up-front.
                            let contacts_count = if data.first() == Some(&0x04) {
                                contacts::ContactStore::new().count().await
                            } else {
                                0u16
                            };

                            // Pre-fetch full contact for GET_CONTACT_BY_KEY (0x1E)
                            // and GET_ADVERT_PATH (0x2A).
                            let contact_by_key: Option<contacts::Contact> =
                                if data.first() == Some(&0x1E) && data.len() >= 33 {
                                    let key: [u8; 32] = data[1..33].try_into().unwrap();
                                    contacts::ContactStore::new().find_by_key(&key).await
                                } else if data.first() == Some(&0x2A) && data.len() >= 34 {
                                    let key: [u8; 32] = data[2..34].try_into().unwrap();
                                    contacts::ContactStore::new().find_by_key(&key).await
                                } else {
                                    None
                                };

                            // Declared before the match so mutations can happen after encode.
                            let mut pending_name:     Option<([u8; settings::MAX_NODE_NAME], usize)> = None;
                            let mut pending_radio:    Option<settings::RadioParams> = None;
                            let mut pending_position: Option<settings::Position> = None;
                            let mut pending_other:    Option<settings::OtherParams> = None;
                            let mut pending_reboot:   bool = false;
                            let mut pending_contact:  Option<contacts::Contact> = None;
                            // Self-telemetry LPP buffer: voltage (4B) + temperature (4B).
                            let mut self_telem_lpp: [u8; 8] = [0u8; 8];
                            let mut self_telem_lpp_len: usize = 4;
                            let mut self_telem_prefix: [u8; 6] = [0u8; 6];
                            let response = match companion::cmd::parse(data) {
                                Err(_) => {
                                    defmt::warn!("companion: empty write");
                                    companion::Response::Error(companion::ErrorCode::Generic)
                                }

                                Ok(companion::cmd::Command::AppStart) => {
                                    companion::Response::SelfInfo(companion::SelfInfo {
                                        adv_type: 1,
                                        tx_power: radio_params.tx_power,
                                        max_tx_power: 22,
                                        pub_key: &ctx.pub_key,
                                        lat: position.lat,
                                        lon: position.lon,
                                        multi_acks: other_params.multi_acks,
                                        adv_location_policy: other_params.advert_loc_policy,
                                        telemetry_mode: other_params.telemetry_mode_base
                                            | (other_params.telemetry_mode_loc << 2)
                                            | (other_params.telemetry_mode_env << 4),
                                        manual_add_contacts: other_params.manual_add_contacts,
                                        frequency_hz: radio_params.freq_hz,
                                        bandwidth_hz: radio_params.bw_hz,
                                        spreading_factor: radio_params.sf,
                                        coding_rate: radio_params.cr,
                                        name: &node_name[..node_name_len],
                                    })
                                }

                                Ok(companion::cmd::Command::DeviceQuery(ver)) => {
                                    defmt::info!("companion: DEVICE_QUERY ver={=u8}", ver);
                                    companion::Response::DeviceInfo(companion::DeviceInfo {
                                        fw_version: 3,
                                        // Protocol encodes capacity as (actual ÷ 2); u8 max = 255
                                        // so we saturate at 510 (255 × 2) as the reported limit.
                                        max_contacts_raw: (contacts::MAX_CONTACTS / 2).min(u8::MAX as usize) as u8,
                                        max_channels: channels::NUM_CHANNELS as u8,
                                        ble_pin: {
                                            let v = crate::BLE_PASSKEY.load(Ordering::Relaxed);
                                            if v == u32::MAX { 0 } else { v }
                                        },
                                        fw_build: b"dev",
                                        model: b"BornHack Cyber\xC3\x86gg",
                                        version: b"0.1.0",
                                        client_repeat: false,
                                        path_hash_mode: 0,
                                    })
                                }

                                Ok(companion::cmd::Command::GetBattery) => {
                                    let mv = crate::fw::battery::read_mv();
                                    let pct = crate::fw::battery::read_pct();
                                    defmt::info!("companion: GET_BATT → BATTERY {} mV {}%", mv, pct);
                                    companion::Response::Battery {
                                        mv,
                                        used_kb: 0,
                                        total_kb: 8192,
                                    }
                                }

                                Ok(companion::cmd::Command::SyncNextMessage) => {
                                    match popped {
                                        Some(ref msg) => {
                                            let remaining = msg_queue::count();
                                            if remaining > 0 {
                                                crate::MESSAGES_WAITING_SIGNAL.signal(());
                                            }
                                            match msg.kind {
                                                msg_queue::MsgKind::Private => {
                                                    defmt::info!(
                                                        "companion: SYNC_NEXT_MESSAGE → private from={=[u8]:02x} ts={=u32} rssi={=i16} ({=u16} remaining)",
                                                        msg.sender_prefix, msg.timestamp, msg.rssi, remaining
                                                    );
                                                    companion::Response::ContactMsgRecvV3(companion::ContactMsgV3 {
                                                        rf_info:        [msg.rssi.unsigned_abs().min(255) as u8, 0, 0],
                                                        pub_key_prefix: msg.sender_prefix,
                                                        path_len:  msg.path_len,
                                                        text_type: msg.text_type,
                                                        timestamp: msg.timestamp,
                                                        signature: None,
                                                        text:      &msg.text,
                                                    })
                                                }
                                                msg_queue::MsgKind::Channel => {
                                                    defmt::info!(
                                                        "companion: SYNC_NEXT_MESSAGE → ch={=u8} ts={=u32} rssi={=i16} ({=u16} remaining)",
                                                        msg.channel_idx, msg.timestamp, msg.rssi, remaining
                                                    );
                                                    companion::Response::ChannelMsgRecvV3(companion::ChannelMsgV3 {
                                                        rf_info:   [msg.rssi.unsigned_abs().min(255) as u8, 0, 0],
                                                        channel:   msg.channel_idx,
                                                        path_len:  msg.path_len,
                                                        text_type: msg.text_type,
                                                        timestamp: msg.timestamp,
                                                        text:      &msg.text,
                                                    })
                                                }
                                            }
                                        }
                                        None => {
                                            defmt::info!("companion: SYNC_NEXT_MESSAGE → NO_MORE_MSGS");
                                            companion::Response::NoMoreMsgs
                                        }
                                    }
                                }

                                Ok(companion::cmd::Command::GetContacts) => {
                                    if contacts_count > 0 {
                                        contacts_stream = Some((0, contacts_count));
                                        companion::Response::ContactStart
                                    } else {
                                        companion::Response::NoMoreMsgs
                                    }
                                }

                                Ok(companion::cmd::Command::GetContactByKey(_key)) => {
                                    match contact_by_key {
                                        Some(ref c) => {
                                            let name_end = c.name.iter().position(|&b| b == 0).unwrap_or(32);
                                            companion::Response::ContactDetails(companion::response::NewAdvert {
                                                pub_key: &c.pub_key,
                                                adv_type: c.node_type,
                                                flags: c.flags,
                                                out_path_len: c.out_path_len,
                                                out_path: &c.out_path,
                                                name: &c.name[..name_end],
                                                last_advert_timestamp: c.last_advert_ts,
                                                gps_lat: c.gps_lat,
                                                gps_lon: c.gps_lon,
                                                lastmod: c.lastmod,
                                            })
                                        }
                                        None => {
                                            defmt::warn!("companion: GET_CONTACT_BY_KEY not found");
                                            companion::Response::Error(companion::ErrorCode::InvalidParameter)
                                        }
                                    }
                                }

                                Ok(companion::cmd::Command::GetChannel(idx)) => {
                                    if idx as usize >= channels::NUM_CHANNELS {
                                        companion::Response::NoMoreMsgs
                                    } else {
                                        let (name, key) = channels::get(idx).await
                                            .unwrap_or(([0u8; 32], [0u8; 16]));
                                        companion::Response::ChannelInfo(companion::ChannelInfo { index: idx, name, key })
                                    }
                                }

                                Ok(companion::cmd::Command::SendSelfAdvert(mode)) => {
                                    let advert_mode = if mode == 1 {
                                        crate::fw::meshcore::AdvertMode::Flood
                                    } else {
                                        crate::fw::meshcore::AdvertMode::ZeroHop
                                    };
                                    crate::SEND_ADVERT_SIGNAL.signal(advert_mode);
                                    defmt::info!("companion: SEND_SELF_ADVERT mode={=u8} → signalled", mode);
                                    companion::Response::Ok
                                }

                                Ok(companion::cmd::Command::ResetPath(key)) => {
                                    defmt::info!("companion: RESET_PATH key={=[u8]:02x}", &key[..6]);
                                    match contacts::ContactStore::new()
                                        .update_path(key, contacts::OUT_PATH_UNKNOWN, &[0u8; contacts::MAX_PATH_SIZE])
                                        .await
                                    {
                                        Ok(()) => companion::Response::Ok,
                                        Err(e) => {
                                            defmt::warn!("companion: RESET_PATH failed: {:?}", e);
                                            companion::Response::Error(companion::ErrorCode::Generic)
                                        }
                                    }
                                }

                                Ok(companion::cmd::Command::RemoveContact(key)) => {
                                    match contacts::ContactStore::new().delete(key).await {
                                        Ok(true) => {
                                            defmt::info!("companion: REMOVE_CONTACT deleted {:02x}", &key[..6]);
                                            companion::Response::Ok
                                        }
                                        Ok(false) => {
                                            defmt::warn!("companion: REMOVE_CONTACT not found");
                                            companion::Response::Error(companion::ErrorCode::InvalidParameter)
                                        }
                                        Err(e) => {
                                            defmt::warn!("companion: REMOVE_CONTACT failed: {:?}", e);
                                            companion::Response::Error(companion::ErrorCode::Generic)
                                        }
                                    }
                                }

                                Ok(companion::cmd::Command::AddUpdateContact) => {
                                    match contacts::Contact::from_add_update_payload(data) {
                                        Some(c) => {
                                            defmt::debug!("companion: ADD_UPDATE_CONTACT key={:02x}", &c.pub_key[..6]);
                                            pending_contact = Some(c);
                                            companion::Response::Ok
                                        }
                                        None => {
                                            defmt::warn!("companion: ADD_UPDATE_CONTACT payload too short");
                                            companion::Response::Error(companion::ErrorCode::InvalidParameter)
                                        }
                                    }
                                }

                                Ok(companion::cmd::Command::Reboot) => {
                                    defmt::info!("companion: REBOOT → scheduled");
                                    pending_reboot = true;
                                    companion::Response::Ok
                                }

                                Ok(companion::cmd::Command::SetDeviceTime(ts)) => {
                                    crate::set_wall_clock(ts);
                                    defmt::info!("companion: SET_DEVICE_TIME unix={}", ts);
                                    companion::Response::Ok
                                }

                                Ok(companion::cmd::Command::SetChannel { index, name: ch_name, key: ch_key }) => {
                                    if channels::set(index, ch_name, ch_key).await {
                                        defmt::info!("companion: SET_CHANNEL idx={=u8} → stored", index);
                                        crate::CHANNELS_CHANGED_SIGNAL.signal(());
                                        companion::Response::Ok
                                    } else {
                                        defmt::warn!("companion: SET_CHANNEL idx={=u8} out of range", index);
                                        companion::Response::Error(companion::ErrorCode::IndexOutOfRange)
                                    }
                                }

                                Ok(companion::cmd::Command::SendTxtMsg { txt_type, attempt, timestamp, pub_key_prefix, text }) => {
                                    // Look up the full pub_key by prefix scan.
                                    let recipient = contacts::ContactStore::new()
                                        .find_by_prefix(&pub_key_prefix)
                                        .await;
                                    match recipient {
                                        None => {
                                            defmt::warn!("companion: SEND_TXT_MSG recipient not found for prefix {=[u8]:02x}", pub_key_prefix);
                                            companion::Response::Error(companion::ErrorCode::InvalidParameter)
                                        }
                                        Some(c) => {
                                            let mut v: heapless::Vec<u8, { msg_queue::MAX_TEXT }> = heapless::Vec::new();
                                            let _ = v.extend_from_slice(&text[..text.len().min(msg_queue::MAX_TEXT)]);
                                            let is_flood = c.out_path_len == contacts::OUT_PATH_UNKNOWN;
                                            match crate::TX_PM_CHANNEL.try_send(crate::TxPrivateMsg {
                                                recipient_pub_key: c.pub_key,
                                                timestamp,
                                                text: v,
                                            }) {
                                                Ok(()) => {
                                                    // Compute expected_ack = SHA-256([ts:4][flags:1][text] || sender_pk)[0..4]
                                                    let flags = (attempt & 3) | (txt_type << 2);
                                                    let text_len = text.len().min(meshcore::payload::txt_msg::MAX_TXT_TEXT_SIZE);
                                                    let mut pfx = [0u8; 5 + meshcore::payload::txt_msg::MAX_TXT_TEXT_SIZE];
                                                    pfx[0..4].copy_from_slice(&timestamp.to_le_bytes());
                                                    pfx[4] = flags;
                                                    pfx[5..5 + text_len].copy_from_slice(&text[..text_len]);
                                                    let expected_ack = meshcore::payload::txt_msg::compute_ack_hash(
                                                        &pfx[..5 + text_len],
                                                        &ctx.pub_key,
                                                    );
                                                    let est_timeout_ms = if is_flood { 30_000u32 } else { 15_000u32 };
                                                    defmt::info!(
                                                        "companion: SEND_TXT_MSG to={=[u8]:02x} → queued, ack={=u32:#010x} flood={=bool}",
                                                        pub_key_prefix, expected_ack, is_flood
                                                    );
                                                    companion::Response::SentWithTag {
                                                        is_flood,
                                                        tag: expected_ack,
                                                        est_timeout_ms,
                                                    }
                                                }
                                                Err(_) => {
                                                    defmt::warn!("companion: SEND_TXT_MSG TX queue full");
                                                    companion::Response::Error(companion::ErrorCode::InsufficientStorage)
                                                }
                                            }
                                        }
                                    }
                                }

                                Ok(companion::cmd::Command::SendChannelMessage { channel, timestamp, text }) => {
                                    let mut v: heapless::Vec<u8, { msg_queue::MAX_TEXT }> = heapless::Vec::new();
                                    let _ = v.extend_from_slice(&text[..text.len().min(msg_queue::MAX_TEXT)]);
                                    match crate::TX_MSG_CHANNEL.try_send(crate::TxChannelMsg {
                                        channel_idx: channel,
                                        timestamp,
                                        text: v,
                                    }) {
                                        Ok(()) => {
                                            defmt::info!("companion: SEND_CHANNEL_MSG ch={=u8} → queued for TX", channel);
                                            companion::Response::Ok
                                        }
                                        Err(_) => {
                                            defmt::warn!("companion: SEND_CHANNEL_MSG ch={=u8} → TX queue full", channel);
                                            companion::Response::Error(companion::ErrorCode::InsufficientStorage)
                                        }
                                    }
                                }

                                Ok(companion::cmd::Command::SetFloodScope(key)) => {
                                    match key {
                                        Some(k) => defmt::info!("companion: SET_FLOOD_SCOPE key={:02X} → OK", k),
                                        None    => defmt::info!("companion: SET_FLOOD_SCOPE (clear) → OK"),
                                    }
                                    companion::Response::Ok
                                }

                                Ok(companion::cmd::Command::SetAdvertName(name)) => {
                                    let len = name.len().min(settings::MAX_NODE_NAME);
                                    let mut new_name = [0u8; settings::MAX_NODE_NAME];
                                    new_name[..len].copy_from_slice(&name[..len]);
                                    defmt::info!("companion: SET_ADVERT_NAME ({=usize} B) → OK", len);
                                    pending_name = Some((new_name, len));
                                    companion::Response::Ok
                                }

                                Ok(companion::cmd::Command::SetRadioParams { freq_khz, bw_hz, sf, cr, client_repeat }) => {
                                    // Validate ranges per MeshCore reference firmware.
                                    if freq_khz >= 300_000 && freq_khz <= 2_500_000
                                        && bw_hz >= 7_000 && bw_hz <= 500_000
                                        && sf >= 5 && sf <= 12
                                        && cr >= 5 && cr <= 8
                                    {
                                        defmt::info!(
                                            "companion: SET_RADIO_PARAMS freq={=u32}kHz bw={=u32}Hz SF={=u8} CR={=u8} → OK",
                                            freq_khz, bw_hz, sf, cr
                                        );
                                        pending_radio = Some(settings::RadioParams {
                                            freq_hz: freq_khz * 1000,
                                            bw_hz,
                                            sf,
                                            cr,
                                            tx_power: radio_params.tx_power,
                                            client_repeat,
                                        });
                                        companion::Response::Ok
                                    } else {
                                        defmt::warn!("companion: SET_RADIO_PARAMS out of range → ERROR");
                                        companion::Response::Error(companion::ErrorCode::InvalidParameter)
                                    }
                                }

                                Ok(companion::cmd::Command::SetRadioTxPower(power)) => {
                                    if power >= -9 && power <= 22 {
                                        defmt::info!("companion: SET_RADIO_TX_POWER {=i8} dBm → OK", power);
                                        pending_radio = Some(settings::RadioParams { tx_power: power, ..radio_params });
                                        companion::Response::Ok
                                    } else {
                                        defmt::warn!("companion: SET_RADIO_TX_POWER {=i8} dBm out of range → ERROR", power);
                                        companion::Response::Error(companion::ErrorCode::InvalidParameter)
                                    }
                                }

                                Ok(companion::cmd::Command::SetOtherParams { manual_add_contacts, telemetry, advert_loc_policy, multi_acks }) => {
                                    defmt::info!(
                                        "companion: SET_OTHER_PARAMS manual={=u8} tele={=u8} loc={=u8} macks={=u8} → OK",
                                        manual_add_contacts, telemetry, advert_loc_policy, multi_acks
                                    );
                                    pending_other = Some(settings::OtherParams {
                                        manual_add_contacts,
                                        telemetry_mode_base: telemetry & 0x03,
                                        telemetry_mode_loc:  (telemetry >> 2) & 0x03,
                                        telemetry_mode_env:  (telemetry >> 4) & 0x03,
                                        advert_loc_policy,
                                        multi_acks,
                                    });
                                    companion::Response::Ok
                                }

                                Ok(companion::cmd::Command::SetAdvertLatLon { lat, lon }) => {
                                    if lat >= -90_000_000 && lat <= 90_000_000
                                        && lon >= -180_000_000 && lon <= 180_000_000
                                    {
                                        defmt::info!(
                                            "companion: SET_ADVERT_LATLON lat={=i32} lon={=i32} → OK",
                                            lat, lon
                                        );
                                        pending_position = Some(settings::Position { lat, lon });
                                        companion::Response::Ok
                                    } else {
                                        defmt::warn!(
                                            "companion: SET_ADVERT_LATLON lat={=i32} lon={=i32} out of range → ERROR",
                                            lat, lon
                                        );
                                        companion::Response::Error(companion::ErrorCode::InvalidParameter)
                                    }
                                }

                                Ok(companion::cmd::Command::SendTracePath { tag, auth, flags, path }) => {
                                    let mut path_vec: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> =
                                        heapless::Vec::new();
                                    let _ = path_vec.extend_from_slice(
                                        &path[..path.len().min(meshcore::MAX_PATH_SIZE)]
                                    );
                                    // Estimate timeout: 5 seconds per hop + base 3 seconds
                                    let hops = path_vec.len().max(1) as u32;
                                    let est_timeout_ms = 3_000 + hops * 5_000;
                                    defmt::info!(
                                        "companion: SEND_TRACE_PATH tag={=u32:#010x} path_len={=usize} → queued",
                                        tag, path_vec.len(),
                                    );
                                    let _ = crate::TX_TRACE_CHANNEL.try_send(crate::TxTracePath {
                                        tag,
                                        auth,
                                        flags,
                                        path: path_vec,
                                    });
                                    let sent_resp = companion::Response::SentWithTag {
                                        is_flood: false,
                                        tag,
                                        est_timeout_ms,
                                    };
                                    let mut _s = [0u8; 10];
                                    let _sl = companion::encode(&sent_resp, &mut _s);
                                    defmt::info!("companion: RESP_CODE_SENT ({=usize}B): {=[u8]:02x}", _sl, &_s[.._sl]);
                                    sent_resp
                                }

                                Ok(companion::cmd::Command::SendStatusRequest { pub_key }) => {
                                    defmt::info!(
                                        "companion: SEND_STATUS_REQUEST key={=[u8]:02x}",
                                        &pub_key[..6],
                                    );
                                    let _ = crate::TX_STATUS_REQ_CHANNEL.try_send(crate::TxStatusReq {
                                        pub_key: *pub_key,
                                    });
                                    companion::Response::Ok
                                }

                                Ok(companion::cmd::Command::SendLogin { pub_key, password }) => {
                                    let mut pw_vec: heapless::Vec<u8, 15> = heapless::Vec::new();
                                    let _ = pw_vec.extend_from_slice(
                                        &password[..password.len().min(15)]
                                    );
                                    // Generate a tag from the dest pub_key (first 4 bytes LE).
                                    let tag = u32::from_le_bytes([
                                        pub_key[0], pub_key[1], pub_key[2], pub_key[3],
                                    ]);
                                    defmt::info!(
                                        "companion: SEND_LOGIN key={=[u8]:02x} tag={=u32:#010x} → queued",
                                        &pub_key[..6], tag,
                                    );
                                    let _ = crate::TX_LOGIN_CHANNEL.try_send(crate::TxLogin {
                                        pub_key: *pub_key,
                                        password: pw_vec,
                                    });
                                    companion::Response::SentWithTag {
                                        is_flood: false,
                                        tag,
                                        est_timeout_ms: 15_000,
                                    }
                                }

                                Ok(companion::cmd::Command::GetAdvertPath { pub_key }) => {
                                    defmt::info!("companion: GET_ADVERT_PATH key={=[u8]:02x}", &pub_key[..6]);
                                    match contact_by_key {
                                        Some(ref c) => {
                                            let path_byte_len = c.path_actual_bytes();
                                            companion::Response::AdvertPath {
                                                recv_timestamp: c.last_advert_ts,
                                                path_len_byte:  c.out_path_len,
                                                path:           &c.out_path[..path_byte_len],
                                            }
                                        }
                                        None => {
                                            defmt::warn!("companion: GET_ADVERT_PATH key={=[u8]:02x} not found", &pub_key[..6]);
                                            companion::Response::Error(companion::ErrorCode::InvalidParameter)
                                        }
                                    }
                                }

                                Ok(companion::cmd::Command::SetTuningParams { rx_delay_base_x1000, airtime_factor_x1000 }) => {
                                    let af = airtime_factor_x1000.min(9_000);
                                    defmt::info!(
                                        "companion: SET_TUNING_PARAMS rx_delay={=u32} af={=u32}",
                                        rx_delay_base_x1000, af,
                                    );
                                    let params = settings::TuningParams {
                                        rx_delay_base_x1000,
                                        airtime_factor_x1000: af,
                                    };
                                    match settings::set_tuning_params(params).await {
                                        Ok(()) => {
                                            crate::TUNING_CHANGED_SIGNAL.signal(af);
                                            companion::Response::Ok
                                        }
                                        Err(e) => {
                                            defmt::warn!("companion: SET_TUNING_PARAMS persist failed: {:?}", e);
                                            companion::Response::Error(companion::ErrorCode::Generic)
                                        }
                                    }
                                }

                                Ok(companion::cmd::Command::GetTuningParams) => {
                                    let params = settings::get_tuning_params().await;
                                    defmt::info!(
                                        "companion: GET_TUNING_PARAMS rx={=u32} af={=u32}",
                                        params.rx_delay_base_x1000, params.airtime_factor_x1000,
                                    );
                                    companion::Response::TuningParams {
                                        rx_delay_base_x1000:  params.rx_delay_base_x1000,
                                        airtime_factor_x1000: params.airtime_factor_x1000,
                                    }
                                }

                                Ok(companion::cmd::Command::SendTelemetryRequest { pub_key: Some(key) }) => {
                                    let tag = crate::unix_now().unwrap_or(0);
                                    let contact = contacts::ContactStore::new().find_by_key(&key).await;
                                    let is_flood = contact
                                        .as_ref()
                                        .map(|c| c.out_path_len == contacts::OUT_PATH_UNKNOWN)
                                        .unwrap_or(true);
                                    let est_timeout_ms = if is_flood { 30_000u32 } else { 15_000u32 };
                                    defmt::info!(
                                        "companion: SEND_TELEMETRY_REQ key={=[u8]:02x} tag={=u32:#010x} flood={=bool}",
                                        &key[..6], tag, is_flood,
                                    );
                                    let _ = crate::TX_TELEM_REQ_CHANNEL.try_send(crate::TxTelemReq {
                                        pub_key: key,
                                        tag,
                                    });
                                    companion::Response::SentWithTag { is_flood, tag, est_timeout_ms }
                                }

                                Ok(companion::cmd::Command::SendTelemetryRequest { pub_key: None }) => {
                                    // Self-telemetry: CayenneLPP battery voltage + die temperature.
                                    // LPP_VOLTAGE (0x74): [ch=1][0x74][val:2 BE unsigned], 0.01V resolution.
                                    let mv = crate::fw::battery::read_mv() as u32;
                                    let v_val = ((mv + 5) / 10) as u16; // mV → cV
                                    self_telem_lpp[0] = 1;
                                    self_telem_lpp[1] = 0x74;
                                    self_telem_lpp[2] = (v_val >> 8) as u8;
                                    self_telem_lpp[3] = v_val as u8;
                                    self_telem_lpp_len = 4;

                                    // CayenneLPP temperature: [ch=1][0x67][val:2 BE signed], 0.1°C resolution.
                                    let t_c10 = crate::fw::temperature::last_c10();
                                    if t_c10 != i16::MIN {
                                        let t_bytes = t_c10.to_be_bytes();
                                        self_telem_lpp[4] = 1;
                                        self_telem_lpp[5] = 0x67;
                                        self_telem_lpp[6] = t_bytes[0];
                                        self_telem_lpp[7] = t_bytes[1];
                                        self_telem_lpp_len = 8;
                                    }

                                    self_telem_prefix.copy_from_slice(&ctx.pub_key[..6]);
                                    defmt::info!(
                                        "companion: SEND_TELEMETRY_REQ (self) batt={=u16}cV temp={=i16}×0.1°C lpp={=usize}B",
                                        v_val, t_c10, self_telem_lpp_len,
                                    );
                                    companion::Response::TelemetryResponse {
                                        pub_key_prefix: self_telem_prefix,
                                        lpp_data: &self_telem_lpp[..self_telem_lpp_len],
                                    }
                                }

                                Ok(companion::cmd::Command::Unknown(b)) => {
                                    defmt::warn!("companion: unknown command 0x{:02X} → ERROR", b);
                                    companion::Response::Error(companion::ErrorCode::InvalidCommand)
                                }
                            };

                            // Acknowledge the write then queue the response notification.
                            match write.accept() {
                                Ok(reply) => reply.send().await,
                                Err(e) => defmt::warn!("companion: write.accept() failed: {:?}", defmt::Debug2Format(&e)),
                            }
                            enqueue_notify(outbox, &response);

                            // Apply any pending settings changes (after response is sent
                            // so the borrow on node_name via `response` has ended).
                            if let Some((new_name, len)) = pending_name {
                                node_name[..settings::MAX_NODE_NAME].copy_from_slice(&new_name);
                                node_name_len = len;
                                match settings::set_node_name(&node_name[..len]).await {
                                    Ok(()) => defmt::info!("companion: node_name persisted"),
                                    Err(e) => defmt::warn!("companion: node_name persist failed: {:?}", e),
                                }
                                crate::update_node_name(&node_name[..node_name_len]);
                            }
                            if let Some(new_radio) = pending_radio {
                                radio_params = new_radio;
                                match settings::set_radio_params(radio_params).await {
                                    Ok(()) => defmt::info!("companion: radio params persisted (takes effect on reboot)"),
                                    Err(e) => defmt::warn!("companion: radio params persist failed: {:?}", e),
                                }
                            }
                            if let Some(new_pos) = pending_position {
                                position = new_pos;
                                match settings::set_position(position).await {
                                    Ok(()) => defmt::info!("companion: position persisted"),
                                    Err(e) => defmt::warn!("companion: position persist failed: {:?}", e),
                                }
                            }
                            if let Some(new_other) = pending_other {
                                other_params = new_other;
                                match settings::set_other_params(other_params).await {
                                    Ok(()) => defmt::info!("companion: other params persisted"),
                                    Err(e) => defmt::warn!("companion: other params persist failed: {:?}", e),
                                }
                            }
                            if pending_reboot {
                                // Give BLE stack time to deliver the PACKET_OK notification
                                // before pulling the reset line.
                                embassy_time::Timer::after_millis(200).await;
                                cortex_m::peripheral::SCB::sys_reset();
                            }

                            // Persist an ADD_UPDATE_CONTACT payload to flash and push
                            // ContactStart/Contact/ContactEnd back into the outbox.
                            if let Some(ref contact) = pending_contact {
                                let store = contacts::ContactStore::new();
                                match store.add_or_update(contact).await {
                                    Ok(r) => {
                                        defmt::info!("companion: ADD_UPDATE_CONTACT → {:?}", r);
                                        let mut prefix = [0u8; 6];
                                        prefix.copy_from_slice(&contact.pub_key[..6]);
                                        let name_end = contact.name.iter().position(|&b| b == 0).unwrap_or(32);
                                        enqueue_notify(outbox, &companion::Response::ContactStart);
                                        enqueue_notify(outbox, &companion::Response::Contact(companion::response::Contact {
                                            pub_key_prefix: prefix,
                                            flags:     contact.flags,
                                            last_seen: contact.last_advert_ts,
                                            name:      &contact.name[..name_end],
                                        }));
                                        enqueue_notify(outbox, &companion::Response::ContactEnd);
                                    }
                                    Err(e) => defmt::warn!("companion: ADD_UPDATE_CONTACT store failed: {:?}", e),
                                }
                            }
                        } else if let Ok(reply) = write.accept() {
                            reply.send().await;
                        }
                    }
                    _ => {}
                }

                // -----------------------------------------------------------
                // New messages arrived while connected — push 0x83 to app.
                // -----------------------------------------------------------
                Either6::Second(()) => {
                    defmt::debug!("BLE: {} message(s) waiting, sending 0x83", msg_queue::count());
                    enqueue_notify(outbox, &companion::Response::MessagesWaiting);
                }

                // -----------------------------------------------------------
                // Raw LoRa packet received — push 0x88 to app.
                // -----------------------------------------------------------
                Either6::Third(pkt) => {
                    defmt::debug!("BLE: raw LoRa pkt {} bytes, pushing 0x88", pkt.len);
                    enqueue_notify(outbox, &companion::Response::LogRxData {
                        snr_x4: pkt.snr_x4,
                        rssi:   pkt.rssi,
                        data:   &pkt.data[..pkt.len],
                    });
                }

                // -----------------------------------------------------------
                // Advert received — push 0x8A (NewAdvert) to app.
                // -----------------------------------------------------------
                Either6::Fourth(notif) => {
                    defmt::debug!("BLE: advert from {:02x}, pushing 0x8A", &notif.pub_key[..6]);
                    let out_path = [0u8; 64];
                    enqueue_notify(outbox, &companion::Response::NewAdvert(companion::response::NewAdvert {
                        pub_key:               &notif.pub_key,
                        adv_type:              notif.adv_type,
                        flags:                 0,
                        out_path_len:          0xFF,
                        out_path:              &out_path,
                        name:                  &notif.name,
                        last_advert_timestamp: notif.timestamp,
                        gps_lat:               notif.lat,
                        gps_lon:               notif.lon,
                        lastmod:               0,
                    }));
                }

                // -----------------------------------------------------------
                // Trace-path result — push 0x89 (PUSH_CODE_TRACE_DATA) to app.
                // -----------------------------------------------------------
                Either6::Fifth(trace) => {
                    let mut _dbg_buf = [0u8; companion::MAX_RESPONSE_LEN];
                    let _dbg_len = companion::encode(&companion::Response::TraceData {
                        path_len:    trace.path_len,
                        flags:       trace.flags,
                        tag:         trace.tag,
                        auth_code:   trace.auth_code,
                        path_hashes: &trace.path_hashes,
                        path_snrs:   &trace.path_snrs,
                        final_snr:   trace.final_snr,
                    }, &mut _dbg_buf);
                    defmt::info!(
                        "BLE: trace result tag={=u32:#010x} path_len={=u8}, pushing 0x89 ({=usize}B): {=[u8]:02x}",
                        trace.tag, trace.path_len, _dbg_len, &_dbg_buf[.._dbg_len],
                    );
                    enqueue_notify(outbox, &companion::Response::TraceData {
                        path_len:    trace.path_len,
                        flags:       trace.flags,
                        tag:         trace.tag,
                        auth_code:   trace.auth_code,
                        path_hashes: &trace.path_hashes,
                        path_snrs:   &trace.path_snrs,
                        final_snr:   trace.final_snr,
                    });
                }

                // -----------------------------------------------------------
                // Login result — push 0x85 (success) or 0x86 (fail) to app.
                // Status result — push 0x87 to app.
                // -----------------------------------------------------------
                Either6::Sixth(either) => match either {
                    Either::First(login) => {
                        let mut prefix = [0u8; 6];
                        prefix.copy_from_slice(&login.pub_key[..6]);
                        if login.success {
                            defmt::info!("BLE: login OK from {:02x}, pushing 0x85", &login.pub_key[..6]);
                            enqueue_notify(outbox, &companion::Response::LoginSuccess {
                                is_admin:       login.is_admin,
                                pub_key_prefix: prefix,
                                tag:            login.tag,
                                acl_perms:      login.acl_perms,
                                fw_ver_level:   login.fw_ver_level,
                            });
                        } else {
                            defmt::info!("BLE: login FAIL from {:02x}, pushing 0x86", &login.pub_key[..6]);
                            enqueue_notify(outbox, &companion::Response::LoginFail { pub_key_prefix: prefix });
                        }
                    }
                    Either::Second(either2) => match either2 {
                        Either::First(status) => {
                            let mut prefix = [0u8; 6];
                            prefix.copy_from_slice(&status.pub_key[..6]);
                            defmt::info!(
                                "BLE: status from {:02x} uptime={=u32}s batt={=u16}mV, pushing 0x87",
                                &status.pub_key[..6], status.uptime_secs, status.battery_mv,
                            );
                            enqueue_notify(outbox, &companion::Response::StatusResponse {
                                pub_key_prefix: prefix,
                                uptime_secs:    status.uptime_secs,
                                battery_mv:     status.battery_mv,
                            });
                        }
                        Either::Second(either3) => match either3 {
                            Either::First(ack) => {
                                defmt::info!(
                                    "BLE: ACK ack_crc={=u32:#010x} trip={=u32}ms, pushing 0x82",
                                    ack.ack_crc, ack.trip_time_ms,
                                );
                                enqueue_notify(outbox, &companion::Response::Ack(companion::Ack {
                                    ack_crc:      ack.ack_crc,
                                    trip_time_ms: ack.trip_time_ms,
                                }));
                            }
                            Either::Second(telem) => {
                                let mut prefix = [0u8; 6];
                                prefix.copy_from_slice(&telem.pub_key[..6]);
                                defmt::info!(
                                    "BLE: telem from {:02x} lpp={=usize}B, pushing 0x8B",
                                    &telem.pub_key[..6], telem.lpp.len(),
                                );
                                enqueue_notify(outbox, &companion::Response::TelemetryResponse {
                                    pub_key_prefix: prefix,
                                    lpp_data: &telem.lpp,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Give the HCI runner time to fully process the disconnection before
        // the outer loop tries to start advertising again.  Without this
        // delay the advertiser immediately gets "Connection Rejected due to
        // Limited Resources" because the controller slot isn't freed yet.
        embassy_time::Timer::after_millis(200).await;
    }
}
