/*! 
Convenient structures to read/write the slave's dictionnary objects (SDO) and configure mappings.

# Example of PDO mapping

	// typical mapping
	// the SDOs are declared first somewhere
	let rx = SyncManager {index: 0x1c12, num: 3};
	let pdo = ConfigurablePdo {index: 0x1600, num: 3};
	let target_controlword = SubItem::<ControlWord> {index: 0x6040, sub: 0, field: Field::new(0, 2)};
	let target_position = SubItem::<i32> {index: 0x607a, sub: 0, field: Field::new(0, 4)};
	
	// the mapping is done at program start
	let mut sm = SyncMapping::new(slave, &rx).await?;
	let mut pdo = sm.push(&pdo).await?;
	let offset_controlword = pdo.push(&target_controlword).await?;
	let offset_position = pdo.push(&target_position).await?;
	pdo.finish().await?;
	sm.finish().await?;
	
	// typical use latter in the program
	offset_controlword.set(slave.outputs(), ...);
*/

use crate::{
	Slave, SlaveRef, SubIndex,
	error::Error,
	pdu_data::{Field, PduData, ByteArray},
	};
use core::{fmt, borrow::Borrow};


/// description of an SDO's subitem, not a SDO itself
#[derive(Clone)]
pub struct SubItem<T: PduData> {
	/// index of the item in the slave's dictionnary of objects
	pub index: u16,
	/// subindex in the item
	pub sub: u8,
	/// field pointing to the subitem in the byte sequence of the complete SDO
	// TODO: see if this is really usefull/mendatory, since [ByteArray::len] already provides half of the field and its offset is rarely used
	pub field: Field<T>,
}
impl<T: PduData> SubItem<T> {
	/// create a subitem, deducing its size from the `PduData` impl
	pub fn new(index: u16, sub: u8, offset: usize) -> Self { Self{
		index,
		sub,
		field: Field::new(offset, T::ByteArray::len()),
	}}
	/// create a subitem at the given index, with `sub=0` and `byte=0`
	pub fn complete(index: u16) -> Self { Self{ 
		index, 
		sub: 0, 
		field: Field::new(0, T::ByteArray::len()),
	}}
	/// retreive the current subitem value from the given slave
	pub async fn get<'a, S: Borrow<Slave>>(&self, slave: &SlaveRef<'a, S>) -> Result<T, Error>  {
		slave.sdo_read(self.index, self.sub).await
	}
	/// set the subitem value on the given slave
	pub async fn set<'a, S: Borrow<Slave>>(&self, slave: &SlaveRef<'a, S>, value: T) -> Result<(), Error>   {
		slave.sdo_write(self.index, self.sub, value).await?;
		Ok(())
	}
}
impl<T: PduData> fmt::Debug for SubItem<T> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "SubItem {{index: {:x}, sub: {}, field: {:?}}}", self.index, self.sub, self.field)
	}
}

/// description of SDO configuring a PDO
/// the SDO is assumed to follow the cia402 specifications for PDO SDOs
#[derive(Clone)]
pub struct ConfigurablePdo {
	/// index of the SDO that configures the PDO
	pub index: u16,
	/// number of entries in the PDO
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
	/// index of the SDO that configures the SyncManager
	pub index: u16,
	/// max number of PDO that can be assigned to the SyncManager
	pub num: u8,
}
impl fmt::Debug for SyncManager {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "ConfigurablePdo {{index: {:x}, num: {}}}", self.index, self.num)
	}
}



/// configure a sync manager and select the pdos it is using
pub struct SyncMapping<'a, S> {
	/// slave for communication
	slave: &'a SlaveRef<'a, S>,
	/// SDO informations
	manager: &'a SyncManager,
	/// state of the mapping
	offset: usize,
	num: u8,
}

