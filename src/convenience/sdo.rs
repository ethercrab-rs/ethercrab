use crate::{
	Slave, SlaveRef, SubIndex, PduData,
	error::Error,
	};
use super::field::{Field, DType};
use std::{fmt, borrow::Borrow};

/// description of an SDO's subitem, not a SDO itself
#[derive(Clone)]
pub struct SubItem<T: PduData + DType> {
	/// index of the item in the slave's dictionnary of objects
	pub index: u16,
	/// subindex in the item
	pub sub: u8,
	/// field pointing to the subitem in the byte sequence of the complete SDO
	/// TODO: see if this is really usefull/mendatory
	pub field: Field<T>,
}
impl<T: PduData + DType> SubItem<T> {
	/// retreive the current subitem value from the given slave
	pub async fn get<'a, S: Borrow<Slave>>(&self, slave: &SlaveRef<'a, S>) -> Result<T, Error>  {
		slave.sdo_read(self.index, SubIndex::Index(self.sub)).await
	}
	/// set the subitem value on the given slave
	pub async fn set<'a, S: Borrow<Slave>>(&self, slave: &SlaveRef<'a, S>, value: T) -> Result<(), Error>   {
		slave.sdo_write(self.index, SubIndex::Index(self.sub), value).await?;
		Ok(())
	}
}
impl<T: PduData + DType> fmt::Debug for SubItem<T> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "SubItem {{index: {:x}, sub: {}, field: {:?}}}", self.index, self.sub, self.field)
	}
}

/// description of SDO configuring a PDO
/// the SDO is assumed to follow the cia402 specifications for PDO SDOs
#[derive(Clone)]
pub struct ConfigurablePdo {
	pub index: u16,
	pub num: u8,
}
impl fmt::Debug for ConfigurablePdo {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "ConfigurablePdo {{index: {:x}, num: {}}}", self.index, self.num)
	}
}

/// description of SDO configuring a SyncManager
/// the SDO is assumed to follow the cia402 specifications for syncmanager SDOs
#[derive(Clone)]
pub struct SyncManager {
	pub index: u16,
	pub num: u8,
}
impl fmt::Debug for SyncManager {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "ConfigurablePdo {{index: {:x}, num: {}}}", self.index, self.num)
	}
}



