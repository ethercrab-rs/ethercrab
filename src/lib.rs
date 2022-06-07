const LEN_MASK: u16 = 0b0000_0111_1111_1111;

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
        // TODO: Update previous PDU to not be the last one

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
            buf.extend_from_slice(&pdu.as_bytes());
        }

        let padding = 64usize.saturating_sub(buf.len());
        let mut pad = [0x00u8].repeat(padding);

        buf.append(&mut pad);

        buf
    }
}

pub enum Pdu {
    Aprd(Aprd),
    Fprd(Fprd),
}

impl Pdu {
    pub fn byte_len(&self) -> u16 {
        match self {
            Self::Aprd(c) => c.byte_len(),
            Self::Fprd(c) => c.byte_len(),
        }
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            Self::Aprd(c) => c.as_bytes(),
            Self::Fprd(c) => c.as_bytes(),
        }
    }
}

pub struct Aprd {
    command: u8,
    idx: u8,
    /// Auto increment address.
    adp: i16,
    /// Memory or register address.
    ado: u16,
    /// len(11), reserved(3), circulating(1), next(1)
    packed: u16,
    irq: u16,
    /// Read buffer containing response from slave.
    data: Vec<u8>,
    working_counter: u16,
}

impl Aprd {
    pub fn new(data: Vec<u8>, len: u16, slave_addr: u16, memory_address: u16) -> Self {
        // Other fields are all zero for now
        let packed = len & LEN_MASK;

        let adp = -(i16::try_from(slave_addr).expect("Bad slave addr"));

        Self {
            command: Command::Aprd as u8,
            idx: 0,
            adp,
            ado: memory_address,
            packed,
            irq: 0,
            // TODO: This is a read, so this ends up being a buffer, right?
            data,
            working_counter: 0,
        }
    }

    /// Length of this entire struct in bytes
    pub fn byte_len(&self) -> u16 {
        // 11 bytes fixed overhead
        let static_len = 11;

        static_len + u16::try_from(self.data.len()).expect("Too long")
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.push(self.command);
        buf.push(self.idx);
        buf.extend_from_slice(&self.adp.to_le_bytes());
        buf.extend_from_slice(&self.ado.to_le_bytes());
        buf.extend_from_slice(&self.packed.to_le_bytes());
        buf.extend_from_slice(&self.irq.to_le_bytes());
        buf.extend_from_slice(&self.data);
        buf.extend_from_slice(&self.working_counter.to_le_bytes());

        buf
    }
}

pub struct Fprd {
    command: u8,
    idx: u8,
    /// Auto increment address.
    adp: i16,
    /// Memory or register address.
    ado: u16,
    /// len(11), reserved(3), circulating(1), next(1)
    packed: u16,
    irq: u16,
    /// Read buffer containing response from slave.
    data: Vec<u8>,
    working_counter: u16,
}

impl Fprd {
    pub fn new(data: Vec<u8>, len: u16, slave_addr: u16, memory_address: u16) -> Self {
        // Other fields are all zero for now
        let packed = len & LEN_MASK;

        let adp = -(i16::try_from(slave_addr).expect("Bad slave addr"));

        Self {
            command: Command::Fprd as u8,
            // Hard coded to match Wireshark capture
            idx: 0xdc,
            adp,
            ado: memory_address,
            packed,
            irq: 0,
            // TODO: This is a read, so this ends up being a buffer, right?
            data,
            working_counter: 0,
        }
    }

    /// Length of this entire struct in bytes
    pub fn byte_len(&self) -> u16 {
        // 11 bytes fixed overhead
        let static_len = 11;

        static_len + u16::try_from(self.data.len()).expect("Too long")
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.push(self.command);
        buf.push(self.idx);
        buf.extend_from_slice(&self.adp.to_le_bytes());
        buf.extend_from_slice(&self.ado.to_le_bytes());
        buf.extend_from_slice(&self.packed.to_le_bytes());
        buf.extend_from_slice(&self.irq.to_le_bytes());
        buf.extend_from_slice(&self.data);
        buf.extend_from_slice(&self.working_counter.to_le_bytes());

        buf
    }
}

pub enum Command {
    Aprd = 0x01,
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