impl<'a, S: Borrow<Slave>> SyncMapping<'a, S> {
	/// clear and start the mapping of slave PDOs
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
	///
	/// must be called or the sync manager will not transmit
	pub async fn finish(&mut self) -> Result<(), Error> {
		log::debug!("finish sm {:x} {}: {:x}", self.manager.index, 0, self.num);
		self.slave.sdo_write(self.manager.index, SubIndex::Index(0), self.num).await?;
		Ok(())
	}
}


/// configure a pdo and select the sdos subitems it is using
pub struct PdoMapping<'b, S> {
	/// the mapping in modfication
	mapping: &'b mut SyncMapping<'b, S>,
	/// SDO informations
	pdo: &'b ConfigurablePdo,
	/// state of mapping
	num: u8,
}
impl<'b, S: Borrow<Slave>> PdoMapping<'b, S> {
	async fn new(mapping: &'b mut SyncMapping<'b, S>, pdo: &'b ConfigurablePdo) -> Result<PdoMapping<'b, S>, Error> {
		let new = Self {mapping, pdo, num: 0};
		new.mapping.slave.sdo_write(new.pdo.index, SubIndex::Index(0), new.num).await?;
		Ok(new)
	}
	/// set the next entry in the PDO with the given SDO
	///
	/// the given SDO will then be transmitted in PDU at the returned offset
	pub async fn push<T: PduData>(&mut self, sdo: &SubItem<T>) -> Result<Field<T>, Error> {
		assert!(self.num < self.pdo.num, "maximum number of pdo entries reached");
		
		let entry = ((sdo.index as u32) << 16) | ((sdo.sub as u32) << 8) | (((sdo.field.len * 8) as u32) & 0xff);
		log::debug!("  push {:x} {}: {:x}", self.pdo.index, self.num+1, entry);
		
		self.mapping.slave.sdo_write::<u32>(
			self.pdo.index, 
			SubIndex::Index(self.num+1), 
			entry,
			).await?;
		let result = Field::new(self.mapping.offset.into(), sdo.field.len);
		self.mapping.offset += sdo.field.len;
		self.num += 1;
		Ok(result)
	}
	/// check that the next entry is the given SDO, and else panick
	///
	/// this is useful to check that entries in a fixed PDO (or already set PDO) are correct
	pub async fn count<T: PduData>(&mut self, sdo: &SubItem<T>) -> Result<Field<T>, Error> {
		let entry = ((sdo.index as u32) << 16) | ((sdo.sub as u32) << 8) | (((sdo.field.len * 8) as u32) & 0xff);
		log::debug!("  check {:x} {}: {:x}", self.pdo.index, self.num+1, entry);
	
		assert!(self.num < self.pdo.num, "maximum number of pdo entries reached");
		assert_eq!(
			self.mapping.slave.sdo_read::<u32>(
				self.pdo.index, 
				SubIndex::Index(self.num+1),
				).await?, 
			entry,
			"wrong sdo present in pdo entry",
			);
		
		let result = Field::new(self.mapping.offset.into(), sdo.field.len);
		self.mapping.offset += sdo.field.len;  // round value to ceil
		self.num += 1;
		Ok(result)
	}
	/// pass the next PDO entry, but reads the slave mapping to count its size anyway
	///
	/// this is useful for fixed PDOs where some entries are useless but need to be accounted in offsets
	pub async fn pass(&mut self) -> Result<(), Error> {
		assert!(self.num < self.pdo.num, "maximum number of pdo entries reached");
		
		let entry = self.mapping.slave.sdo_read::<u32>(
				self.pdo.index,
				SubIndex::Index(self.num+1),
				).await?;
		let len = (((entry & 0xff) +7) / 8) as usize;
		self.mapping.offset += len;
		self.num += 1;
		Ok(())
	}
	/// finalize the pdo configuration
	/// must be called or the pdo will contain nothing
	pub async fn finish(&mut self) -> Result<(), Error> {
		log::debug!("finish pdo {} {}", self.num, self.mapping.offset);
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
