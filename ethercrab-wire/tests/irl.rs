use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireReadWrite};

#[test]
fn sync_manager_channel() {
    #[derive(Default, Copy, Clone, Debug, PartialEq, Eq, EtherCrabWireReadWrite)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    #[wire(bits = 8)]
    pub struct Control {
        #[wire(bits = 2)]
        pub operation_mode: OperationMode,
        #[wire(bits = 2)]
        pub direction: Direction,
        #[wire(bits = 1)]
        pub ecat_event_enable: bool,
        #[wire(bits = 1)]
        pub dls_user_event_enable: bool,
        #[wire(bits = 1, post_skip = 1)]
        pub watchdog_enable: bool,
    }

    #[derive(Default, Copy, Clone, Debug, PartialEq, Eq, EtherCrabWireReadWrite)]
    #[wire(bits = 2)]
    #[repr(u8)]
    pub enum OperationMode {
        #[default]
        Normal = 0x00,
        Mailbox = 0x02,
    }

    #[derive(Default, Copy, Clone, Debug, PartialEq, Eq, EtherCrabWireReadWrite)]
    #[wire(bits = 2)]
    #[repr(u8)]
    pub enum Direction {
        #[default]
        MasterRead = 0x00,
        MasterWrite = 0x01,
    }
}

#[test]
fn pdo() {
    #[derive(Clone, PartialEq, ethercrab_wire::EtherCrabWireReadWrite)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    #[wire(bytes = 6)]
    pub struct Pdo {
        #[wire(bytes = 2)]
        pub(crate) index: u16,
        #[wire(bytes = 1)]
        pub(crate) num_entries: u8,
        #[wire(bytes = 1)]
        pub(crate) sync_manager: u8,
        #[wire(bytes = 1)]
        pub(crate) dc_sync: u8,
        /// Index into EEPROM Strings section for PDO name.
        #[wire(bytes = 1)]
        pub(crate) name_string_idx: u8,

        // NOTE: This field is skipped during parsing from the wire and is populated later.
        #[wire(skip)]
        pub(crate) entries: heapless::Vec<u8, 16>,
    }
}

#[test]
fn slave_state() {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, EtherCrabWireReadWrite)]
    #[repr(u8)]
    pub enum SlaveState {
        /// No state recorded/read/known.
        None = 0x00,
        /// EtherCAT `INIT` state.
        Init = 0x01,
        /// EtherCAT `PRE-OP` state.
        PreOp = 0x02,
        /// EtherCAT `BOOT` state.
        Bootstrap = 0x03,
        /// EtherCAT `SAFE-OP` state.
        SafeOp = 0x04,
        /// EtherCAT `OP` state.
        Op = 0x8,
        /// State is a combination of above variants or is an unknown value.
        #[wire(catch_all)]
        Other(u8),
    }
}

#[test]
fn bare_enum() {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, EtherCrabWireReadWrite)]
    #[repr(u8)]
    pub enum SlaveState {
        /// No state recorded/read/known.
        None = 0x00,
        /// EtherCAT `INIT` state.
        Init = 0x01,
        /// EtherCAT `PRE-OP` state.
        PreOp = 0x02,
        /// EtherCAT `BOOT` state.
        Bootstrap = 0x03,
        /// EtherCAT `SAFE-OP` state.
        SafeOp = 0x04,
        /// EtherCAT `OP` state.
        Op = 0x8,
    }
}

#[test]
fn arr() {
    #[derive(Default, Copy, Clone, Debug, PartialEq, Eq, EtherCrabWireReadWrite)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    #[wire(bytes = 4)]
    pub struct Control {
        #[wire(bytes = 4)]
        pub data: [u8; 4],
    }
}

#[test]
fn enum_u16() {
    #[derive(Debug, Copy, Clone, ethercrab_wire::EtherCrabWireReadWrite)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    #[repr(u16)]
    pub enum AlStatusCode {
        NoError = 0x0000,
        #[wire(catch_all)]
        Unknown(u16),
    }
}

#[test]
fn enum_alternatives() {
    #[derive(
        Default, Debug, Copy, Clone, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite,
    )]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    #[repr(u16)]
    pub enum Alternatives {
        #[default]
        Nop = 0,
        #[wire(alternatives = [2,3,4,5,6,7,8,9])]
        DeviceSpecific = 1,
    }
}

#[test]
fn enum_default_and_catch_all() {
    #[derive(Default, Debug, Copy, Clone, ethercrab_wire::EtherCrabWireReadWrite)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    #[repr(u16)]
    pub enum AlStatusCode {
        NoError = 0x0000,
        #[default]
        Something = 0x0001,
        #[wire(catch_all)]
        Unknown(u16),
    }
}

#[test]
fn enum_default_only() {
    #[derive(Default, Debug, Copy, Clone, ethercrab_wire::EtherCrabWireReadWrite)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    #[repr(u16)]
    pub enum AlStatusCode {
        NoError = 0x0000,
        #[default]
        Something = 0x0001,
    }
}

#[test]
fn heapless_vec() {
    #[derive(
        Default, Debug, Copy, Clone, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite,
    )]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    #[repr(u8)]
    pub enum SyncManagerType {
        /// Not used or unknown.
        #[default]
        Unknown = 0x00,
        /// Used for writing into the slave.
        MailboxWrite = 0x01,
        /// Used for reading from the slave.
        MailboxRead = 0x02,
        /// Used for process data outputs from master.
        ProcessDataWrite = 0x03,
        /// Used for process data inputs to master.
        ProcessDataRead = 0x04,
    }

    let data = [0x00u8, 0x04u8, 0x02u8];

    let result = heapless::Vec::<SyncManagerType, 16>::unpack_from_slice(&data);

    assert_eq!(
        result,
        Ok(heapless::Vec::try_from(
            [
                SyncManagerType::Unknown,
                SyncManagerType::ProcessDataRead,
                SyncManagerType::MailboxRead,
            ]
            .as_ref()
        )
        .unwrap())
    )
}
