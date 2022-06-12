use std::io::{self, Write};

use smoltcp::wire::EthernetFrame;

const LEN_MASK: u16 = 0b0000_0111_1111_1111;

#[derive(Debug)]
pub struct EthercatPduFrame {
    /// Length of PDUs in this frame (excludes the packed u16 header)
    len: u16,
    pdus: Vec<Pdu>,
}

impl EthercatPduFrame {
    pub fn new() -> Self {
        Self {
            len: 0,
            pdus: Vec::new(),
        }
    }

    pub fn push_pdu(&mut self, pdu: Pdu) {
        self.pdus.last_mut().map(|last| {
            last.set_has_next(true);
        });

        self.len += pdu.byte_len();

        self.pdus.push(pdu);
    }

    pub fn as_bytes(&self, mut buf: &mut [u8]) -> io::Result<()> {
        let packed = {
            let len = self.len & LEN_MASK;

            assert_eq!(len, self.len, "Length was truncated");

            // TODO: Const 0x01 = DLPDUs
            let protocol_type = 0x01u16 << 12;

            len | protocol_type
        };

        buf.write_all(&packed.to_le_bytes())?;

        for pdu in &self.pdus {
            pdu.as_bytes(buf)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum Pdu {
    Fprd(Fprd),
}

impl Pdu {
    pub fn byte_len(&self) -> u16 {
        match self {
            Self::Fprd(c) => c.byte_len(),
        }
    }

    pub fn as_bytes(&self, buf: &mut [u8]) -> io::Result<()> {
        match self {
            Self::Fprd(c) => c.as_bytes(buf),
        }
    }

    fn set_has_next(&mut self, has_next: bool) {
        match self {
            Self::Fprd(c) => c.set_has_next(has_next),
        }
    }
}

#[derive(Debug)]
pub struct Fprd {
    command: u8,
    idx: u8,
    adp: u16,
    /// Memory or register address.
    ado: u16,
    /// len(11), reserved(3), circulating(1), next(1)
    packed: u16,
    irq: u16,
    /// Read buffer containing response from slave.
    data: Vec<u8>,
    working_counter: u16,

    // Not in PDU
    data_len: u16,
}

impl Fprd {
    pub fn new(len: u16, slave_addr: u16, memory_address: u16) -> Self {
        // Other fields are all zero for now
        let packed = len & LEN_MASK;

        Self {
            command: Command::Fprd as u8,
            idx: 0x01,
            adp: slave_addr,
            ado: memory_address,
            packed,
            irq: 0,
            data: Vec::with_capacity(len.into()),
            working_counter: 0,
            data_len: len,
        }
    }

    /// Length of this entire struct in bytes
    pub fn byte_len(&self) -> u16 {
        let static_len = 12;

        static_len + u16::try_from(self.data_len).expect("Too long")
    }

    pub fn as_bytes(&self, mut buf: &mut [u8]) -> io::Result<()> {
        buf.write_all(&[self.command])?;
        buf.write_all(&[self.idx])?;
        buf.write_all(&self.adp.to_le_bytes())?;
        buf.write_all(&self.ado.to_le_bytes())?;
        buf.write_all(&self.packed.to_le_bytes())?;
        buf.write_all(&self.irq.to_le_bytes())?;
        // Populate data payload with zeroes. The slave will write data into this section.
        buf.write_all(&[0x00].repeat(self.data_len.into()))?;
        buf.write_all(&self.working_counter.to_le_bytes())?;

        Ok(())
    }

    fn set_has_next(&mut self, has_next: bool) {
        let flag = u16::from(has_next) << 15;

        println!("{flag:016b}");

        self.packed |= flag;
    }
}

pub enum Command {
    Fprd = 0x04,
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
