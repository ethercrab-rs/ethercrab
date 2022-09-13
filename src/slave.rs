use crate::{
    al_control::AlControl,
    al_status::AlState,
    al_status_code::AlStatusCode,
    client::Client,
    error::Error,
    pdu::CheckWorkingCounter,
    register::RegisterAddress,
    sii::{CategoryType, SiiCategory, SiiCoding, SiiControl, SiiGeneral, SiiReadSize, SiiRequest},
    timer_factory::TimerFactory,
    PduRead,
};
use core::{cell::RefMut, str::FromStr, time::Duration};
use nom::{multi::length_data, number::complete::le_u8};
use packed_struct::PackedStruct;

#[derive(Clone, Debug)]
pub struct Slave {
    pub configured_address: u16,
    pub state: AlState,
}

impl Slave {
    pub fn new(configured_address: u16, state: AlState) -> Self {
        Self {
            configured_address,
            state,
        }
    }
}

pub struct SlaveRef<
    'a,
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    const MAX_SLAVES: usize,
    TIMEOUT,
> {
    client: &'a Client<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>,
    slave: RefMut<'a, Slave>,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, const MAX_SLAVES: usize, TIMEOUT>
    SlaveRef<'a, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new(
        client: &'a Client<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>,
        slave: RefMut<'a, Slave>,
    ) -> Self {
        Self { client, slave }
    }

    pub async fn request_slave_state(&self, state: AlState) -> Result<(), Error> {
        debug!(
            "Set state {} for slave address {:#04x}",
            state, self.slave.configured_address
        );

        // Send state request
        self.client
            .fpwr(
                self.slave.configured_address,
                RegisterAddress::AlControl,
                AlControl::new(state).pack().unwrap(),
            )
            .await?
            .wkc(1, "AL control")?;

        let res = crate::timeout::<TIMEOUT, _, _>(Duration::from_millis(1000), async {
            loop {
                let status = self
                    .client
                    .fprd::<AlControl>(self.slave.configured_address, RegisterAddress::AlStatus)
                    .await?
                    .wkc(1, "AL status")?;

                if status.state == state {
                    break Result::<(), _>::Ok(());
                }

                TIMEOUT::timer(Duration::from_millis(10)).await;
            }
        })
        .await;

        match res {
            Err(Error::Timeout) => {
                // TODO: Extract into separate method to get slave status code
                {
                    let (status, _working_counter) = self
                        .client
                        .fprd::<AlStatusCode>(
                            self.slave.configured_address,
                            RegisterAddress::AlStatusCode,
                        )
                        .await?;

                    debug!("Slave status code: {}", status);
                }

                Err(Error::Timeout)
            }
            other => other,
        }
    }

    // TODO: Make a new SiiRead trait instead of repurposing PduRead - some types can only be read
    // from EEPROM.
    pub async fn read_eeprom<T>(&self, eeprom_address: SiiCoding) -> Result<T, Error>
    where
        T: PduRead,
    {
        // TODO: Make this a const condition when possible
        debug_assert!(T::LEN <= 8);

        let eeprom_address = u16::from(eeprom_address);

        let buf = self.read_eeprom_raw(eeprom_address).await?;

        let buf = buf.get(0..usize::from(T::LEN)).ok_or(Error::EepromDecode)?;

        T::try_from_slice(buf).map_err(|_| Error::EepromDecode)
    }

    // TODO: This only ever returns 4 or 8 byte reads. Can we just return an enum instead of writing
    // into a buffer? Then a method to turn it into an array of N bytes or a slice.
    pub async fn read_eeprom_raw(
        &self,
        eeprom_address: impl Into<u16>,
    ) -> Result<heapless::Vec<u8, 8>, Error> {
        let eeprom_address: u16 = eeprom_address.into();

        // TODO: Check EEPROM error flags

        let setup = SiiRequest::read(eeprom_address);

        // Set up an SII read. This writes the control word and the register word after it
        self.client
            .fpwr(
                self.slave.configured_address,
                RegisterAddress::SiiControl,
                setup.to_array(),
            )
            .await?
            .wkc(1, "SII read setup")?;

        // TODO: Configurable timeout
        let timeout = core::time::Duration::from_millis(10);

        let read_size = crate::timeout::<TIMEOUT, _, _>(timeout, async {
            loop {
                let control = self
                    .client
                    .fprd::<SiiControl>(self.slave.configured_address, RegisterAddress::SiiControl)
                    .await?
                    .wkc(1, "SII busy wait")?;

                if control.busy == false {
                    break Ok(control.read_size);
                }

                // TODO: Configurable loop tick
                TIMEOUT::timer(core::time::Duration::from_millis(1)).await;
            }
        })
        .await?;

        let data = match read_size {
            SiiReadSize::Octets4 => {
                let data = self
                    .client
                    .fprd::<[u8; 4]>(self.slave.configured_address, RegisterAddress::SiiData)
                    .await?
                    .wkc(1, "SII data")?;

                log::debug!("Read {:#04x?} {:02x?}", eeprom_address, data);

                heapless::Vec::from_slice(&data)
            }
            SiiReadSize::Octets8 => {
                let data = self
                    .client
                    .fprd::<[u8; 8]>(self.slave.configured_address, RegisterAddress::SiiData)
                    .await?
                    .wkc(1, "SII data")?;

                log::debug!("Read {:#04x?} {:02x?}", eeprom_address, data);

                heapless::Vec::from_slice(&data)
            }
        };

        data.map_err(|_| Error::EepromDecode)
    }

    pub async fn device_name<const N: usize>(&self) -> Result<Option<heapless::String<N>>, Error> {
        let general = self.load_eeprom_general().await?;

        let name_idx = general.name_string_idx;

        self.find_string(name_idx).await
    }

    async fn load_eeprom_general(&self) -> Result<SiiGeneral, Error> {
        let category = self
            .find_eeprom_category_start(CategoryType::General)
            .await?
            .ok_or(Error::EepromNoCategory)?;

        let len = usize::from(category.len);
        let mut start = category.start;

        // Chunks are read in multiples of 4 or 8 and we need at least 18 bytes
        let mut buf = heapless::Vec::<u8, 24>::new();

        // TODO: This loop needs splitting into a function which fills up a slice and returns it
        let buf = loop {
            let sl = self.read_eeprom_raw(start).await.unwrap();
            log::debug!("Read {start:#06x?} {:02x?}", sl);
            // Each EEPROM address contains 2 bytes, so we need to step half as fast
            start += sl.len() as u16 / 2;

            buf.extend_from_slice(sl.as_slice())
                .expect("Buffer is full");

            if buf.len() >= len {
                break &buf[0..len];
            }
        };

        let (_, general) = SiiGeneral::parse(buf).expect("General parse");

        Ok(general)
    }

    /// Find a string by index.
    ///
    /// Note that SII string indices start at 1, not 0.
    ///
    /// Passing an index of 0 will return `None`, although the spec defines index 0 as an empty
    /// string as per ETG1000.6 Table 20 footnote.
    // TODO: This is a pretty inefficient algorithm. Find a way to use only `N` bytes as well as
    // skipping ignored strings instead of reading them into the buffer.
    async fn find_string<const N: usize>(
        &self,
        search_index: u8,
    ) -> Result<Option<heapless::String<N>>, Error> {
        if search_index == 0 {
            return Ok(None);
        }

        let pos = self
            .find_eeprom_category_start(CategoryType::Strings)
            .await?;

        if let Some(pos) = pos {
            let mut start = pos.start;

            let sl = self.read_eeprom_raw(start).await.unwrap();
            // Each EEPROM address contains 2 bytes, so we need to step half as fast
            start += sl.len() as u16 / 2;

            // The first byte of the strings section is the number of strings contained within it
            let (num_strings, buf) = sl.split_first().expect("Split first");
            let num_strings = *num_strings;

            log::debug!("Found {num_strings} strings");

            // Initialise the buffer with the remaining first read
            // TODO: Use `{ N + 8 }` when generic_const_exprs is stabilised
            let mut buf = heapless::Vec::<u8, 255>::from_slice(buf).unwrap();

            for idx in 0..num_strings {
                // TODO: DRY: This loop needs splitting into a function which fills up a slice and returns it
                loop {
                    let sl = self.read_eeprom_raw(start).await.unwrap();
                    // Each EEPROM address contains 2 bytes, so we need to step half as fast
                    start += sl.len() as u16 / 2;
                    buf.extend_from_slice(sl.as_slice())
                        .expect("Buffer is full");

                    let i = buf.as_slice();

                    let i = match length_data::<_, _, (), _>(le_u8)(i) {
                        Ok((i, string_data)) => {
                            if idx == search_index.saturating_sub(1) {
                                let s = core::str::from_utf8(string_data)
                                    .map_err(|_| Error::EepromDecode)?;

                                let s = heapless::String::from_str(s)
                                    .map_err(|_| Error::EepromDecode)?;

                                return Ok(Some(s));
                            }

                            i
                        }
                        Err(e) => match e {
                            nom::Err::Incomplete(_needed) => {
                                continue;
                            }
                            nom::Err::Error(e) => panic!("Error {e:?}"),
                            nom::Err::Failure(e) => panic!("Fail {e:?}"),
                        },
                    };

                    buf = heapless::Vec::from_slice(i).unwrap();

                    break;
                }
            }

            // TODO: Index out of bounds error
            Err(Error::EepromDecode)
        } else {
            Ok(None)
        }
    }

    // TODO: Split out all EEPROM functions so we can do something like `slave.eeprom().whatever()`
    async fn find_eeprom_category_start(
        &self,
        category: CategoryType,
    ) -> Result<Option<SiiCategory>, Error> {
        // TODO: Const SII_FIRST_SECTION_START - defined in ETG1000.6 Table 17 – "Slave Information
        // Interface Categories". There's a bunch of bootstrap information contained in lower
        // addresses.
        let mut start = 0x0040u16;

        loop {
            // First address, returns 2 bytes, contains the category type.
            let (category_type, data_len) = self
                .read_eeprom_raw(start)
                .await
                .map(|chunk| {
                    // SAFETY: `chunk` is always at least 4 bytes long, so the below unwraps and
                    // array indexes are safe.
                    let category = u16::from_le_bytes(chunk[0..2].try_into().unwrap());
                    let len = u16::from_le_bytes(chunk[2..4].try_into().unwrap());

                    (
                        CategoryType::try_from(category).unwrap_or(CategoryType::Nop),
                        len,
                    )
                })
                // FIXME
                .unwrap();

            // Two header bytes
            start += 2;

            log::debug!(
                "Found category {:?}, data starts at {start:#06x?}, length {:#04x?} ({}) bytes",
                category_type,
                data_len,
                data_len
            );

            match category_type {
                cat if cat == category => {
                    break Ok(Some(SiiCategory {
                        category: cat,
                        start,
                        len: data_len,
                    }))
                }
                CategoryType::End => break Ok(None),
                _ => (),
            }

            // Next category starts after the current category's data
            start += data_len;
        }
    }
}
