use crate::{
	SlaveRef, SubIndex, PduData,
	error::Error,
	};
use super::field::{Field, DType};

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
	pub async fn get<'a>(&self, slave: &SlaveRef<'a>) -> Result<T, Error>  {
		slave.read_sdo(self.index, SubIndex::Index(self.sub)).await
	}
	/// set the subitem value on the given slave
	pub async fn set<'a>(&self, slave: &SlaveRef<'a>, value: T) -> Result<(), Error>   {
		slave.write_sdo(self.index, SubIndex::Index(self.sub), value).await?;
		Ok(())
	}
}

/// description of SDO configuring a PDO
/// the SDO is assumed to follow the cia402 specifications for PDO SDOs
#[derive(Clone)]
pub struct ConfigurablePdo {
	pub index: u16,
	pub num: u8,
}

/// description of SDO configuring a SyncManager
/// the SDO is assumed to follow the cia402 specifications for syncmanager SDOs
#[derive(Clone)]
pub struct SyncManager {
	pub index: u16,
	pub num: u8,
}



/// configure a sync manager and select the pdos it is using
pub struct SyncMapping<'a> {
	slave: &'a SlaveRef<'a>,
	manager: &'a SyncManager,
	offset: usize,
	num: u8,
}
// unsafe impl<'a> Send for SyncMapping<'a> {}
// unsafe impl<'a> Send for *mut SyncMapping<'a> {}
impl<'a> SyncMapping<'a> {
	pub async fn new(slave: &'a SlaveRef<'a>, manager: &'a SyncManager) -> Result<SyncMapping<'a>, Error> {
		let new = Self {  
			slave, 
			manager, 
			offset: 0,
			num: 0,
			};
		new.slave.write_sdo(new.manager.index, SubIndex::Index(0), new.num).await?;
		Ok(new)
	}
	/// set the given pdo to be transmitted
	pub async fn push<'b>(&'b mut self, pdo: &'b ConfigurablePdo) -> Result<PdoMapping<'b>, Error>
	where 'a: 'b {
		assert!(self.num < self.manager.num, "maximum number of pdos reached");
		log::debug!("push  {:x} {}: {:x}", self.manager.index, self.num+1, pdo.index);
		self.slave.write_sdo(self.manager.index, SubIndex::Index(self.num+1), pdo.index).await?;
		self.num += 1;
		
		// this savage cast is here to prevent rust asking 'b to outlive 'a, which is non-sense
		// the cast is in a separate function because the compiler seems to keep the pointer value alive during the following await, out of the unsafe block, which cause the async function to be send
		fn cast<'a, 'b>(value: &'b mut SyncMapping<'a>) -> &'b mut SyncMapping<'b>
		where 'a: 'b {
			unsafe {
				&mut *std::mem::transmute::<
							*mut SyncMapping<'a>, 
							*mut SyncMapping<'b>,
							>(value)
		}}
		Ok(PdoMapping::new(cast(self), pdo).await?) 
	}
	/// finalize the mapping configuration
	/// must be called or the sync manager will not transmit
	pub async fn finish(&mut self) -> Result<(), Error> {
		log::debug!("finish  {:x} {}: {:x}", self.manager.index, 0, self.num);
		self.slave.write_sdo(self.manager.index, SubIndex::Index(0), self.num).await?;
		Ok(())
	}
}


/// configure a pdo and select the sdos subitems it is using
pub struct PdoMapping<'b> {
	mapping: &'b mut SyncMapping<'b>,
	pdo: &'b ConfigurablePdo,
	num: u8,
}
impl<'b> PdoMapping<'b> {
	async fn new(mapping: &'b mut SyncMapping<'b>, pdo: &'b ConfigurablePdo) -> Result<PdoMapping<'b>, Error> {
		let new = Self {mapping, pdo, num: 0};
		new.mapping.slave.write_sdo(new.pdo.index, SubIndex::Index(0), new.num).await?;
		Ok(new)
	}
	/// set the given sdo item to be transmitted
	pub async fn push<T: PduData + DType>(&mut self, sdo: &SubItem<T>) -> Result<Field<T>, Error> {
		assert!(self.num < self.pdo.num, "maximum number of pdo entries reached");
		log::debug!("  push {:x} {}: {:x}", self.pdo.index, self.num+1, ((sdo.index as u32) << 16) | ((sdo.sub as u32) << 4) | ((sdo.field.bitlen as u32) & 0xff));
		
		self.mapping.slave.write_sdo::<u32>(
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
			self.mapping.slave.read_sdo::<u32>(
				self.pdo.index, 
				SubIndex::Index(self.num+1),
				).await?, 
			((sdo.index as u32) << 16) | ((sdo.sub as u32) << 8) | ((sdo.field.bitlen as u32) & 0xff),
			"wrong sdo present in pdo entry",
			);
		
		let result = Field::new(self.mapping.offset.into(), 0, sdo.field.bitlen);
		self.mapping.offset += (sdo.field.bitlen+1)/8;
		self.num += 1;
		Ok(result)
	}
	/// finalize the pdo configuration
	/// must be called or the pdo will contain nothing
	pub async fn finish(&mut self) -> Result<(), Error> {
		log::debug!("  finish {:x} {}: {:x}", self.pdo.index, 0, self.num);
		self.mapping.slave.write_sdo(self.pdo.index, SubIndex::Index(0), self.num).await?;
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
