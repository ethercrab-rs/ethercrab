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

    pub fn as_bytes(&self) -> Vec<u8> {
        let packed = {
            let len = self.len & LEN_MASK;

            assert_eq!(len, self.len, "Length was truncated");

            // TODO: Const 0x01 = DLPDUs
            let protocol_type = 0x01u16 << 12;

            len | protocol_type
        };

        let mut buf = Vec::new();
        buf.extend_from_slice(&packed.to_le_bytes());

        for pdu in &self.pdus {
            dbg!(pdu.byte_len());
            buf.extend_from_slice(&pdu.as_bytes());
        }

        buf
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

    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            Self::Fprd(c) => c.as_bytes(),
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
    pub fn new(data: Vec<u8>, len: u16, slave_addr: u16, memory_address: u16) -> Self {
        // Other fields are all zero for now
        let packed = len & LEN_MASK;

        Self {
            command: Command::Fprd as u8,
            idx: 0x01,
            adp: slave_addr,
            ado: memory_address,
            packed,
            irq: 0,
            // TODO: This is a read, so this ends up being a buffer, right?
            data,
            working_counter: 0,
            data_len: len,
        }
    }

    /// Length of this entire struct in bytes
    pub fn byte_len(&self) -> u16 {
        let static_len = 12;

        static_len + u16::try_from(self.data_len).expect("Too long")
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // TODO: Pass buf in and use `write_all` - Joshua said it's faster

        buf.push(self.command);
        buf.push(self.idx);
        buf.extend_from_slice(&self.adp.to_le_bytes());
        buf.extend_from_slice(&self.ado.to_le_bytes());
        buf.extend_from_slice(&self.packed.to_le_bytes());
        buf.extend_from_slice(&self.irq.to_le_bytes());
        // TODO: Hmmm
        buf.extend_from_slice(&[0x00].repeat(self.data_len.into()));
        buf.extend_from_slice(&self.working_counter.to_le_bytes());

        buf
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
