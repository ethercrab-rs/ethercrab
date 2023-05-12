//! Traits and impls used to read/write data to/from the wire.

use core::{
	marker::PhantomData,
	fmt,
	};
use packed_struct as packed;
pub use packed_struct::{PackingResult, PackingError, types::bits::ByteArray};

/**
	trait for data types than can be packed/unpacked to/from a PDU
	
	Note:
		this is currently redudnant with [PduData] and should be merged properly
*/
pub trait PduData: Sized {
	const id: TypeId;
    type ByteArray: ByteArray;

    fn pack(&self) -> PackingResult<Self::ByteArray>;
    fn unpack(src: &Self::ByteArray) -> PackingResult<Self>;
}
/// trait marking a [PackedStruct] is a [PduData]
pub trait PduStruct: packed::PackedStruct {}
impl<T: PduStruct> PduData for T {
	const id: TypeId = TypeId::CUSTOM;
	type ByteArray = <T as packed::PackedStruct>::ByteArray;
	
	fn pack(&self) -> PackingResult<Self::ByteArray>    {packed::PackedStruct::pack(self)}
	fn unpack(src: &Self::ByteArray) -> PackingResult<Self>  {packed::PackedStruct::unpack(src)}
}

// trait ByteArrayFrom: ByteArray {
// 	fn from_slice(src: &[u8]) -> Self {
// 		let new = Self::new(0);
// 		new.as_bytes_slice_mut().copy_from_slice(src);
// 		new
// 	}
// }
// impl<T: ByteArray> ByteArrayFrom for T {}

/** dtype identifiers associated to dtypes allowing to dynamically check the type of a [DType] implementor
	
	It is only convering the common useful types and not all the possible implementors of [DType]
*/
#[derive(Copy, Clone, Debug)]
pub enum TypeId {
	CUSTOM,
	BOOL,
	I8, I16, I32, I64,
	U8, U16, U32, U64,
	F32, F64,
}

impl<const N: usize> PduData for [u8; N] {
	const id: TypeId = TypeId::CUSTOM;
	type ByteArray = Self;
	
	fn pack(&self) -> PackingResult<Self::ByteArray>    {Ok(*self)}
	fn unpack(src: &Self::ByteArray) -> PackingResult<Self>  {Ok(*src)}
}

macro_rules! impl_pdudata {
	($t: ty, $id: ident) => { impl PduData for $t {
			const id: TypeId = TypeId::$id;
			type ByteArray = [u8; core::mem::size_of::<$t>()];
			
			fn pack(&self) -> PackingResult<Self::ByteArray> {
				Ok(self.to_le_bytes())
			}
			fn unpack(src: &Self::ByteArray) -> PackingResult<Self> {
				Ok(Self::from_le_bytes(src.clone()))
			}
		}};
	($t: ty) => { impl_pdudata_float(t, TypeId::CUSTOM) };
}

impl_pdudata!(u8, U8);
impl_pdudata!(u16, U16);
impl_pdudata!(u32, U32);
impl_pdudata!(u64, U64);
impl_pdudata!(i8, I8);
impl_pdudata!(i16, I16);
impl_pdudata!(i32, I32);
impl_pdudata!(i64, I64);
impl_pdudata!(f32, F32);
impl_pdudata!(f64, F64);



/** 
	locate some data in a datagram by its byte position and length, which must be extracted to type `T` to be processed in rust
	
	It acts like a getter/setter of a value in a byte sequence. One can think of it as an offset to a data location because it does not actually point the data but only its offset in the byte sequence, it also contains its length to dynamically check memory bounds.
*/
#[derive(Default, Clone)]
pub struct Field<T: PduData> {
	extracted: PhantomData<T>,
	/// start byte index of the object
	pub byte: usize,
	/// byte length of the object
	pub len: usize,
}
impl<T, const N: usize> Field<T> 
where T: PduData<ByteArray=[u8; N]>
{
	/// build a Field from its content
	pub fn new(byte: usize, len: usize) -> Self {
		Self{extracted: PhantomData, byte, len}
	}
	/// extract the value pointed by the field in the given byte array
	pub fn get(&self, data: &[u8]) -> T       {
		T::unpack(&data[self.byte..][..self.len]
					.try_into()
					.expect("wrong data length"))
				.expect("cannot unpack from data")
	}
	/// dump the given value to the place pointed by the field in the byte array
	pub fn set(&self, data: &mut [u8], value: T)   {
		data[self.byte..][..self.len].copy_from_slice(
			value.pack()
				.expect("cannot pack data")
				.as_bytes_slice()
			);
	}
}
impl<T: PduData> fmt::Debug for Field<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Field{{0x{:x}, {}}}", self.byte, self.len)
	}
}
/** 
	locate some data in a datagram by its bit position and length, which must be extracted to type `T` to be processed in rust
	
	It acts like a getter/setter of a value in a byte sequence. One can think of it as an offset to a data location because it does not actually point the data but only its offset in the byte sequence, it also contains its length to dynamically check memory bounds.
*/
#[derive(Default, Clone)]
pub struct BitField<T: PduData> {
	extracted: PhantomData<T>,
	/// start bit index of the object
	pub bit: usize,
	/// bit length of the object
	pub len: usize,
}
impl<T: PduData> BitField<T> {
	/// build a Field from its content
	pub fn new(bit: usize, len: usize) -> Self {
		Self{extracted: PhantomData, bit, len}
	}
	/// extract the value pointed by the field in the given byte array
	pub fn get(&self, data: &[u8]) -> T       {todo!()}
	/// dump the given value to the place pointed by the field in the byte array
	pub fn set(&self, data: &mut [u8], value: T)   {todo!()}
}
impl<T: PduData> fmt::Debug for BitField<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "BitField{{{}, {}}}", self.bit, self.len)
	}
}



/*
impl<const N: usize> PduData for [u8; N] {
    fn as_slice(&self) -> &[u8] {
        self
    }
}

impl PduRead for () {
    const LEN: u16 = 0;

    type Error = TryFromSliceError;

    fn try_from_slice(_slice: &[u8]) -> Result<Self, Self::Error> {
        Ok(())
    }
}

impl PduData for () {
    fn as_slice(&self) -> &[u8] {
        &[]
    }
}

impl<const N: usize, T> PduRead for [T; N]
where
    T: PduRead,
{
    const LEN: u16 = T::LEN * N as u16;

    type Error = ();

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let chunks = slice.chunks_exact(usize::from(T::LEN));

        let mut res = heapless::Vec::<T, N>::new();

        for chunk in chunks {
            res.push(T::try_from_slice(chunk).map_err(|_| ())?)
                .map_err(|_| ())?;
        }

        res.into_array().map_err(|_| ())
    }
}

impl<const N: usize> PduRead for heapless::String<N> {
    const LEN: u16 = N as u16;

    type Error = VisibleStringError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let mut out = heapless::String::new();

        out.push_str(core::str::from_utf8(slice).map_err(VisibleStringError::Decode)?)
            .map_err(|_| VisibleStringError::TooLong)?;

        Ok(out)
    }
}

/// A "Visible String" representation. Characters are specified to be within the ASCII range.
impl<const N: usize> PduData for heapless::String<N> {
    fn as_slice(&self) -> &[u8] {
        self.as_bytes()
    }
}
*/
