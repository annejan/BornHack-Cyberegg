use core::convert::Infallible;
use core::fmt::Debug;

use defmt_rtt as _;
use panic_probe as _;

use embassy_nrf::{
    Peri, bind_interrupts,
    gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull},
    peripherals,
    spim::{self, Frequency, InterruptHandler, Spim},
};
use embassy_time::Delay;
use embassy_time::Timer;
use embedded_hal_bus::spi::ExclusiveDevice;

use super::health::SYSTEM_HEALTH;
use embassy_futures::select::{Either, select};
use sx126x::SX126x;
use sx126x::conf::Config as LoRaConfig;
use sx126x::op::PacketType::LoRa;
use sx126x::op::irq::IrqMaskBit::{
    CrcErr, HeaderError, HeaderValid, PreambleDetected, RxDone, SyncwordValid, Timeout, TxDone,
};
use sx126x::op::rxtx::DeviceSel::SX1262;
use sx126x::op::status::ChipMode;
use sx126x::op::tcxo::{TcxoDelay, TcxoVoltage};
use sx126x::op::*;

use crate::{health_err, update_health};
use meshcore::channel;

// ---------------------------------------------------------------------------
// MeshCore LoRa configuration
// ---------------------------------------------------------------------------

/// Configurable LoRa radio settings for MeshCore.
pub struct MeshCoreConfig {
    pub frequency_hz: u32,
    pub spread_factor: LoRaSpreadFactor,
    pub bandwidth: LoRaBandWidth,
    pub coding_rate: LoraCodingRate,
    /// SX1262 sync word (written to registers 0x0740/0x0741).
    /// MeshCore uses 0x1424 for private networks.
    pub sync_word: u16,
    pub tx_power_dbm: i8,
    pub preamble_len: u16,
    /// TCXO voltage and startup delay for modules that power the TCXO via DIO3
    /// (e.g. eByte E22). Set to None if the module uses a plain crystal instead.
    pub tcxo: Option<(TcxoVoltage, TcxoDelay)>,
}

impl MeshCoreConfig {
    /// MeshCore EU/UK narrow band preset — matches the meshcore-dev/MeshCore firmware defaults.
    ///
    /// 869.618 MHz · BW 62.5 kHz · SF8 · CR 4/5 · sync word 0x1424 (private LoRa)
    ///
    /// TCXO: 1.8 V / 5 ms startup — typical for eByte E22-900M22S and similar modules.
    /// If the board uses a plain crystal, set `tcxo: None`.
    ///
    /// See <https://www.m7spi.co.uk/switching-to-uk-narrow-band-a-guide-for-meshcore-users/>
    pub const UK_NARROW_BAND: Self = Self {
        frequency_hz: 869_618_000,
        spread_factor: LoRaSpreadFactor::SF8,
        bandwidth: LoRaBandWidth::BW62,
        coding_rate: LoraCodingRate::CR4_5,
        sync_word: 0x1424, // RADIOLIB_SX126X_SYNC_WORD_PRIVATE
        tx_power_dbm: 14,
        preamble_len: 8,
        tcxo: None, // External 32 MHz crystal on XTA/XTB — no DIO3 TCXO control needed
    };
}

// ---------------------------------------------------------------------------

const F_XTAL: u32 = 32_000_000; // 32 MHz crystal

bind_interrupts!(struct Irqs {
    SPI2 => InterruptHandler<peripherals::SPI2>;
});

#[derive(Debug, defmt::Format)]
pub enum LoraError {
    Spi(&'static str),
    Timeout,
    Buffer(&'static str),
}

pub struct SimpleLoRa<'a> {
    lora: SX126x<
        ExclusiveDevice<Spim<'a>, Output<'a>, Delay>,
        Output<'a>,
        Input<'a>,
        Output<'a>,
        AlwaysHigh,
    >,
    tx_timeout: RxTxTimeout,
    crc_type: LoRaCrcType,
    preamble_len: u16,
    dio1: Input<'a>,
}

impl<'a> SimpleLoRa<'a> {
    pub fn new(
        spi: Peri<'a, peripherals::SPI2>,
        sck_pin: Peri<'a, AnyPin>,
        mosi_pin: Peri<'a, AnyPin>,
        miso_pin: Peri<'a, AnyPin>,
        nrst_pin: Peri<'a, AnyPin>,
        nss_pin: Peri<'a, AnyPin>,
        busy_pin: Peri<'a, AnyPin>,
        dio1_pin: Peri<'a, AnyPin>,
        ant_pin: Peri<'a, AnyPin>,
        config: &MeshCoreConfig,
    ) -> Result<SimpleLoRa<'a>, LoraError> {
        let mut spi_cfg = spim::Config::default();
        spi_cfg.frequency = Frequency::M1;
        let spim = Spim::new(spi, Irqs, sck_pin, mosi_pin, miso_pin, spi_cfg);

