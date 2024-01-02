use ethercrab_wire::EtherCatWire;

#[test]
fn sync_manager_channel() {
    #[derive(Default, Copy, Clone, Debug, PartialEq, Eq, EtherCatWire)]
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

    #[derive(Default, Copy, Clone, Debug, PartialEq, Eq, EtherCatWire)]
    #[wire(bits = 2)]
    #[repr(u8)]
    pub enum OperationMode {
        #[default]
        Normal = 0x00,
        Mailbox = 0x02,
    }

    #[derive(Default, Copy, Clone, Debug, PartialEq, Eq, EtherCatWire)]
    #[wire(bits = 2)]
    #[repr(u8)]
    pub enum Direction {
        #[default]
        MasterRead = 0x00,
        MasterWrite = 0x01,
    }
}
