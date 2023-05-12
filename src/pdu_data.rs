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
	const ID: TypeId;
    type ByteArray: ByteArray;

    fn pack(&self) -> Self::ByteArray;
    fn unpack(src: &[u8]) -> PackingResult<Self>;
}
/// trait marking a [PackedStruct] is a [PduData]
pub trait PduStruct: packed::PackedStruct {}
impl<T: PduStruct> PduData for T {
	const ID: TypeId = TypeId::CUSTOM;
	type ByteArray = <T as packed::PackedStruct>::ByteArray;
	
	fn pack(&self) -> Self::ByteArray    {packed::PackedStruct::pack(self).unwrap()}
	fn unpack(src: &[u8]) -> PackingResult<Self>  {packed::PackedStructSlice::unpack_from_slice(src)}
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
	const ID: TypeId = TypeId::CUSTOM;
	type ByteArray = Self;
	
	fn pack(&self) -> Self::ByteArray    {*self}
	fn unpack(src: &[u8]) -> PackingResult<Self>  {
		Ok(Self::try_from(src)
			.map_err(|_| PackingError::BufferSizeMismatch{
				expected: N, 
				actual: src.len(),
				})?
			.clone())
	}
}

macro_rules! impl_pdudata {
	($t: ty, $id: ident) => { impl PduData for $t {
			const ID: TypeId = TypeId::$id;
			type ByteArray = [u8; core::mem::size_of::<$t>()];
			
			fn pack(&self) -> Self::ByteArray {
				self.to_le_bytes()
			}
			fn unpack(src: &[u8]) -> PackingResult<Self> {
				Ok(Self::from_le_bytes(src
					.try_into()
					.map_err(|_|  PackingError::BufferSizeMismatch{
								expected: core::mem::size_of::<$t>(),
								actual: src.len(),
								})?
					))
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
impl<T: PduData> Field<T>
{
	/// build a Field from its content
	pub fn new(byte: usize, len: usize) -> Self {
		Self{extracted: PhantomData, byte, len}
	}
	/// extract the value pointed by the field in the given byte array
	pub fn get(&self, data: &[u8]) -> T       {
		T::unpack(&data[self.byte..][..self.len])
				.expect("cannot unpack from data")
	}
	/// dump the given value to the place pointed by the field in the byte array
	pub fn set(&self, data: &mut [u8], value: T)   {
		data[self.byte..][..self.len].copy_from_slice(
			value.pack().as_bytes_slice()
			);
	}
}
impl<T: PduData> fmt::Debug for Field<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Field{{{}, {}}}", self.byte, self.len)
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
	pub fn get(&self, _data: &[u8]) -> T       {todo!()}
	/// dump the given value to the place pointed by the field in the byte array
	pub fn set(&self, _data: &mut [u8], _value: T)   {todo!()}
}
impl<T: PduData> fmt::Debug for BitField<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "BitField{{{}, {}}}", self.bit, self.len)
	}
}

