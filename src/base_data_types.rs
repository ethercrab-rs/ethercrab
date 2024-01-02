/// Base data types, defined in ETG1020 Table 98 (etc) or ETG1000.6 Table 64.
///
/// Many more data types are defined, however this enum only lists the primitive types. Other types,
/// e.g. ETG1020 Table 100 should be defined elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ethercrab_wire::EtherCatWire)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum PrimitiveDataType {
    Unknown = 0x00,

    /// Boolean, bit, on or off.
    Bool = 0x01,
    /// A single byte.
    Byte = 0x1E,
    /// A two byte (16 bit) word.
    Word = 0x1F,
    /// A 4 byte (32 bit) double word.
    DWord = 0x20,

    // ---
    // Bit String
    // ---
    /// A single bit.
    Bit1 = 0x30,
    /// 2 invidual bits.
    Bit2 = 0x31,
    /// 3 invidual bits.
    Bit3 = 0x32,
    /// 4 invidual bits.
    Bit4 = 0x33,
    /// 5 invidual bits.
    Bit5 = 0x34,
    /// 6 invidual bits.
    Bit6 = 0x35,
    /// 7 invidual bits.
    Bit7 = 0x36,
    /// 8 invidual bits.
    Bit8 = 0x37,
    /// 9 invidual bits.
    Bit9 = 0x38,
    /// 10 invidual bits.
    Bit10 = 0x39,
    /// 11 invidual bits.
    Bit11 = 0x3A,
    /// 12 invidual bits.
    Bit12 = 0x3B,
    /// 13 invidual bits.
    Bit13 = 0x3C,
    /// 14 invidual bits.
    Bit14 = 0x3D,
    /// 15 invidual bits.
    Bit15 = 0x3E,
    /// 16 invidual bits.
    Bit16 = 0x3F,
    /// 8 individual Bits
    BitArr8 = 0x2D,
    /// 16 individual Bits
    BitArr16 = 0x2E,
    /// 32 individual Bits
    BitArr32 = 0x2F,

    // ---
    // Signed Integer
    // ---
    /// SINT 8 Short Integer -128 to 127
    I8 = 0x02,
    /// INT 16 Integer -32 768 to 32 767
    I16 = 0x03,
    /// INT24 24 -223 to 223-1
    I24 = 0x10,
    /// DINT 32 Double Integer -231 to 231-1
    I32 = 0x04,
    /// INT40 40
    I40 = 0x12,
    /// INT48 48
    I48 = 0x13,
    /// INT56 56
    I56 = 0x14,
    /// LINT 64 Long Integer -263 to 263-1
    I64 = 0x15,

    // ---
    // Unsigned Integer
    // ---
    /// USINT 8 Unsigned Short Integer 0 to 255
    U8 = 0x05,
    /// UINT 16 Unsigned Integer / Word 0 to 65 535
    U16 = 0x06,
    /// UINT24 24
    U24 = 0x16,
    /// UDINT 32 Unsigned Double Integer 0 to 232-1
    U32 = 0x07,
    /// UINT40 40
    U40 = 0x18,
    /// UINT48 48
    U48 = 0x19,
    /// UINT56 56
    U56 = 0x1A,
    /// ULINT 64 Unsigned Long Integer 0 to 264-1
    U64 = 0x1B,

    // ---
    // Floating point
    // ---
    /// REAL 32 Floating point
    F32 = 0x08,
    /// LREAL 64 Long float
    F64 = 0x11,
}
