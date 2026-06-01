//! USB Mass Storage Class — Bulk-Only Transport (BOT) with minimal SCSI.
//!
//! Ported from the app firmware (`src/fw/usb_msc.rs`).  Self-contained
//! (depends only on `embassy-usb` + `defmt`), so the bootloader can expose the
//! same FAT12 volume over USB *concurrently with DFU* (DFU = ep0/control, MSC =
//! bulk endpoints → no endpoint clash in one composite device).
//!
//! Minimal SCSI set: INQUIRY, TEST UNIT READY, REQUEST SENSE,
//! READ CAPACITY(10), READ(10), WRITE(10), MODE SENSE(6),
//! PREVENT ALLOW MEDIUM REMOVAL.

use embassy_usb::Builder;
use embassy_usb::driver::{Driver, Endpoint, EndpointIn, EndpointOut};

const USB_CLASS_MSC: u8 = 0x08;
const MSC_SUBCLASS_SCSI: u8 = 0x06; // SCSI transparent command set
const MSC_PROTOCOL_BBB: u8 = 0x50; // Bulk-Only Transport

const CBW_SIGNATURE: u32 = 0x4342_5355; // "USBC"
const CSW_SIGNATURE: u32 = 0x5342_5355; // "USBS"
const CBW_SIZE: usize = 31;
const CSW_SIZE: usize = 13;

const CSW_STATUS_PASSED: u8 = 0x00;
const CSW_STATUS_FAILED: u8 = 0x01;

const SCSI_TEST_UNIT_READY: u8 = 0x00;
const SCSI_REQUEST_SENSE: u8 = 0x03;
const SCSI_INQUIRY: u8 = 0x12;
const SCSI_MODE_SENSE_6: u8 = 0x1A;
const SCSI_PREVENT_ALLOW_MEDIUM_REMOVAL: u8 = 0x1E;
const SCSI_READ_CAPACITY_10: u8 = 0x25;
const SCSI_READ_10: u8 = 0x28;
const SCSI_WRITE_10: u8 = 0x2A;

const SENSE_OK: [u8; 18] = [
    0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00,
];

const SENSE_INVALID_CMD: [u8; 18] = [
    0x70, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00,
    0x00, 0x00,
];

/// Block device trait for the storage backend.
#[allow(async_fn_in_trait)]
pub trait BlockDevice {
    /// Block size in bytes (must be 512 for FAT compatibility).
    const BLOCK_SIZE: usize = 512;

    /// Total number of blocks.
    fn block_count(&self) -> u32;

    /// Read one block at `lba` into `buf`.
    async fn read_block(&self, lba: u32, buf: &mut [u8]) -> Result<(), ()>;

    /// Write one block at `lba` from `buf`.
    async fn write_block(&self, lba: u32, buf: &[u8]) -> Result<(), ()>;
}

/// USB MSC class state (must be static).
pub struct MscState {
    last_sense: [u8; 18],
}

impl Default for MscState {
    fn default() -> Self {
        Self::new()
    }
}

impl MscState {
    pub const fn new() -> Self {
        Self {
            last_sense: SENSE_OK,
        }
    }
}

/// USB Mass Storage class.
pub struct MscClass<'d, D: Driver<'d>> {
    read_ep: D::EndpointOut,
    write_ep: D::EndpointIn,
    state: &'d mut MscState,
}

