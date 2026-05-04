pub mod iso14443_3 {
    use core::future::Future;

    use defmt::info;
    use embassy_nrf::nfct::{Error, NfcT};

    pub trait Card {
        type Error;
        fn receive(&mut self, buf: &mut [u8]) -> impl Future<Output = Result<usize, Self::Error>>;
        fn transmit(&mut self, buf: &[u8]) -> impl Future<Output = Result<(), Self::Error>>;
    }

    impl<T: Card> Card for &mut T {
        type Error = T::Error;

        fn receive(&mut self, buf: &mut [u8]) -> impl Future<Output = Result<usize, Self::Error>> {
            T::receive(self, buf)
        }

        fn transmit(&mut self, buf: &[u8]) -> impl Future<Output = Result<(), Self::Error>> {
            T::transmit(self, buf)
        }
    }

    impl<'a> Card for NfcT<'a> {
        type Error = Error;

        fn receive(&mut self, buf: &mut [u8]) -> impl Future<Output = Result<usize, Self::Error>> {
            self.receive(buf)
        }

        fn transmit(&mut self, buf: &[u8]) -> impl Future<Output = Result<(), Self::Error>> {
            self.transmit(buf)
        }
    }

    pub struct Logger<T: Card>(pub T);

    impl<T: Card> Card for Logger<T> {
        type Error = T::Error;

        async fn receive(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            let n = T::receive(&mut self.0, buf).await?;
            info!("<- {:02x}", &buf[..n]);
            Ok(n)
        }

        fn transmit(&mut self, buf: &[u8]) -> impl Future<Output = Result<(), Self::Error>> {
            info!("-> {:02x}", buf);
            T::transmit(&mut self.0, buf)
        }
    }
}

pub mod iso14443_4 {
    use defmt::info;

    use super::iso14443_3;

    #[derive(defmt::Format)]
    pub enum Error<T> {
        Deselected,
        Protocol,
        Lower(T),
    }

    pub trait Card {
        type Error;
        fn receive(&mut self, buf: &mut [u8]) -> impl Future<Output = Result<usize, Self::Error>>;
        fn transmit(&mut self, buf: &[u8]) -> impl Future<Output = Result<(), Self::Error>>;
    }

    pub struct IsoDep<T: iso14443_3::Card> {
        nfc: T,

        /// Block count spin bit: 0 or 1
        block_num: u8,

        /// true if deselected. This is permanent, you must create another
        /// IsoDep instance if we get selected again.
        deselected: bool,

        /// last response, in case we need to retransmit.
        resp: [u8; 256],
        resp_len: usize,
    }

    impl<T: iso14443_3::Card> IsoDep<T> {
        pub fn new(nfc: T) -> Self {
            Self {
                nfc,
                block_num: 1,
                deselected: false,
                resp: [0u8; 256],
                resp_len: 0,
            }
        }
    }

    impl<T: iso14443_3::Card> Card for IsoDep<T> {
        type Error = Error<T::Error>;

        async fn receive(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            if self.deselected {
                return Err(Error::Deselected);
            }

            let mut temp = [0u8; 256];

            loop {
                let n = self.nfc.receive(&mut temp).await.map_err(Error::Lower)?;
                assert!(n != 0);
                match temp[0] {
                    0x02 | 0x03 => {
                        // ISO-DEP I-block.  block_num bit toggles per
                        // accepted block; if the incoming bit matches
                        // our current `block_num` it is a retransmit
                        // of the last block (phone didn't see our
                        // reply) — resend the cached response without
                        // advancing state.
                        let incoming = temp[0] & 0x01;
                        if incoming == self.block_num {
                            info!("Got I-block retransmit, re-sending last response.");
                            let resp: &[u8] = &self.resp[..self.resp_len];
                            self.nfc.transmit(resp).await.map_err(Error::Lower)?;
                            continue;
                        }
                        if incoming != (self.block_num ^ 0x01) {
                            info!("Got I-block with unexpected block_num {:02x}", temp[0]);
                            return Err(Error::Protocol);
                        }
                        self.block_num ^= 0x01;
                        buf[..n - 1].copy_from_slice(&temp[1..n]);
                        return Ok(n - 1);
                    }
                    0xb2 | 0xb3 => {
                        if temp[0] & 0x01 != self.block_num {
                            info!("Got NAK, transmitting ACK.");
                            let resp = &[0xA2 | self.block_num];
                            self.nfc.transmit(resp).await.map_err(Error::Lower)?;
                        } else {
                            info!("Got NAK, retransmitting.");
                            let resp: &[u8] = &self.resp[..self.resp_len];
                            self.nfc.transmit(resp).await.map_err(Error::Lower)?;
                        }
                    }
                    0xe0 => {
                        info!("Got RATS, tx'ing ATS");
                        let resp = &[0x06, 0x77, 0x77, 0x81, 0x02, 0x80];
                        self.nfc.transmit(resp).await.map_err(Error::Lower)?;
                    }
                    0xc2 => {
                        info!("Got deselect!");
                        self.deselected = true;
                        let resp = &[0xC2];
                        self.nfc.transmit(resp).await.map_err(Error::Lower)?;
                        return Err(Error::Deselected);
                    }
                    _ => {
                        info!("Got unknown command {:02x}!", temp[0]);
                        return Err(Error::Protocol);
                    }
                };
            }
        }

        async fn transmit(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
            if self.deselected {
                return Err(Error::Deselected);
            }

            self.resp[0] = 0x02 | self.block_num;
            self.resp[1..][..buf.len()].copy_from_slice(buf);
            self.resp_len = 1 + buf.len();

            let resp: &[u8] = &self.resp[..self.resp_len];
            self.nfc.transmit(resp).await.map_err(Error::Lower)?;

            Ok(())
        }
    }
}