        let nss = Output::new(nss_pin, Level::High, OutputDrive::Standard);
        let nreset = Output::new(nrst_pin, Level::High, OutputDrive::Standard);
        let busy = Input::new(busy_pin, Pull::None);
        let ant = Output::new(ant_pin, Level::High, OutputDrive::Standard);
        let dio1 = Input::new(dio1_pin, Pull::None);

        // AlwaysHigh is a dummy DIO1 for the sx126x struct; real async DIO1 waiting
        // is done externally via wait_for_rising_edge() so the executor is not blocked.
        let spi_dev = ExclusiveDevice::new(spim, nss, Delay).unwrap();

        let conf = build_lora_config(config);
        let mut lora = SX126x::new(spi_dev, (nreset, busy, ant, AlwaysHigh));
        lora.init(conf)
            .map_err(|_| LoraError::Spi("lora init failed"))?;

        lora.set_rx(RxTxTimeout::continuous_rx())
            .map_err(|_| LoraError::Spi("lora set_rx failed"))?;
        lora.set_ant_enabled(true).unwrap();

        Ok(SimpleLoRa {
            lora,
            tx_timeout: 0.into(),
            crc_type: LoRaCrcType::CrcOn,
            preamble_len: config.preamble_len,
            dio1,
        })
    }

    /// Wait for the chip to enter RX mode (0x05), polling every 50 ms for up to 500 ms.
    /// Returns true if RX mode is confirmed.
    pub async fn ensure_rx(&mut self) -> bool {
        // wait_on_busy before set_rx: the crate's set_rx() skips the mandatory busy
        // check, so sending the command while BUSY is asserted would silently drop it.
        self.lora.wait_on_busy().ok();
        self.lora.set_rx(RxTxTimeout::continuous_rx()).ok();
        self.lora.set_ant_enabled(true).ok();

        for i in 0..10u8 {
            Timer::after_millis(50).await;
            if let Ok(s) = self.lora.get_status() {
                let mode = s.chip_mode().map(|m| m as u8).unwrap_or(0xFF);
                let cmd = s.command_status().map(|c| c as u8).unwrap_or(0xFF);
                defmt::info!(
                    "ensure_rx poll {=u8}: chip_mode={=u8:#04x} cmd={=u8:#04x}",
                    i,
                    mode,
                    cmd
                );
                if matches!(s.chip_mode(), Some(ChipMode::RX)) {
                    return true;
                }
            }
        }
        false
    }