impl<'d, D: Driver<'d>> MscClass<'d, D> {
    /// Create a new MSC class and register it with the USB builder.
    pub fn new(builder: &mut Builder<'d, D>, state: &'d mut MscState, max_packet_size: u16) -> Self {
        let mut func = builder.function(USB_CLASS_MSC, MSC_SUBCLASS_SCSI, MSC_PROTOCOL_BBB);
        let mut iface = func.interface();
        let mut alt = iface.alt_setting(USB_CLASS_MSC, MSC_SUBCLASS_SCSI, MSC_PROTOCOL_BBB, None);
        let read_ep = alt.endpoint_bulk_out(None, max_packet_size);
        let write_ep = alt.endpoint_bulk_in(None, max_packet_size);

        Self {
            read_ep,
            write_ep,
            state,
        }
    }

    /// Run the MSC class forever, serving block device requests.
    pub async fn run<B: BlockDevice>(&mut self, dev: &B) -> ! {
        loop {
            self.read_ep.wait_enabled().await;
            defmt::info!("USB MSC: interface enabled");

            loop {
                let mut cbw_buf = [0u8; 64];
                let n = match self.read_ep.read(&mut cbw_buf).await {
                    Ok(n) => n,
                    Err(_) => break, // disconnected
                };

                if n < CBW_SIZE {
                    defmt::warn!("USB MSC: short CBW ({} bytes)", n);
                    continue;
                }

                let sig = u32::from_le_bytes([cbw_buf[0], cbw_buf[1], cbw_buf[2], cbw_buf[3]]);
                if sig != CBW_SIGNATURE {
                    defmt::warn!("USB MSC: bad CBW signature 0x{:08X}", sig);
                    continue;
                }

                let tag = u32::from_le_bytes([cbw_buf[4], cbw_buf[5], cbw_buf[6], cbw_buf[7]]);
                let transfer_len =
                    u32::from_le_bytes([cbw_buf[8], cbw_buf[9], cbw_buf[10], cbw_buf[11]]);
                let flags = cbw_buf[12]; // bit 7: 1=device-to-host, 0=host-to-device
                let _lun = cbw_buf[13] & 0x0F;
                let cb_len = (cbw_buf[14] & 0x1F) as usize;
                let cb = &cbw_buf[15..15 + cb_len.min(16)];

                let direction_in = flags & 0x80 != 0;
                let opcode = cb.first().copied().unwrap_or(0);

                let (status, data_residue) = self
                    .handle_scsi(dev, opcode, cb, transfer_len, direction_in)
                    .await;

                let mut csw = [0u8; CSW_SIZE];
                csw[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
                csw[4..8].copy_from_slice(&tag.to_le_bytes());
                csw[8..12].copy_from_slice(&data_residue.to_le_bytes());
                csw[12] = status;

                if self.write_ep.write(&csw).await.is_err() {
                    break; // disconnected
                }
            }
        }
    }

    async fn handle_scsi<B: BlockDevice>(
        &mut self,
        dev: &B,
        opcode: u8,
        cb: &[u8],
        transfer_len: u32,
        direction_in: bool,
    ) -> (u8, u32) {
        match opcode {
            SCSI_TEST_UNIT_READY => {
                self.state.last_sense = SENSE_OK;
                (CSW_STATUS_PASSED, 0)
            }

            SCSI_REQUEST_SENSE => {
                let sense = self.state.last_sense;
                let len = (transfer_len as usize).min(sense.len());
                let _ = self.write_ep.write(&sense[..len]).await;
                self.state.last_sense = SENSE_OK;
                (CSW_STATUS_PASSED, transfer_len - len as u32)
            }

            SCSI_INQUIRY => {
                let mut resp = [0u8; 36];
                resp[0] = 0x00; // direct access block device
                resp[1] = 0x80; // removable
                resp[2] = 0x02; // SPC-2
                resp[3] = 0x02; // response data format
                resp[4] = 31; // additional length
                resp[8..16].copy_from_slice(b"CyberEgg");
                resp[16..32].copy_from_slice(b"FAT12 Storage   ");
                resp[32..36].copy_from_slice(b"1.00");

                let len = (transfer_len as usize).min(resp.len());
                let _ = self.write_ep.write(&resp[..len]).await;
                (CSW_STATUS_PASSED, transfer_len - len as u32)
            }

            SCSI_READ_CAPACITY_10 => {
                let last_lba = dev.block_count().saturating_sub(1);
                let block_size = B::BLOCK_SIZE as u32;

                let mut resp = [0u8; 8];
                resp[0..4].copy_from_slice(&last_lba.to_be_bytes());
                resp[4..8].copy_from_slice(&block_size.to_be_bytes());

                let len = (transfer_len as usize).min(8);
                let _ = self.write_ep.write(&resp[..len]).await;
                (CSW_STATUS_PASSED, transfer_len - len as u32)
            }

            SCSI_MODE_SENSE_6 => {
                let resp = [0x03, 0x00, 0x00, 0x00];
                let len = (transfer_len as usize).min(resp.len());
                let _ = self.write_ep.write(&resp[..len]).await;
                (CSW_STATUS_PASSED, transfer_len - len as u32)
            }

            SCSI_PREVENT_ALLOW_MEDIUM_REMOVAL => (CSW_STATUS_PASSED, 0),

            SCSI_READ_10 if direction_in => {
                if cb.len() < 10 {
                    self.state.last_sense = SENSE_INVALID_CMD;
                    return (CSW_STATUS_FAILED, transfer_len);
                }
                let lba = u32::from_be_bytes([cb[2], cb[3], cb[4], cb[5]]);
                let count = u16::from_be_bytes([cb[7], cb[8]]) as u32;

                let mut buf = [0u8; 512];
                let mut residue = transfer_len;
                for i in 0..count {
                    if dev.read_block(lba + i, &mut buf).await.is_err() {
                        self.state.last_sense = SENSE_INVALID_CMD;
                        return (CSW_STATUS_FAILED, residue);
                    }
                    for chunk in buf.chunks(64) {
                        if self.write_ep.write(chunk).await.is_err() {
                            return (CSW_STATUS_FAILED, residue);
                        }
                    }
                    residue = residue.saturating_sub(B::BLOCK_SIZE as u32);
                }
                (CSW_STATUS_PASSED, residue)
            }

            SCSI_WRITE_10 if !direction_in => {
                if cb.len() < 10 {
                    self.state.last_sense = SENSE_INVALID_CMD;
                    return (CSW_STATUS_FAILED, transfer_len);
                }
                let lba = u32::from_be_bytes([cb[2], cb[3], cb[4], cb[5]]);
                let count = u16::from_be_bytes([cb[7], cb[8]]) as u32;

                let mut buf = [0u8; 512];
                let mut residue = transfer_len;
                for i in 0..count {
                    let mut pos = 0;
                    while pos < 512 {
                        match self.read_ep.read(&mut buf[pos..]).await {
                            Ok(n) => pos += n,
                            Err(_) => return (CSW_STATUS_FAILED, residue),
                        }
                    }
                    if dev.write_block(lba + i, &buf).await.is_err() {
                        self.state.last_sense = SENSE_INVALID_CMD;
                        return (CSW_STATUS_FAILED, residue);
                    }
                    residue = residue.saturating_sub(B::BLOCK_SIZE as u32);
                }
                (CSW_STATUS_PASSED, residue)
            }

            _ => {
                defmt::warn!("USB MSC: unsupported SCSI opcode 0x{:02X}", opcode);
                self.state.last_sense = SENSE_INVALID_CMD;
                (CSW_STATUS_FAILED, transfer_len)
            }
        }
    }
}