/// configure a sync manager and select the pdos it is using
pub struct SyncMapping<'a, S> {
	slave: &'a SlaveRef<'a, S>,
	manager: &'a SyncManager,
	offset: usize,
	num: u8,
}
// unsafe impl<'a> Send for SyncMapping<'a> {}
// unsafe impl<'a> Send for *mut SyncMapping<'a> {}
impl<'a, S: Borrow<Slave>> SyncMapping<'a, S> {
	pub async fn new(slave: &'a SlaveRef<'a, S>, manager: &'a SyncManager) -> Result<SyncMapping<'a, S>, Error> {
		let new = Self {  
			slave, 
			manager, 
			offset: 0,
			num: 0,
			};
		new.slave.sdo_write(new.manager.index, SubIndex::Index(0), new.num).await?;
		Ok(new)
	}
	/// set the given pdo to be transmitted
	pub async fn push<'b>(&'b mut self, pdo: &'b ConfigurablePdo) -> Result<PdoMapping<'b, S>, Error>
	where 'a: 'b {
		assert!(self.num < self.manager.num, "maximum number of pdos reached");
		log::debug!("push  {:x} {}: {:x}", self.manager.index, self.num+1, pdo.index);
		self.slave.sdo_write(self.manager.index, SubIndex::Index(self.num+1), pdo.index).await?;
		self.num += 1;
		
		// this savage cast is here to prevent rust asking 'b to outlive 'a, which is non-sense
		// the cast is in a separate function because the compiler seems to keep the pointer value alive during the following await, out of the unsafe block, which cause the async function to be send
		fn cast<'a, 'b, S>(value: &'b mut SyncMapping<'a, S>) -> &'b mut SyncMapping<'b, S>
		where 'a: 'b {
			unsafe {
				&mut *std::mem::transmute::<
							*mut SyncMapping<'a, S>, 
							*mut SyncMapping<'b, S>,
							>(value)
		}}
		Ok(PdoMapping::new(cast(self), pdo).await?) 
	}
	/// finalize the mapping configuration
	/// must be called or the sync manager will not transmit
	pub async fn finish(&mut self) -> Result<(), Error> {
		log::debug!("finish  {:x} {}: {:x}", self.manager.index, 0, self.num);
		self.slave.sdo_write(self.manager.index, SubIndex::Index(0), self.num).await?;
		Ok(())
	}
}


/// configure a pdo and select the sdos subitems it is using
pub struct PdoMapping<'b, S> {
	mapping: &'b mut SyncMapping<'b, S>,
	pdo: &'b ConfigurablePdo,
	num: u8,
}
impl<'b, S: Borrow<Slave>> PdoMapping<'b, S> {
	async fn new(mapping: &'b mut SyncMapping<'b, S>, pdo: &'b ConfigurablePdo) -> Result<PdoMapping<'b, S>, Error> {
		let new = Self {mapping, pdo, num: 0};
		new.mapping.slave.sdo_write(new.pdo.index, SubIndex::Index(0), new.num).await?;
		Ok(new)
	}
	/// set the given sdo item to be transmitted
	pub async fn push<T: PduData + DType>(&mut self, sdo: &SubItem<T>) -> Result<Field<T>, Error> {
		assert!(self.num < self.pdo.num, "maximum number of pdo entries reached");
		log::debug!("  push {:x} {}: {:x}", self.pdo.index, self.num+1, ((sdo.index as u32) << 16) | ((sdo.sub as u32) << 4) | ((sdo.field.bitlen as u32) & 0xff));
		
		self.mapping.slave.sdo_write::<u32>(
			self.pdo.index, 
			SubIndex::Index(self.num+1), 
			((sdo.index as u32) << 16) | ((sdo.sub as u32) << 8) | ((sdo.field.bitlen as u32) & 0xff),
			).await?;
		let result = Field::new(self.mapping.offset.into(), 0, sdo.field.bitlen);
		self.mapping.offset += (sdo.field.bitlen+1)/8;
		self.num += 1;
		Ok(result)
	}
	pub async fn count<T: PduData + DType>(&mut self, sdo: &SubItem<T>) -> Result<Field<T>, Error> {
		assert!(self.num < self.pdo.num, "maximum number of pdo entries reached");
		assert_eq!(
			self.mapping.slave.sdo_read::<u32>(
				self.pdo.index, 
				SubIndex::Index(self.num+1),
				).await?, 
			((sdo.index as u32) << 16) | ((sdo.sub as u32) << 8) | ((sdo.field.bitlen as u32) & 0xff),
			"wrong sdo present in pdo entry",
			);
		
		let result = Field::new(self.mapping.offset.into(), 0, sdo.field.bitlen);
		self.mapping.offset += (sdo.field.bitlen+7)/8;  // round value to ceil
		self.num += 1;
		Ok(result)
	}
	/// finalize the pdo configuration
	/// must be called or the pdo will contain nothing
	pub async fn finish(&mut self) -> Result<(), Error> {
		log::debug!("  finish {:x} {}: {:x}", self.pdo.index, 0, self.num);
		self.mapping.slave.sdo_write(self.pdo.index, SubIndex::Index(0), self.num).await?;
		Ok(())
	}
}




// TODO:  when loading an ESI xml file, add a method to these helper structs from corresponding nodes in the xml document
// impl FromESI for SubItem {
// 	fn from_xml(xml::Node) -> Self {}
// }

// impl FromESI impl SyncManager {
// 	fn from_xml(xml::Node) -> Self {}
// }

// impl FromESI impl ConfigurablePdo {
// 	fn from_xml(xml::Node) -> Self {}
// }