    /// Wait for the next LoRa event (RxDone or Timeout), read the payload if
    /// present, and re-arm RX.
    ///
    /// Returns `Ok(Some((len, rssi_dbm)))` on a valid receive,
    /// `Ok(None)` on timeout or CRC error (RX is re-armed in both cases).
    pub async fn receive_packet(
        &mut self,
        buf: &mut [u8],
    ) -> Result<Option<(usize, i16)>, LoraError> {
        // Log chip mode so we can confirm the chip is actually in RX before waiting.
        // Expected: 0x05 (RX). If 0x02 (StbyRC), set_rx() hasn't taken effect yet.
        if let Ok(s) = self.lora.get_status() {
            let mode = s.chip_mode().map(|m| m as u8).unwrap_or(0xFF);
            defmt::info!("RX wait: chip_mode={=u8:#04x} (0x05=RX, 0x02=StbyRC)", mode);
        }

        // Clear any stale IRQ so DIO1 is deasserted before we arm the rising-edge wait.
        // Without this, a leftover HIGH from the init sequence would block wait_for_rising_edge()
        // forever (it only fires on LOW→HIGH transitions).
        self.lora
            .clear_irq_status(IrqMask::all())
            .map_err(|_| LoraError::Spi("clear_irq before wait failed"))?;

        // TODO: Replace active wait + watchdog timer with deep sleep + GPIO wake-up once
        // reception is confirmed working. The current approach keeps HFCLK running and
        // prevents low-power sleep, significantly increasing battery drain.
        match select(self.dio1.wait_for_rising_edge(), Timer::after_secs(15)).await {
            Either::First(_) => {} // DIO1 fired — read IRQ below
            Either::Second(_) => {
                defmt::info!("LoRa: no DIO1 in 15s — re-arming RX (continuous mode)");
                self.lora.wait_on_busy().ok();
                self.lora
                    .set_rx(RxTxTimeout::continuous_rx())
                    .map_err(|_| LoraError::Timeout)?;
                self.lora.set_ant_enabled(true).unwrap();
                return Ok(None);
            }
        }

        // sx126x 0.3.0 does not call wait_on_busy() before get_irq_status();
        // without this the chip may still be busy processing the just-received
        // packet and the SPI read returns 0x00 (all flags false).
        self.lora.wait_on_busy().ok();
        let irq = self
            .lora
            .get_irq_status()
            .map_err(|_| LoraError::Spi("get_irq_status failed"))?;
        self.lora.wait_on_busy().ok();
        self.lora.clear_irq_status(IrqMask::all()).unwrap();

        // Log every IRQ event so we can diagnose reception issues:
        //   timeout only           → wrong frequency
        //   preamble, no sync_word  → frequency OK, sync word mismatch
        //   sync_word, header_err   → SF/BW/CR mismatch
        //   rx_done + crc_err      → modem settings OK, payload error
        //   rx_done, no crc_err    → full receive
        defmt::info!(
            "DIO1: rx={} crc_err={} timeout={} | preamble={} syncword={} header_ok={} header_err={}",
            irq.rx_done(),
            irq.crc_err(),
            irq.timeout(),
            irq.preamble_detected(),
            irq.syncword_valid(),
            irq.header_valid(),
            irq.header_error(),
        );

        let result = if irq.rx_done() && !irq.crc_err() {
            let buf_status = self
                .lora
                .get_rx_buffer_status()
                .map_err(|_| LoraError::Buffer("get_rx_buffer_status failed"))?;
            let len = buf_status.payload_length_rx() as usize;
            let offset = buf_status.rx_start_buffer_pointer();

            if len > buf.len() {
                return Err(LoraError::Buffer("buffer too small"));
            }

            self.lora
                .read_buffer(offset, &mut buf[..len])
                .map_err(|_| LoraError::Buffer("read_buffer failed"))?;

            let rssi = self
                .lora
                .get_packet_status()
                .map(|s| s.rssi_pkt() as i16)
                .unwrap_or(0);

            Some((len, rssi))
        } else if irq.crc_err() {
            let rssi = self
                .lora
                .get_packet_status()
                .map(|s| s.rssi_pkt() as i16)
                .unwrap_or(0);
            defmt::warn!(
                "LoRa CRC error {=i16}dBm — header decoded OK (SF/BW/CR/syncword match) but payload bytes corrupted (collision, interference, or sender has CRC disabled)",
                rssi
            );
            None
        } else {
            None
        };

        // Re-arm continuous RX
        defmt::debug!("RX re-armed");
        self.lora.wait_on_busy().ok();
        self.lora
            .set_rx(RxTxTimeout::continuous_rx())
            .map_err(|_| LoraError::Timeout)?;
        self.lora.set_ant_enabled(true).unwrap();

        Ok(result)
    }

