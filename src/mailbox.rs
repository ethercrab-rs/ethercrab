bitflags::bitflags! {
    /// Supported mailbox category.
    ///
    /// Defined in ETG1000.6 Table 18.
    pub struct MailboxProtocols: u16 {
        /// ADS over EtherCAT (routing and parallel services).
        const AOE = 0x0001;
        /// Ethernet over EtherCAT (tunnelling of Data Link services).
        const EOE = 0x0002;
        /// CAN application protocol over EtherCAT (access to SDO).
        const COE = 0x0004;
        /// File Access over EtherCAT.
        const FOE = 0x0008;
        /// Servo Drive Profile over EtherCAT.
        const SOE = 0x0010;
        /// Vendor specific protocol over EtherCAT.
        const VOE = 0x0020;
    }
}
