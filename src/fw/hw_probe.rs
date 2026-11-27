// SX1262 LoRa presence probe shared by the in-firmware factory test
// (`fw/factory_test.rs`) and the standalone `bin/hwtest.rs` (which cannot link
// the lib, so both `include!` this file).  Keep it a regular `//` comment and
// free of `use`s: every path below is fully qualified.

/// Reset the SX1262 and read its status over SPI.  Returns true if the chip
/// releases BUSY within 50 ms and `GetStatus` returns a plausible status byte.
///
/// `make_spi` builds the `Spim` once BUSY has fallen.  It is a closure because
/// the SPI2 interrupt binding differs between the two callers (the lib reuses
/// `mesh::sx1262::Irqs`, hwtest binds its own); it receives the 1 MHz config
/// both callers use.  The RF switch, reset and chip-select pins are driven from
/// here and released when the function returns.
#[allow(dead_code)]
async fn sx1262_status_ok<'d>(
    rf_sw: embassy_nrf::Peri<'d, embassy_nrf::gpio::AnyPin>,
    rst: embassy_nrf::Peri<'d, embassy_nrf::gpio::AnyPin>,
    nss: embassy_nrf::Peri<'d, embassy_nrf::gpio::AnyPin>,
    busy: embassy_nrf::Peri<'d, embassy_nrf::gpio::AnyPin>,
    make_spi: impl FnOnce(embassy_nrf::spim::Config) -> embassy_nrf::spim::Spim<'d>,
) -> bool {
    use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};

    // Hold RF switch in a defined state.
    let _rf_sw = Output::new(rf_sw, Level::Low, OutputDrive::Standard);
    let mut rst_out = Output::new(rst, Level::Low, OutputDrive::Standard);
    let mut nss_out = Output::new(nss, Level::High, OutputDrive::Standard);
    let busy_in = Input::new(busy, Pull::None);

    // Reset pulse: hold low >= 100 us then release.
    embassy_time::Timer::after_micros(500).await;
    rst_out.set_high();

    // Wait for BUSY to fall (chip ready), 50 ms timeout.
    let mut ready = false;
    for _ in 0..50u8 {
        if !busy_in.is_high() {
            ready = true;
            break;
        }
        embassy_time::Timer::after_millis(1).await;
    }
    if !ready {
        defmt::warn!("hwtest: LoRa BUSY stuck high after reset");
        return false;
    }

    let mut spi_cfg = embassy_nrf::spim::Config::default();
    spi_cfg.frequency = embassy_nrf::spim::Frequency::M1;
    let mut spi = make_spi(spi_cfg);

    // GetStatus (0xC0) + NOP byte -> status returned on MISO in the second byte.
    let tx = [0xC0u8, 0x00];
    let mut rx = [0u8; 2];
    nss_out.set_low();
    let r = spi.transfer(&mut rx, &tx).await;
    nss_out.set_high();

    match r {
        Ok(()) => {
            let status = rx[1];
            defmt::info!("hwtest: LoRa GetStatus = 0x{:02X}", status);
            // Chip mode bits [6:4]: 0 = unused, 7 = reserved.  Valid modes are 2..=6.
            let mode = (status >> 4) & 0x07;
            mode != 0 && mode != 7 && status != 0xFF
        }
        Err(_) => {
            defmt::warn!("hwtest: LoRa SPI transfer failed");
            false
        }
    }
}