    pub async fn send_message(&mut self, message: &str) -> Result<(), LoraError> {
        self.lora.set_ant_enabled(true).unwrap();

        self.lora.write_buffer(0x00, message.as_bytes()).unwrap();
        let packet_params = LoRaPacketParams::default()
            .set_preamble_len(self.preamble_len)
            .set_payload_len(message.len() as u8)
            .set_crc_type(self.crc_type)
            .into();
        self.lora.set_packet_params(packet_params).unwrap();
        self.lora
            .set_tx(self.tx_timeout)
            .map_err(|_| LoraError::Timeout)?;

        self.dio1.wait_for_rising_edge().await;
        self.lora.clear_irq_status(IrqMask::all()).unwrap();
        self.lora.set_ant_enabled(false).unwrap();

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MeshCore listener task
// ---------------------------------------------------------------------------

/// Listen for MeshCore packets and log `public` channel messages to defmt.
///
/// Configures the SX1262 using [`MeshCoreConfig::UK_NARROW_BAND`] and enters
/// a continuous receive loop.  Every received packet is parsed with the
/// `meshcore` vendor crate.  Group-text messages (`GrpTxt`) are logged with
/// their channel hash and text; node advertisements are logged with device
/// name and role; all other types are logged as raw hex.
///
/// **Channel key**: MeshCore encrypts group text with AES-128-ECB.  Set
/// `PUBLIC_CHANNEL_KEY` to the correct 16-byte key for your network.
/// The MeshCore app encodes the key in base64 under *Settings → Channels*.
/// Until the key is configured, the raw ciphertext bytes are logged instead.
pub async fn run_meshcore_listener<'a>(
    spi: Peri<'a, peripherals::SPI2>,
    sck_pin: Peri<'a, AnyPin>,
    mosi_pin: Peri<'a, AnyPin>,
    miso_pin: Peri<'a, AnyPin>,
    nrst_pin: Peri<'a, AnyPin>,
    nss_pin: Peri<'a, AnyPin>,
    busy_pin: Peri<'a, AnyPin>,
    dio1_pin: Peri<'a, AnyPin>,
    ant_pin: Peri<'a, AnyPin>,
) -> ! {
    update_health!(|h| h.lora.set_ok("Ok when started."));

    // Adjust MeshCoreConfig::UK_NARROW_BAND or replace with your own config.
    let config = &MeshCoreConfig::UK_NARROW_BAND;

    let mut lora = match SimpleLoRa::new(
        spi, sck_pin, mosi_pin, miso_pin, nrst_pin, nss_pin, busy_pin, dio1_pin, ant_pin, config,
    ) {
        Ok(l) => {
            // Hardware test: SX1262 responded correctly to init sequence.
            SYSTEM_HEALTH.lock(|cell| {
                cell.borrow_mut().lora.set_ok("SX1262 init OK");
            });
            l
        }
        Err(e) => {
            health_err!(lora, "LoRa init failed");
            defmt::error!("LoRa init failed: {:?}", e);
            loop {
                Timer::after_millis(60_000).await;
            }
        }
    };

    // Poll chip_mode every 50ms for up to 500ms to confirm the chip entered RX.
    // set_rx() in sx126x 0.3.0 has no wait_on_busy(), so the command may be
    // dropped if the chip is still busy after init. ensure_rx() re-issues it.
    if !lora.ensure_rx().await {
        defmt::error!(
            "SX1262 failed to enter RX mode after 500ms — check crystal/wiring"
        );
    }

    defmt::info!(
        "MeshCore listener ready — freq={=u32}Hz BW=62.5kHz SF=8 CR=4/5 sync={=u16:#06x} preamble={=u16}",
        config.frequency_hz,
        config.sync_word,
        config.preamble_len,
    );

    // Build the channel list from names — no keys are hardcoded here.
    // Add or remove entries to match the channels you want to monitor.
    let channels = [
        KnownChannel::from_public(),
        KnownChannel::from_hashtag("#test"),
        KnownChannel::from_hashtag("#prut"),
        KnownChannel::from_hashtag("#gezellig"),
        KnownChannel::from_hashtag("#leiden"),
    ];

    let mut raw = [0u8; 255];

    loop {
        match lora.receive_packet(&mut raw).await {
            Ok(None) => { /* timeout or CRC error — already re-armed */ }

            Ok(Some((len, rssi))) => {
                let frame = &raw[..len];

                match meshcore::packet::deserialize(frame) {
                    Err(_) => {
                        defmt::info!(
                            "MeshCore [raw {=usize}B {=i16}dBm]: {=[u8]}",
                            len,
                            rssi,
                            frame
                        );
                    }

                    Ok(msg) => {
                        update_health!(|h| h.lora.set_ok("Packet received."));
                        use meshcore::packet::PayloadType;
                        match msg.payload_type {
                            PayloadType::GrpTxt  => log_grp_txt(&msg.payload, rssi, &channels),
                            PayloadType::TxtMsg  => log_grp_txt(&msg.payload, rssi, &channels),
                            PayloadType::Advert  => log_advert(&msg.payload, rssi),
                            PayloadType::Ack     => defmt::info!("MeshCore Ack [{=i16}dBm]", rssi),
                            other => {
                                defmt::info!(
                                    "MeshCore type={=u8} [{=usize}B {=i16}dBm]: {=[u8]:x}",
                                    other.to_u8(),
                                    len,
                                    rssi,
                                    frame
                                );
                            }
                        }
                    }
                }
            }

            Err(e) => {
                defmt::error!("LoRa RX error: {:?}", e);
                health_err!(lora, "LoRa RX error");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-type log helpers
// ---------------------------------------------------------------------------

/// A MeshCore channel with its derived key and hash.
struct KnownChannel {
    /// Human-readable label used in log output (e.g. "public" or "#test").
    name: &'static str,
    key:  [u8; meshcore::CIPHER_KEY_SIZE],
    /// SHA-256(key)[0] — matches the channel_hash byte in GrpTxt packets.
    hash: u8,
}

impl KnownChannel {
    /// The default public channel (spec-defined fixed PSK).
    fn from_public() -> Self {
        let key = channel::PUBLIC_CHANNEL_KEY;
        Self { name: "public", key, hash: channel::hash_from_key(&key) }
    }

    /// A hashtag channel derived from its name (e.g. `"#test"`).
    fn from_hashtag(name: &'static str) -> Self {
        let key = channel::key_from_hashtag(name);
        Self { name, key, hash: channel::hash_from_key(&key) }
    }
}

fn log_grp_txt(payload: &[u8], rssi: i16, channels: &[KnownChannel]) {
    use meshcore::payload::grp_txt;

    let grp = match grp_txt::deserialize(payload) {
        Ok(g) => g,
        Err(_) => {
            defmt::warn!("GrpTxt: failed to parse payload");
            return;
        }
    };

    // Find the first channel whose hash matches the packet.
    let ch = match channels.iter().find(|c| c.hash == grp.channel_hash) {
        Some(c) => c,
        None => {
            defmt::info!(
                "MeshCore GrpTxt [channel={=u8} {=i16}dBm] (unknown channel): {=[u8]}",
                grp.channel_hash,
                rssi,
                &grp.data[..]
            );
            return;
        }
    };

    if grp_txt::verify_mac(&ch.key, &grp).is_err() {
        defmt::warn!(
            "MeshCore GrpTxt [channel={=u8}] MAC mismatch on channel {=str}",
            grp.channel_hash,
            ch.name
        );
        return;
    }

    match grp_txt::decrypt(&ch.key, &grp) {
        Ok(dec) => {
            let text = core::str::from_utf8(&dec.text).unwrap_or("<invalid utf-8>");
            defmt::info!(
                "MeshCore GrpTxt [{=str} ts={=u32} {=i16}dBm]: {=str}",
                ch.name,
                dec.timestamp,
                rssi,
                text
            );
        }
        Err(_) => {
            defmt::warn!("GrpTxt: decryption failed on channel {=str}", ch.name);
        }
    }
}

fn log_advert(payload: &[u8], rssi: i16) {
    use meshcore::payload::advert;

    match advert::deserialize(payload) {
        Ok(a) => {
            if let Some(ref name) = a.name {
                defmt::info!(
                    "MeshCore advert [{=i16}dBm] role={=u8} name={=[u8]}",
                    rssi,
                    a.role.to_u8(),
                    &name[..]
                );
            } else {
                defmt::info!(
                    "MeshCore advert [{=i16}dBm] role={=u8} key={=[u8]}",
                    rssi,
                    a.role.to_u8(),
                    &a.pub_key[..8]
                );
            }
        }
        Err(_) => {
            defmt::warn!("Advert: failed to parse payload");
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn build_lora_config(config: &MeshCoreConfig) -> LoRaConfig {
    let mod_params = LoraModParams::default()
        .set_spread_factor(config.spread_factor)
        .set_bandwidth(config.bandwidth)
        .set_coding_rate(config.coding_rate)
        .into();

    let tx_params = TxParams::default()
        .set_power_dbm(config.tx_power_dbm)
        .set_ramp_time(RampTime::Ramp200u);

    let pa_config = PaConfig::default()
        .set_device_sel(SX1262)
        .set_pa_duty_cycle(0x04);

    let dio1_irq_mask = IrqMask::none()
        .combine(TxDone)
        .combine(RxDone)
        .combine(CrcErr)
        .combine(Timeout)
        .combine(PreambleDetected)
        .combine(SyncwordValid)
        .combine(HeaderValid)
        .combine(HeaderError);

    let packet_params = LoRaPacketParams::default()
        .set_preamble_len(config.preamble_len)
        .into();

    let rf_freq = sx126x::calc_rf_freq(config.frequency_hz as f32, F_XTAL as f32);

    LoRaConfig {
        packet_type: LoRa,
        sync_word: config.sync_word,
        calib_param: CalibParam::from(0x7F),
        mod_params,
        tx_params,
        pa_config,
        packet_params: Some(packet_params),
        dio1_irq_mask,
        dio2_irq_mask: IrqMask::none(),
        dio3_irq_mask: IrqMask::none(),
        rf_frequency: config.frequency_hz,
        rf_freq,
        tcxo_opts: config.tcxo,
    }
}

/// Dummy DIO1 pin passed to SX126x.  Reports "always high" so that the
/// library's internal wait_on_dio1 spin-loop exits immediately.
/// Real interrupt waiting is done externally with `wait_for_rising_edge()`.
struct AlwaysHigh;

impl embedded_hal::digital::ErrorType for AlwaysHigh {
    type Error = Infallible;
}

impl embedded_hal::digital::InputPin for AlwaysHigh {
    fn is_high(&mut self) -> Result<bool, Infallible> {
        Ok(true)
    }
    fn is_low(&mut self) -> Result<bool, Infallible> {
        Ok(false)
    }
}
