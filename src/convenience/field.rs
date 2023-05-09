use std::{
	marker::PhantomData,
	fmt,
	};

/** 
	locate some data in a datagram, which must be extracted to type `T` to be processed in rust
	
	It acts like a getter/setter of a value in a byte sequence. One can think of it as an offset to a data location because it does not actually point the data but only its offset in the byte sequence, it also contains its length to dynamically check memory bounds.
*/
#[derive(Default, Clone)]
pub struct Field<T: DType> {
	dtype: PhantomData<T>,
	/// start byte index of the object
	pub byte: usize,
	/// start bit index in the start byte
	pub bit: u8,
	/// bit length of the object
	pub bitlen: usize,
}
impl<T: DType> Field<T> {
	/// build a Field from its content
	pub fn new(byte: usize, bit: u8, bitlen: usize) -> Self {
		Self{dtype: PhantomData, byte, bit, bitlen}
	}
	/// extract the value pointed by the field in the given byte array
	pub fn get(&self, data: &[u8]) -> T       {T::from_dfield(self, data)}
	/// dump the given value to the place pointed by the field in the byte array
	pub fn set(&self, data: &mut [u8], value: T)   {value.to_dfield(self, data)}
}
impl<T: DType> fmt::Debug for Field<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Field")
			.field("byte", &self.byte)
			.field("bit", &self.bit)
			.field("bitlen", &self.bitlen)
			.finish()
	}
}

/**
	trait for data types than can be packed/unpacked to/from a PDU
	
	Note:
		this is currently redudnant with [PduData] and should be merged properly
*/
pub trait DType: Sized {
	fn id() -> TypeID;
	fn from_dfield(field: &Field<Self>, data: &[u8]) -> Self;
	fn to_dfield(&self, field: &Field<Self>, data: &mut [u8]);
}

/** dtype identifiers associated to dtypes allowing to dynamically check the type of a [DType] implementor
	
	It is only convering the common useful types and not all the possible implementors of [DType]
*/
#[derive(Copy, Clone, Debug)]
pub enum TypeID {
	CUSTOM,
	BOOL,
	I8, I16, I32,
	U8, U16, U32,
	F32, F64,
}

// TODO: use a macro instead of all this repeated code

impl DType for f32 {
	fn id() -> TypeID 	{TypeID::F32}
	
	fn from_dfield(field: &Field<Self>, data: &[u8]) -> Self {
		assert_eq!(field.bit, 0, "bit aligned floats are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		Self::from_le_bytes(data[field.byte..field.byte+std::mem::size_of::<Self>()].try_into().expect("wrong data size"))
	}
	fn to_dfield(&self, field: &Field<Self>, data: &mut [u8]) {
		assert_eq!(field.bit, 0, "bit aligned floats are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		data[field.byte..field.byte+std::mem::size_of::<Self>()].copy_from_slice(&self.to_le_bytes());
	}
}
impl DType for f64 {
	fn id() -> TypeID 	{TypeID::F64}
	
	fn from_dfield(field: &Field<Self>, data: &[u8]) -> Self {
		assert_eq!(field.bit, 0, "bit aligned floats are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		Self::from_le_bytes(data[field.byte..field.byte+std::mem::size_of::<Self>()].try_into().expect("wrong data size"))
	}
	fn to_dfield(&self, field: &Field<Self>, data: &mut [u8]) {
		assert_eq!(field.bit, 0, "bit aligned floats are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		data[field.byte..field.byte+std::mem::size_of::<Self>()].copy_from_slice(&self.to_le_bytes());
	}
}
impl DType for u32 {
	fn id() -> TypeID 	{TypeID::U32}
	
	fn from_dfield(field: &Field<Self>, data: &[u8]) -> Self {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		Self::from_le_bytes(data[field.byte..field.byte+std::mem::size_of::<Self>()].try_into().expect("wrong data size"))
	}
	fn to_dfield(&self, field: &Field<Self>, data: &mut [u8]) {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		data[field.byte..field.byte+std::mem::size_of::<Self>()].copy_from_slice(&self.to_le_bytes());
	}
}
impl DType for u16 {
	fn id() -> TypeID 	{TypeID::U16}
	
	fn from_dfield(field: &Field<Self>, data: &[u8]) -> Self {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		Self::from_le_bytes(data[field.byte..field.byte+std::mem::size_of::<Self>()].try_into().expect("wrong data size"))
	}
	fn to_dfield(&self, field: &Field<Self>, data: &mut [u8]) {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		data[field.byte..field.byte+std::mem::size_of::<Self>()].copy_from_slice(&self.to_le_bytes());
	}
}
impl DType for u8 {
	fn id() -> TypeID 	{TypeID::U8}
	
	fn from_dfield(field: &Field<Self>, data: &[u8]) -> Self {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		Self::from_le_bytes(data[field.byte..field.byte+std::mem::size_of::<Self>()].try_into().expect("wrong data size"))
	}
	fn to_dfield(&self, field: &Field<Self>, data: &mut [u8]) {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		data[field.byte..field.byte+std::mem::size_of::<Self>()].copy_from_slice(&self.to_le_bytes());
	}
}
impl DType for i32 {
	fn id() -> TypeID 	{TypeID::I32}
	
	fn from_dfield(field: &Field<Self>, data: &[u8]) -> Self {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		Self::from_le_bytes(data[field.byte..field.byte+std::mem::size_of::<Self>()].try_into().expect("wrong data size"))
	}
	fn to_dfield(&self, field: &Field<Self>, data: &mut [u8]) {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		data[field.byte..field.byte+std::mem::size_of::<Self>()].copy_from_slice(&self.to_le_bytes());
	}
}
impl DType for i16 {
	fn id() -> TypeID 	{TypeID::I16}
	
	fn from_dfield(field: &Field<Self>, data: &[u8]) -> Self {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		Self::from_le_bytes(data[field.byte..field.byte+std::mem::size_of::<Self>()].try_into().expect("wrong data size"))
	}
	fn to_dfield(&self, field: &Field<Self>, data: &mut [u8]) {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		data[field.byte..field.byte+std::mem::size_of::<Self>()].copy_from_slice(&self.to_le_bytes());
	}
}
impl DType for i8 {
	fn id() -> TypeID 	{TypeID::I8}
	
	fn from_dfield(field: &Field<Self>, data: &[u8]) -> Self {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		Self::from_le_bytes(data[field.byte..field.byte+std::mem::size_of::<Self>()].try_into().expect("wrong data size"))
	}
	fn to_dfield(&self, field: &Field<Self>, data: &mut [u8]) {
		assert_eq!(field.bit, 0, "bit aligned integers are not supported");
		assert_eq!(field.bitlen, 8*std::mem::size_of::<Self>(), "wrong field size");
		data[field.byte..field.byte+std::mem::size_of::<Self>()].copy_from_slice(&self.to_le_bytes());
	}
}

// TODO: use crate `packing`

// use packing::Packed;

// impl<T: Packed> DType for T {
// 	fn id() -> TypeID   {TypeID::CUSTOM}
// 	
// 	fn from_dfield(field: &Field<Self>, data: &[u8]) -> Self {
// 		assert_eq!(field.bit, 0, "bit alignment for packed structs are not supported");
// 		Self::unpack(data[field.byte as usize .. (field.bitlen/8) as usize]).unwrap()
// 	}
// 	fn to_dfield(&self, field: &Field<Self>, data: &mut [u8]) {
// 		assert_eq!(field.bit, 0, "bit alignment for packed structs are not supported");
// 		self.pack(data[field.byte as usize .. (field.bitlen/8) as usize]).unwrap()
// 	}
// }
