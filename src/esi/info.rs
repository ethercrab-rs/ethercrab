use yaserde::{YaDeserialize, YaSerialize};

//use EtherCATBase.xsd  ;
#[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
pub struct EtherCATInfo {
    #[yaserde(rename = "InfoReference")]
    pub info_reference: Vec<String>,

    #[yaserde(rename = "Vendor")]
    pub vendor: ether_cat_info::VendorType,

    #[yaserde(rename = "Descriptions")]
    pub descriptions: ether_cat_info::DescriptionsType,

    #[yaserde(attribute = true, rename = "Version")]
    pub version: Option<String>,
}

pub mod ether_cat_info {
    use super::*;

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct VendorType {
        #[yaserde(attribute = true, rename = "FileVersion")]
        pub file_version: Option<i32>,
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct DescriptionsType {
        #[yaserde(rename = "Groups")]
        pub groups: descriptions_type::GroupsType,

        #[yaserde(rename = "Devices")]
        pub devices: descriptions_type::DevicesType,

        #[yaserde(rename = "Modules")]
        pub modules: Option<descriptions_type::ModulesType>,
    }

    pub mod descriptions_type {
        use super::*;

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct GroupsType {
            #[yaserde(rename = "Group")]
            pub group: Vec<groups_type::GroupType>,
        }

        pub mod groups_type {
            use crate::esi::String;

            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct GroupType {
                #[yaserde(attribute = true, rename = "SortOrder")]
                pub sort_order: Option<i32>,

                #[yaserde(attribute = true, rename = "ParentGroup")]
                pub parent_group: Option<String>,

                #[yaserde(rename = "Type")]
                pub _type: String,

                #[yaserde(rename = "Name")]
                pub name: Vec<String>,

                #[yaserde(rename = "Comment")]
                pub comment: Vec<String>,

                #[yaserde(rename = "GroupTypeChoice")]
                pub group_type_choice: group_type::GroupTypeChoice,

                #[yaserde(rename = "VendorSpecific")]
                pub vendor_specific: Option<String>,
            }

            pub mod group_type {
                use super::*;

                #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                pub enum GroupTypeChoice {
                    // obsolete
                    #[yaserde(rename = "Image16x14")]
                    Image16X14(Option<String>),
                    #[yaserde(rename = "ImageFile16x14")]
                    ImageFile16X14(Option<String>),
                    #[yaserde(rename = "ImageData16x14")]
                    ImageData16X14(Option<String>),
                    __Unknown__(String),
                }

                impl Default for GroupTypeChoice {
                    fn default() -> GroupTypeChoice {
                        Self::__Unknown__("No valid variants".into())
                    }
                }
            }
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct DevicesType {
            #[yaserde(rename = "Device")]
            pub device: Vec<devices_type::DeviceType>,
        }

        pub mod devices_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct DeviceType {
                #[yaserde(attribute = true, rename = "Invisible")]
                pub invisible: Option<bool>,

                #[yaserde(attribute = true, rename = "Physics")]
                pub physics: String,

                #[yaserde(attribute = true, rename = "Crc32")]
                pub crc_32: Option<String>,

                // (SCI)
                #[yaserde(rename = "Sci")]
                pub sci: Option<device_type::SciType>,

                #[yaserde(rename = "Type")]
                pub _type: device_type::TypeType,

                #[yaserde(rename = "HideType")]
                pub hide_type: Vec<device_type::HideTypeType>,

                #[yaserde(rename = "AlternativeType")]
                pub alternative_type: Vec<device_type::AlternativeTypeType>,

                #[yaserde(rename = "SubDevice")]
                pub sub_device: Vec<device_type::SubDeviceType>,

                #[yaserde(rename = "Name")]
                pub name: Vec<String>,

                #[yaserde(rename = "Comment")]
                pub comment: Vec<String>,

                #[yaserde(rename = "URL")]
                pub url: Vec<String>,

                #[yaserde(rename = "Info")]
                pub info: Option<InfoType>,

                #[yaserde(rename = "GroupType")]
                pub group_type: String,

                #[yaserde(rename = "Profile")]
                pub profile: Vec<device_type::ProfileType>,

                #[yaserde(rename = "Fmmu")]
                pub fmmu: Vec<device_type::FmmuType>,

                #[yaserde(rename = "Sm")]
                pub sm: Vec<device_type::SmType>,

                #[yaserde(rename = "Su")]
                pub su: Vec<device_type::SuType>,

                #[yaserde(rename = "RxPdo")]
                pub rx_pdo: Vec<PdoType>,

                #[yaserde(rename = "TxPdo")]
                pub tx_pdo: Vec<PdoType>,

                #[yaserde(rename = "Mailbox")]
                pub mailbox: Option<device_type::MailboxType>,

                #[yaserde(rename = "Dc")]
                pub dc: Option<device_type::DcType>,

                #[yaserde(rename = "Slots")]
                pub slots: Option<device_type::SlotsType>,

                #[yaserde(rename = "ESC")]
                pub esc: Option<device_type::Esctype>,

                #[yaserde(rename = "Eeprom")]
                pub eeprom: Option<device_type::EepromType>,

                #[yaserde(rename = "DeviceTypeChoice")]
                pub device_type_choice: device_type::DeviceTypeChoice,

                #[yaserde(rename = "VendorSpecific")]
                pub vendor_specific: Option<String>,
            }

            pub mod device_type {
                use super::*;

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct SciType {
                    #[yaserde(rename = "Name")]
                    pub name: Vec<String>,

                    #[yaserde(rename = "Description")]
                    pub description: Vec<String>,

                    #[yaserde(rename = "Guid")]
                    pub guid: Guid,

                    #[yaserde(rename = "CreatedBy")]
                    pub created_by: sci_type::CreatedByType,

                    #[yaserde(rename = "TargetSpecific")]
                    pub target_specific: Option<sci_type::TargetSpecificType>,

                    #[yaserde(rename = "VendorSpecific")]
                    pub vendor_specific: Option<String>,

                    // Version of ETG.2000 on which this SCI is based on
                    #[yaserde(attribute = true, rename = "SciVersion")]
                    pub sci_version: Option<String>,
                }

                pub mod sci_type {
                    use super::*;

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct CreatedByType {
                        #[yaserde(rename = "Company")]
                        pub company: String,

                        #[yaserde(rename = "VendorId")]
                        pub vendor_id: Option<String>,

                        #[yaserde(rename = "Tool")]
                        pub tool: Option<created_by_type::ToolType>,
                    }

                    pub mod created_by_type {
                        use super::*;

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct ToolType {
                            #[yaserde(attribute = true, rename = "Version")]
                            pub version: Option<String>,

                            pub base: String,
                        }
                    }

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct TargetSpecificType {
                        #[yaserde(rename = "AoeNetId")]
                        pub aoe_net_id: Option<target_specific_type::AoeNetIdType>,

                        #[yaserde(rename = "EoeMacIp")]
                        pub eoe_mac_ip: Option<target_specific_type::EoeMacIpType>,

                        #[yaserde(rename = "DcCycleTime")]
                        pub dc_cycle_time: Option<target_specific_type::DcCycleTimeType>,

                        #[yaserde(rename = "ModuleIdents")]
                        pub module_idents: Option<target_specific_type::ModuleIdentsType>,
                    }

                    pub mod target_specific_type {
                        use super::*;

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct AoeNetIdType {
                            #[yaserde(rename = "Comment")]
                            pub comment: Option<String>,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct EoeMacIpType {
                            #[yaserde(rename = "Comment")]
                            pub comment: Option<String>,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct DcCycleTimeType {
                            #[yaserde(rename = "Comment")]
                            pub comment: Option<String>,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct ModuleIdentsType {
                            #[yaserde(rename = "ModuleIdent")]
                            pub module_ident: Vec<module_idents_type::ModuleIdentType>,
                        }

                        pub mod module_idents_type {
                            use super::*;

                            #[derive(
                                Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize,
                            )]
                            pub struct ModuleIdentType {
                                #[yaserde(rename = "SlotNo")]
                                pub slot_no: i32,

                                #[yaserde(rename = "Esi")]
                                pub esi: String,

                                #[yaserde(rename = "Sci")]
                                pub sci: String,
                            }
                        }
                    }
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct TypeType {
                    #[yaserde(attribute = true, rename = "ProductCode")]
                    pub product_code: Option<String>,

                    #[yaserde(attribute = true, rename = "RevisionNo")]
                    pub revision_no: Option<String>,

                    #[yaserde(attribute = true, rename = "SerialNo")]
                    pub serial_no: Option<String>,

                    #[yaserde(attribute = true, rename = "CheckProductCode")]
                    pub check_product_code: Option<String>,

                    #[yaserde(attribute = true, rename = "CheckRevisionNo")]
                    pub check_revision_no: Option<String>,

                    #[yaserde(attribute = true, rename = "CheckSerialNo")]
                    pub check_serial_no: Option<String>,

                    // obsolete
                    #[yaserde(attribute = true, rename = "TcSmClass")]
                    pub tc_sm_class: Option<String>,

                    // obsolete
                    #[yaserde(attribute = true, rename = "TcCfgModeSafeOp")]
                    pub tc_cfg_mode_safe_op: Option<bool>,

                    #[yaserde(attribute = true, rename = "UseLrdLwr")]
                    pub use_lrd_lwr: Option<bool>,

                    #[yaserde(attribute = true, rename = "ModulePdoGroup")]
                    pub module_pdo_group: Option<i32>,

                    // obsolete
                    #[yaserde(attribute = true, rename = "DownloadModuleList")]
                    pub download_module_list: Option<bool>,

                    #[yaserde(attribute = true, rename = "ShowHideableSubDevices")]
                    pub show_hideable_sub_devices: Option<bool>,

                    pub base: String,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct HideTypeType {
                    #[yaserde(attribute = true, rename = "ProductCode")]
                    pub product_code: Option<String>,

                    #[yaserde(attribute = true, rename = "RevisionNo")]
                    pub revision_no: Option<String>,

                    // obsolete
                    #[yaserde(attribute = true, rename = "ProductRevision")]
                    pub product_revision: Option<String>,

                    pub base: String,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct AlternativeTypeType {
                    // for future use
                    #[yaserde(attribute = true, rename = "ProductCode")]
                    pub product_code: Option<String>,

                    // for future use
                    #[yaserde(attribute = true, rename = "RevisionNo")]
                    pub revision_no: Option<String>,

                    pub base: String,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct SubDeviceType {
                    // for future use
                    #[yaserde(attribute = true, rename = "ProductCode")]
                    pub product_code: Option<String>,

                    // for future use
                    #[yaserde(attribute = true, rename = "RevisionNo")]
                    pub revision_no: Option<String>,

                    #[yaserde(attribute = true, rename = "PreviousDevice")]
                    pub previous_device: Option<i32>,

                    #[yaserde(attribute = true, rename = "PreviousPortNo")]
                    pub previous_port_no: Option<i32>,

                    #[yaserde(attribute = true, rename = "Hideable")]
                    pub hideable: Option<bool>,

                    pub base: String,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct ProfileType {
                    // obsolete
                    #[yaserde(attribute = true, rename = "Channel")]
                    pub channel: Option<i32>,

                    pub base: ProfileType,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct FmmuType {
                    // obsolete
                    #[yaserde(attribute = true, rename = "OpOnly")]
                    pub op_only: Option<bool>,

                    #[yaserde(attribute = true, rename = "Sm")]
                    pub sm: Option<i32>,

                    #[yaserde(attribute = true, rename = "Su")]
                    pub su: Option<i32>,

                    pub base: String,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct SmType {
                    #[yaserde(attribute = true, rename = "MinSize")]
                    pub min_size: Option<String>,

                    #[yaserde(attribute = true, rename = "MaxSize")]
                    pub max_size: Option<String>,

                    #[yaserde(attribute = true, rename = "DefaultSize")]
                    pub default_size: Option<String>,

                    #[yaserde(attribute = true, rename = "StartAddress")]
                    pub start_address: Option<String>,

                    #[yaserde(attribute = true, rename = "ControlByte")]
                    pub control_byte: Option<String>,

                    #[yaserde(attribute = true, rename = "Enable")]
                    pub enable: Option<String>,

                    // obsolete
                    #[yaserde(attribute = true, rename = "OneByteMode")]
                    pub one_byte_mode: Option<bool>,

                    #[yaserde(attribute = true, rename = "Virtual")]
                    pub _virtual: Option<bool>,

                    // obsolete
                    #[yaserde(attribute = true, rename = "Watchdog")]
                    pub watchdog: Option<bool>,

                    #[yaserde(attribute = true, rename = "OpOnly")]
                    pub op_only: Option<bool>,

                    // obsolete
                    #[yaserde(attribute = true, rename = "FixedAssignment")]
                    pub fixed_assignment: Option<bool>,

                    pub base: String,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct SuType {
                    #[yaserde(attribute = true, rename = "SeparateSu")]
                    pub separate_su: Option<bool>,

                    #[yaserde(attribute = true, rename = "SeparateFrame")]
                    pub separate_frame: Option<bool>,

                    // for future use
                    #[yaserde(attribute = true, rename = "DependOnInputState")]
                    pub depend_on_input_state: Option<bool>,

                    #[yaserde(attribute = true, rename = "FrameRepeatSupport")]
                    pub frame_repeat_support: Option<bool>,

                    pub base: String,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                #[yaserde]
                pub struct MailboxType {
                    #[yaserde(rename = "AoE")]
                    pub ao_e: Option<mailbox_type::AoEType>,

                    #[yaserde(rename = "EoE")]
                    pub eo_e: Option<mailbox_type::EoEType>,

                    #[yaserde(rename = "CoE")]
                    pub co_e: Option<mailbox_type::CoEType>,

                    #[yaserde(rename = "FoE")]
                    pub fo_e: Option<mailbox_type::FoEType>,

                    #[yaserde(rename = "SoE")]
                    pub so_e: Option<mailbox_type::SoEType>,

                    #[yaserde(rename = "VoE")]
                    pub vo_e: Option<mailbox_type::VoEType>,

                    #[yaserde(rename = "VendorSpecific")]
                    pub vendor_specific: Option<String>,

                    #[yaserde(attribute = true, rename = "DataLinkLayer")]
                    pub data_link_layer: Option<bool>,

                    // for future use
                    #[yaserde(attribute = true, rename = "RealTimeMode")]
                    pub real_time_mode: Option<bool>,
                }

                pub mod mailbox_type {
                    use super::*;

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct AoEType {
                        #[yaserde(rename = "InitCmd")]
                        pub init_cmd: Vec<ao_e_type::InitCmdType>,

                        #[yaserde(attribute = true, rename = "AdsRouter")]
                        pub ads_router: Option<bool>,

                        #[yaserde(attribute = true, rename = "GenerateOwnNetId")]
                        pub generate_own_net_id: Option<bool>,

                        #[yaserde(attribute = true, rename = "InitializeOwnNetId")]
                        pub initialize_own_net_id: Option<bool>,
                    }

                    pub mod ao_e_type {
                        use super::*;

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct InitCmdType {
                            #[yaserde(rename = "Transition")]
                            pub transition: Vec<init_cmd_type::TransitionType>,

                            #[yaserde(rename = "Data")]
                            pub data: String,

                            #[yaserde(rename = "Comment")]
                            pub comment: Option<String>,
                        }

                        pub mod init_cmd_type {
                            use super::*;

                            #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                            pub enum TransitionType {
                                #[yaserde(rename = "IP")]
                                Ip,
                                #[yaserde(rename = "PS")]
                                Ps,
                                #[yaserde(rename = "SO")]
                                So,
                                #[yaserde(rename = "SP")]
                                Sp,
                                #[yaserde(rename = "OP")]
                                Op,
                                #[yaserde(rename = "OS")]
                                Os,
                                __Unknown__(String),
                            }

                            impl Default for TransitionType {
                                fn default() -> TransitionType {
                                    Self::__Unknown__("No valid variants".into())
                                }
                            }
                        }
                    }

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct EoEType {
                        #[yaserde(rename = "InitCmd")]
                        pub init_cmd: Vec<eo_e_type::InitCmdType>,

                        #[yaserde(attribute = true, rename = "IP")]
                        pub ip: Option<bool>,

                        #[yaserde(attribute = true, rename = "MAC")]
                        pub mac: Option<bool>,

                        #[yaserde(attribute = true, rename = "TimeStamp")]
                        pub time_stamp: Option<bool>,
                    }

                    pub mod eo_e_type {
                        use super::*;

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct InitCmdType {
                            #[yaserde(rename = "Transition")]
                            pub transition: Vec<init_cmd_type::TransitionType>,

                            #[yaserde(rename = "Type")]
                            pub _type: i32,

                            #[yaserde(rename = "Data")]
                            pub data: String,

                            #[yaserde(rename = "Comment")]
                            pub comment: Option<String>,
                        }

                        pub mod init_cmd_type {
                            use super::*;

                            #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                            pub enum TransitionType {
                                #[yaserde(rename = "IP")]
                                Ip,
                                #[yaserde(rename = "PS")]
                                Ps,
                                #[yaserde(rename = "SO")]
                                So,
                                #[yaserde(rename = "SP")]
                                Sp,
                                #[yaserde(rename = "OP")]
                                Op,
                                #[yaserde(rename = "OS")]
                                Os,
                                __Unknown__(String),
                            }

                            impl Default for TransitionType {
                                fn default() -> TransitionType {
                                    Self::__Unknown__("No valid variants".into())
                                }
                            }
                        }
                    }

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct CoEType {
                        // obsolete
                        #[yaserde(rename = "Object")]
                        pub object: Vec<co_e_type::ObjectType>,

                        #[yaserde(rename = "InitCmd")]
                        pub init_cmd: Vec<co_e_type::InitCmdType>,

                        #[yaserde(attribute = true, rename = "SdoInfo")]
                        pub sdo_info: Option<bool>,

                        #[yaserde(attribute = true, rename = "PdoAssign")]
                        pub pdo_assign: Option<bool>,

                        #[yaserde(attribute = true, rename = "PdoConfig")]
                        pub pdo_config: Option<bool>,

                        #[yaserde(attribute = true, rename = "PdoUpload")]
                        pub pdo_upload: Option<bool>,

                        #[yaserde(attribute = true, rename = "CompleteAccess")]
                        pub complete_access: Option<bool>,

                        #[yaserde(attribute = true, rename = "EdsFile")]
                        pub eds_file: Option<String>,

                        // obsolete
                        #[yaserde(attribute = true, rename = "DS402Channels")]
                        pub ds402_channels: Option<i32>,

                        #[yaserde(attribute = true, rename = "SegmentedSdo")]
                        pub segmented_sdo: Option<bool>,

                        #[yaserde(attribute = true, rename = "DiagHistory")]
                        pub diag_history: Option<bool>,

                        #[yaserde(attribute = true, rename = "SdoUploadWithMaxLength")]
                        pub sdo_upload_with_max_length: Option<bool>,

                        #[yaserde(attribute = true, rename = "TimeDistribution")]
                        pub time_distribution: Option<bool>,
                    }

                    pub mod co_e_type {
                        use super::*;

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct ObjectType {
                            // obsolete
                            #[yaserde(rename = "Index")]
                            pub index: i32,

                            // obsolete
                            #[yaserde(rename = "SubIndex")]
                            pub sub_index: i32,

                            // obsolete
                            #[yaserde(rename = "Data")]
                            pub data: String,

                            // obsolete
                            #[yaserde(rename = "Comment")]
                            pub comment: Option<String>,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct InitCmdType {
                            #[yaserde(rename = "Transition")]
                            pub transition: Vec<init_cmd_type::TransitionType>,

                            #[yaserde(rename = "Index")]
                            pub index: String,

                            #[yaserde(rename = "SubIndex")]
                            pub sub_index: String,

                            #[yaserde(rename = "Data")]
                            pub data: init_cmd_type::DataType,

                            #[yaserde(rename = "Comment")]
                            pub comment: Option<String>,

                            // obsolete
                            #[yaserde(attribute = true, rename = "Fixed")]
                            pub fixed: Option<bool>,

                            #[yaserde(attribute = true, rename = "CompleteAccess")]
                            pub complete_access: Option<bool>,

                            #[yaserde(attribute = true, rename = "OverwrittenByModule")]
                            pub overwritten_by_module: Option<bool>,
                        }

                        pub mod init_cmd_type {
                            use super::*;

                            #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                            pub enum TransitionType {
                                #[yaserde(rename = "IP")]
                                Ip,
                                #[yaserde(rename = "PS")]
                                Ps,
                                #[yaserde(rename = "SO")]
                                So,
                                #[yaserde(rename = "SP")]
                                Sp,
                                #[yaserde(rename = "OP")]
                                Op,
                                #[yaserde(rename = "OS")]
                                Os,
                                __Unknown__(String),
                            }

                            impl Default for TransitionType {
                                fn default() -> TransitionType {
                                    Self::__Unknown__("No valid variants".into())
                                }
                            }

                            #[derive(
                                Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize,
                            )]
                            pub struct DataType {
                                #[yaserde(attribute = true, rename = "AdaptAutomatically")]
                                pub adapt_automatically: Option<bool>,

                                pub base: String,
                            }
                        }
                    }

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct FoEType {}

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct SoEType {
                        #[yaserde(rename = "InitCmd")]
                        pub init_cmd: Vec<so_e_type::InitCmdType>,

                        #[yaserde(rename = "SoEChoice")]
                        pub so_e_choice: so_e_type::SoEChoice,

                        #[yaserde(attribute = true, rename = "ChannelCount")]
                        pub channel_count: Option<i32>,

                        #[yaserde(attribute = true, rename = "DriveFollowsBit3Support")]
                        pub drive_follows_bit_3_support: Option<bool>,
                    }

                    pub mod so_e_type {
                        use super::*;

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct InitCmdType {
                            #[yaserde(rename = "Transition")]
                            pub transition: Vec<init_cmd_type::TransitionType>,

                            #[yaserde(rename = "IDN")]
                            pub idn: i32,

                            #[yaserde(rename = "Data")]
                            pub data: String,

                            #[yaserde(rename = "Comment")]
                            pub comment: Option<String>,

                            #[yaserde(attribute = true, rename = "Chn")]
                            pub chn: Option<i32>,
                        }

                        pub mod init_cmd_type {
                            use super::*;

                            #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                            pub enum TransitionType {
                                #[yaserde(rename = "IP")]
                                Ip,
                                #[yaserde(rename = "PS")]
                                Ps,
                                #[yaserde(rename = "SO")]
                                So,
                                #[yaserde(rename = "SP")]
                                Sp,
                                #[yaserde(rename = "OP")]
                                Op,
                                #[yaserde(rename = "OS")]
                                Os,
                                __Unknown__(String),
                            }

                            impl Default for TransitionType {
                                fn default() -> TransitionType {
                                    Self::__Unknown__("No valid variants".into())
                                }
                            }
                        }

                        #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                        pub enum SoEChoice {
                            DiagFile(Vec<String>),
                            DiagMessages(Option<DiagnosticsType>),
                            __Unknown__(String),
                        }

                        impl Default for SoEChoice {
                            fn default() -> SoEChoice {
                                Self::__Unknown__("No valid variants".into())
                            }
                        }
                    }

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct VoEType {}
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct DcType {
                    #[yaserde(rename = "OpMode")]
                    pub op_mode: Vec<dc_type::OpModeType>,

                    #[yaserde(rename = "VendorSpecific")]
                    pub vendor_specific: Option<String>,

                    #[yaserde(attribute = true, rename = "UnknownFRMW")]
                    pub unknown_frmw: Option<bool>,

                    #[yaserde(attribute = true, rename = "Unknown64Bit")]
                    pub unknown_64_bit: Option<bool>,

                    #[yaserde(attribute = true, rename = "ExternalRefClock")]
                    pub external_ref_clock: Option<bool>,

                    #[yaserde(attribute = true, rename = "PotentialReferenceClock")]
                    pub potential_reference_clock: Option<bool>,

                    #[yaserde(attribute = true, rename = "TimeLoopControlOnly")]
                    pub time_loop_control_only: Option<bool>,

                    #[yaserde(attribute = true, rename = "PdoOversampling")]
                    pub pdo_oversampling: Option<bool>,
                }

                pub mod dc_type {
                    use super::*;

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct OpModeType {
                        #[yaserde(rename = "Name")]
                        pub name: String,

                        #[yaserde(rename = "Desc")]
                        pub desc: Option<String>,

                        #[yaserde(rename = "AssignActivate")]
                        pub assign_activate: String,

                        #[yaserde(rename = "ActivateAdditional")]
                        pub activate_additional: Option<String>,

                        #[yaserde(rename = "CycleTimeSync0")]
                        pub cycle_time_sync_0: Option<op_mode_type::CycleTimeSync0Type>,

                        #[yaserde(rename = "ShiftTimeSync0")]
                        pub shift_time_sync_0: Option<op_mode_type::ShiftTimeSync0Type>,

                        #[yaserde(rename = "CycleTimeSync1")]
                        pub cycle_time_sync_1: Option<op_mode_type::CycleTimeSync1Type>,

                        #[yaserde(rename = "ShiftTimeSync1")]
                        pub shift_time_sync_1: Option<op_mode_type::ShiftTimeSync1Type>,

                        #[yaserde(rename = "CycleTimeSync2")]
                        pub cycle_time_sync_2: Option<op_mode_type::CycleTimeSync2Type>,

                        #[yaserde(rename = "ShiftTimeSync2")]
                        pub shift_time_sync_2: Option<op_mode_type::ShiftTimeSync2Type>,

                        #[yaserde(rename = "CycleTimeSync3")]
                        pub cycle_time_sync_3: Option<op_mode_type::CycleTimeSync3Type>,

                        #[yaserde(rename = "ShiftTimeSync3")]
                        pub shift_time_sync_3: Option<op_mode_type::ShiftTimeSync3Type>,

                        #[yaserde(rename = "Sm")]
                        pub sm: Vec<op_mode_type::SmType>,

                        #[yaserde(rename = "VendorSpecific")]
                        pub vendor_specific: Option<String>,
                    }

                    pub mod op_mode_type {
                        use super::*;

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct CycleTimeSync0Type {
                            #[yaserde(attribute = true, rename = "Factor")]
                            pub factor: Option<i32>,

                            pub base: i32,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct ShiftTimeSync0Type {
                            #[yaserde(attribute = true, rename = "Factor")]
                            pub factor: Option<i32>,

                            #[yaserde(attribute = true, rename = "Input")]
                            pub input: Option<bool>,

                            #[yaserde(attribute = true, rename = "OutputDelayTime")]
                            pub output_delay_time: Option<i32>,

                            #[yaserde(attribute = true, rename = "InputDelayTime")]
                            pub input_delay_time: Option<i32>,

                            pub base: i32,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct CycleTimeSync1Type {
                            #[yaserde(attribute = true, rename = "Factor")]
                            pub factor: Option<i32>,

                            pub base: i32,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct ShiftTimeSync1Type {
                            // for future use
                            #[yaserde(attribute = true, rename = "Factor")]
                            pub factor: Option<i32>,

                            #[yaserde(attribute = true, rename = "Input")]
                            pub input: Option<bool>,

                            #[yaserde(attribute = true, rename = "OutputDelayTime")]
                            pub output_delay_time: Option<i32>,

                            #[yaserde(attribute = true, rename = "InputDelayTime")]
                            pub input_delay_time: Option<i32>,

                            pub base: i32,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct CycleTimeSync2Type {
                            #[yaserde(attribute = true, rename = "Factor")]
                            pub factor: Option<i32>,

                            pub base: i32,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct ShiftTimeSync2Type {
                            #[yaserde(attribute = true, rename = "Factor")]
                            pub factor: Option<i32>,

                            #[yaserde(attribute = true, rename = "Input")]
                            pub input: Option<bool>,

                            #[yaserde(attribute = true, rename = "OutputDelayTime")]
                            pub output_delay_time: Option<i32>,

                            #[yaserde(attribute = true, rename = "InputDelayTime")]
                            pub input_delay_time: Option<i32>,

                            pub base: i32,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct CycleTimeSync3Type {
                            #[yaserde(attribute = true, rename = "Factor")]
                            pub factor: Option<i32>,

                            pub base: i32,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct ShiftTimeSync3Type {
                            // for future use
                            #[yaserde(attribute = true, rename = "Factor")]
                            pub factor: Option<i32>,

                            #[yaserde(attribute = true, rename = "Input")]
                            pub input: Option<bool>,

                            #[yaserde(attribute = true, rename = "OutputDelayTime")]
                            pub output_delay_time: Option<i32>,

                            #[yaserde(attribute = true, rename = "InputDelayTime")]
                            pub input_delay_time: Option<i32>,

                            pub base: i32,
                        }

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct SmType {
                            // obsolete
                            #[yaserde(rename = "SyncType")]
                            pub sync_type: Option<i32>,

                            // obsolete
                            #[yaserde(rename = "CycleTime")]
                            pub cycle_time: Option<sm_type::CycleTimeType>,

                            // obsolete
                            #[yaserde(rename = "ShiftTime")]
                            pub shift_time: Option<sm_type::ShiftTimeType>,

                            #[yaserde(rename = "Pdo")]
                            pub pdo: Vec<sm_type::PdoType>,

                            #[yaserde(attribute = true, rename = "No")]
                            pub no: i32,
                        }

                        pub mod sm_type {
                            use super::*;

                            #[derive(
                                Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize,
                            )]
                            pub struct CycleTimeType {
                                // obsolete
                                #[yaserde(attribute = true, rename = "Factor")]
                                pub factor: Option<i32>,

                                pub base: i32,
                            }

                            #[derive(
                                Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize,
                            )]
                            pub struct ShiftTimeType {
                                // obsolete
                                #[yaserde(attribute = true, rename = "MinAfterSync")]
                                pub min_after_sync: Option<i32>,

                                // obsolete
                                #[yaserde(attribute = true, rename = "MinBeforeFrame")]
                                pub min_before_frame: Option<i32>,

                                pub base: i32,
                            }

                            #[derive(
                                Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize,
                            )]
                            pub struct PdoType {
                                #[yaserde(attribute = true, rename = "OSFac")]
                                pub os_fac: Option<i32>,

                                pub base: String,
                            }
                        }
                    }
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct SlotsType {
                    #[yaserde(rename = "Slot")]
                    pub slot: Vec<SlotType>,

                    #[yaserde(rename = "ModuleGroups")]
                    pub module_groups: Option<slots_type::ModuleGroupsType>,

                    #[yaserde(rename = "SlotGroupData")]
                    pub slot_group_data: Vec<slots_type::SlotGroupDataType>,

                    #[yaserde(rename = "ModulePdoGroup")]
                    pub module_pdo_group: Vec<slots_type::ModulePdoGroupType>,

                    #[yaserde(rename = "SlotSelections")]
                    pub slot_selections: Vec<slots_type::SlotSelectionsType>,

                    #[yaserde(attribute = true, rename = "DownloadModuleIdentList")]
                    pub download_module_ident_list: Option<bool>,

                    #[yaserde(attribute = true, rename = "DownloadModuleAddressList")]
                    pub download_module_address_list: Option<bool>,

                    #[yaserde(attribute = true, rename = "DownloadModuleListTransition")]
                    pub download_module_list_transition: Option<String>,

                    #[yaserde(attribute = true, rename = "MaxSlotCount")]
                    pub max_slot_count: Option<String>,

                    #[yaserde(attribute = true, rename = "MaxSlotGroupCount")]
                    pub max_slot_group_count: Option<String>,

                    #[yaserde(attribute = true, rename = "SlotPdoIncrement")]
                    pub slot_pdo_increment: Option<String>,

                    #[yaserde(attribute = true, rename = "SlotGroupPdoIncrement")]
                    pub slot_group_pdo_increment: Option<String>,

                    #[yaserde(attribute = true, rename = "SlotIndexIncrement")]
                    pub slot_index_increment: Option<String>,

                    #[yaserde(attribute = true, rename = "SlotGroupIndexIncrement")]
                    pub slot_group_index_increment: Option<String>,

                    // obsolete
                    #[yaserde(attribute = true, rename = "IdentifyModuleBy")]
                    pub identify_module_by: Option<String>,
                }

                pub mod slots_type {
                    use super::*;

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct ModuleGroupsType {
                        #[yaserde(rename = "ModuleGroup")]
                        pub module_group: Vec<module_groups_type::ModuleGroupType>,
                    }

                    pub mod module_groups_type {
                        use super::*;

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct ModuleGroupType {
                            #[yaserde(rename = "Name")]
                            pub name: String,

                            #[yaserde(rename = "Type")]
                            pub _type: String,

                            #[yaserde(rename = "Class")]
                            pub class: Option<String>,

                            #[yaserde(rename = "ModuleIdent")]
                            pub module_ident: Vec<String>,

                            #[yaserde(rename = "ModuleGroupChoice")]
                            pub module_group_choice: module_group_type::ModuleGroupChoice,

                            #[yaserde(attribute = true, rename = "Id")]
                            pub id: String,
                        }

                        pub mod module_group_type {
                            use super::*;

                            #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                            pub enum ModuleGroupChoice {
                                #[yaserde(rename = "ImageFile16x14")]
                                ImageFile16X14(Option<String>),
                                #[yaserde(rename = "ImageData16x14")]
                                ImageData16X14(Option<String>),
                                __Unknown__(String),
                            }

                            impl Default for ModuleGroupChoice {
                                fn default() -> ModuleGroupChoice {
                                    Self::__Unknown__("No valid variants".into())
                                }
                            }
                        }
                    }

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct SlotGroupDataType {
                        #[yaserde(rename = "Name")]
                        pub name: Vec<String>,

                        #[yaserde(rename = "ModuleGroups")]
                        pub module_groups: Option<slot_group_data_type::ModuleGroupsType>,

                        #[yaserde(rename = "SlotGroupDataChoice")]
                        pub slot_group_data_choice: slot_group_data_type::SlotGroupDataChoice,

                        #[yaserde(attribute = true, rename = "SlotGroup")]
                        pub slot_group: String,
                    }

                    pub mod slot_group_data_type {
                        use super::*;

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct ModuleGroupsType {
                            #[yaserde(rename = "Id")]
                            pub id: Vec<String>,
                        }

                        #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                        pub enum SlotGroupDataChoice {
                            #[yaserde(rename = "ImageFile16x14")]
                            ImageFile16X14(Option<String>),
                            #[yaserde(rename = "ImageData16x14")]
                            ImageData16X14(Option<String>),
                            __Unknown__(String),
                        }

                        impl Default for SlotGroupDataChoice {
                            fn default() -> SlotGroupDataChoice {
                                Self::__Unknown__("No valid variants".into())
                            }
                        }
                    }

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct ModulePdoGroupType {
                        #[yaserde(attribute = true, rename = "Alignment")]
                        pub alignment: Option<i32>,

                        #[yaserde(attribute = true, rename = "RxPdo")]
                        pub rx_pdo: Option<String>,

                        #[yaserde(attribute = true, rename = "TxPdo")]
                        pub tx_pdo: Option<String>,

                        pub base: String,
                    }

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct SlotSelectionsType {
                        #[yaserde(rename = "Name")]
                        pub name: String,

                        #[yaserde(rename = "ModuleIdent")]
                        pub module_ident: Vec<String>,
                    }
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct Esctype {
                    #[yaserde(rename = "Reg0108")]
                    pub reg_0108: Option<String>,

                    #[yaserde(rename = "Reg0400")]
                    pub reg_0400: Option<String>,

                    #[yaserde(rename = "Reg0410")]
                    pub reg_0410: Option<String>,

                    #[yaserde(rename = "Reg0420")]
                    pub reg_0420: Option<String>,

                    #[yaserde(rename = "VendorSpecific")]
                    pub vendor_specific: Option<String>,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct EepromType {
                    #[yaserde(attribute = true, rename = "AssignToPdi")]
                    pub assign_to_pdi: Option<bool>,

                    pub base: EepromType,
                }

                #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                pub enum DeviceTypeChoice {
                    // obsolete
                    #[yaserde(rename = "Image16x14")]
                    Image16X14(Option<String>),
                    #[yaserde(rename = "ImageFile16x14")]
                    ImageFile16X14(Option<String>),
                    #[yaserde(rename = "ImageData16x14")]
                    ImageData16X14(Option<String>),
                    __Unknown__(String),
                }

                impl Default for DeviceTypeChoice {
                    fn default() -> DeviceTypeChoice {
                        Self::__Unknown__("No valid variants".into())
                    }
                }
            }
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct ModulesType {
            #[yaserde(rename = "Module")]
            pub module: Vec<modules_type::ModuleType>,
        }

        pub mod modules_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct ModuleType {
                #[yaserde(attribute = true, rename = "Crc32")]
                pub crc_32: Option<String>,
            }
        }
    }
}

#[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
pub struct DeviceType {
    // (SCI)
    #[yaserde(rename = "Sci")]
    pub sci: Option<device_type::SciType>,

    #[yaserde(rename = "Type")]
    pub _type: device_type::TypeType,

    #[yaserde(rename = "HideType")]
    pub hide_type: Vec<device_type::HideTypeType>,

    #[yaserde(rename = "AlternativeType")]
    pub alternative_type: Vec<device_type::AlternativeTypeType>,

    #[yaserde(rename = "SubDevice")]
    pub sub_device: Vec<device_type::SubDeviceType>,

    #[yaserde(rename = "Name")]
    pub name: Vec<String>,

    #[yaserde(rename = "Comment")]
    pub comment: Vec<String>,

    #[yaserde(rename = "URL")]
    pub url: Vec<String>,

    #[yaserde(rename = "Info")]
    pub info: Option<InfoType>,

    #[yaserde(rename = "GroupType")]
    pub group_type: String,

    #[yaserde(rename = "Profile")]
    pub profile: Vec<device_type::ProfileType>,

    #[yaserde(rename = "Fmmu")]
    pub fmmu: Vec<device_type::FmmuType>,

    #[yaserde(rename = "Sm")]
    pub sm: Vec<device_type::SmType>,

    #[yaserde(rename = "Su")]
    pub su: Vec<device_type::SuType>,

    #[yaserde(rename = "RxPdo")]
    pub rx_pdo: Vec<PdoType>,

    #[yaserde(rename = "TxPdo")]
    pub tx_pdo: Vec<PdoType>,

    #[yaserde(rename = "Mailbox")]
    pub mailbox: Option<device_type::MailboxType>,

    #[yaserde(rename = "Dc")]
    pub dc: Option<device_type::DcType>,

    #[yaserde(rename = "Slots")]
    pub slots: Option<device_type::SlotsType>,

    #[yaserde(rename = "ESC")]
    pub esc: Option<device_type::Esctype>,

    #[yaserde(rename = "Eeprom")]
    pub eeprom: Option<device_type::EepromType>,

    #[yaserde(rename = "DeviceTypeChoice")]
    pub device_type_choice: device_type::DeviceTypeChoice,

    #[yaserde(rename = "VendorSpecific")]
    pub vendor_specific: Option<String>,
}

pub mod device_type {
    use super::*;

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct SciType {
        #[yaserde(rename = "Name")]
        pub name: Vec<String>,

        #[yaserde(rename = "Description")]
        pub description: Vec<String>,

        #[yaserde(rename = "Guid")]
        pub guid: Guid,

        #[yaserde(rename = "CreatedBy")]
        pub created_by: sci_type::CreatedByType,

        #[yaserde(rename = "TargetSpecific")]
        pub target_specific: Option<sci_type::TargetSpecificType>,

        #[yaserde(rename = "VendorSpecific")]
        pub vendor_specific: Option<String>,

        // Version of ETG.2000 on which this SCI is based on
        #[yaserde(attribute = true, rename = "SciVersion")]
        pub sci_version: Option<String>,
    }

    pub mod sci_type {
        use super::*;

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct CreatedByType {
            #[yaserde(rename = "Company")]
            pub company: String,

            #[yaserde(rename = "VendorId")]
            pub vendor_id: Option<String>,

            #[yaserde(rename = "Tool")]
            pub tool: Option<created_by_type::ToolType>,
        }

        pub mod created_by_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct ToolType {
                #[yaserde(attribute = true, rename = "Version")]
                pub version: Option<String>,
            }
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct TargetSpecificType {
            #[yaserde(rename = "AoeNetId")]
            pub aoe_net_id: Option<target_specific_type::AoeNetIdType>,

            #[yaserde(rename = "EoeMacIp")]
            pub eoe_mac_ip: Option<target_specific_type::EoeMacIpType>,

            #[yaserde(rename = "DcCycleTime")]
            pub dc_cycle_time: Option<target_specific_type::DcCycleTimeType>,

            #[yaserde(rename = "ModuleIdents")]
            pub module_idents: Option<target_specific_type::ModuleIdentsType>,
        }

        pub mod target_specific_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct AoeNetIdType {
                #[yaserde(rename = "Comment")]
                pub comment: Option<String>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct EoeMacIpType {
                #[yaserde(rename = "Comment")]
                pub comment: Option<String>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct DcCycleTimeType {
                #[yaserde(rename = "Comment")]
                pub comment: Option<String>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct ModuleIdentsType {
                #[yaserde(rename = "ModuleIdent")]
                pub module_ident: Vec<module_idents_type::ModuleIdentType>,
            }

            pub mod module_idents_type {
                use super::*;

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct ModuleIdentType {
                    #[yaserde(rename = "SlotNo")]
                    pub slot_no: i32,

                    #[yaserde(rename = "Esi")]
                    pub esi: String,

                    #[yaserde(rename = "Sci")]
                    pub sci: String,
                }
            }
        }
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct TypeType {
        #[yaserde(attribute = true, rename = "ProductCode")]
        pub product_code: Option<String>,

        #[yaserde(attribute = true, rename = "RevisionNo")]
        pub revision_no: Option<String>,

        #[yaserde(attribute = true, rename = "SerialNo")]
        pub serial_no: Option<String>,

        #[yaserde(attribute = true, rename = "CheckProductCode")]
        pub check_product_code: Option<String>,

        #[yaserde(attribute = true, rename = "CheckRevisionNo")]
        pub check_revision_no: Option<String>,

        #[yaserde(attribute = true, rename = "CheckSerialNo")]
        pub check_serial_no: Option<String>,

        // obsolete
        #[yaserde(attribute = true, rename = "TcSmClass")]
        pub tc_sm_class: Option<String>,

        // obsolete
        #[yaserde(attribute = true, rename = "TcCfgModeSafeOp")]
        pub tc_cfg_mode_safe_op: Option<bool>,

        #[yaserde(attribute = true, rename = "UseLrdLwr")]
        pub use_lrd_lwr: Option<bool>,

        #[yaserde(attribute = true, rename = "ModulePdoGroup")]
        pub module_pdo_group: Option<i32>,

        // obsolete
        #[yaserde(attribute = true, rename = "DownloadModuleList")]
        pub download_module_list: Option<bool>,

        #[yaserde(attribute = true, rename = "ShowHideableSubDevices")]
        pub show_hideable_sub_devices: Option<bool>,
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct HideTypeType {
        #[yaserde(attribute = true, rename = "ProductCode")]
        pub product_code: Option<String>,

        #[yaserde(attribute = true, rename = "RevisionNo")]
        pub revision_no: Option<String>,

        // obsolete
        #[yaserde(attribute = true, rename = "ProductRevision")]
        pub product_revision: Option<String>,
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct AlternativeTypeType {
        // for future use
        #[yaserde(attribute = true, rename = "ProductCode")]
        pub product_code: Option<String>,

        // for future use
        #[yaserde(attribute = true, rename = "RevisionNo")]
        pub revision_no: Option<String>,
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct SubDeviceType {
        // for future use
        #[yaserde(attribute = true, rename = "ProductCode")]
        pub product_code: Option<String>,

        // for future use
        #[yaserde(attribute = true, rename = "RevisionNo")]
        pub revision_no: Option<String>,

        #[yaserde(attribute = true, rename = "PreviousDevice")]
        pub previous_device: Option<i32>,

        #[yaserde(attribute = true, rename = "PreviousPortNo")]
        pub previous_port_no: Option<i32>,

        #[yaserde(attribute = true, rename = "Hideable")]
        pub hideable: Option<bool>,
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct ProfileType {
        // obsolete
        #[yaserde(attribute = true, rename = "Channel")]
        pub channel: Option<i32>,
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct FmmuType {
        // obsolete
        #[yaserde(attribute = true, rename = "OpOnly")]
        pub op_only: Option<bool>,

        #[yaserde(attribute = true, rename = "Sm")]
        pub sm: Option<i32>,

        #[yaserde(attribute = true, rename = "Su")]
        pub su: Option<i32>,
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct SmType {
        #[yaserde(attribute = true, rename = "MinSize")]
        pub min_size: Option<String>,

        #[yaserde(attribute = true, rename = "MaxSize")]
        pub max_size: Option<String>,

        #[yaserde(attribute = true, rename = "DefaultSize")]
        pub default_size: Option<String>,

        #[yaserde(attribute = true, rename = "StartAddress")]
        pub start_address: Option<String>,

        #[yaserde(attribute = true, rename = "ControlByte")]
        pub control_byte: Option<String>,

        #[yaserde(attribute = true, rename = "Enable")]
        pub enable: Option<String>,

        // obsolete
        #[yaserde(attribute = true, rename = "OneByteMode")]
        pub one_byte_mode: Option<bool>,

        #[yaserde(attribute = true, rename = "Virtual")]
        pub _virtual: Option<bool>,

        // obsolete
        #[yaserde(attribute = true, rename = "Watchdog")]
        pub watchdog: Option<bool>,

        #[yaserde(attribute = true, rename = "OpOnly")]
        pub op_only: Option<bool>,

        // obsolete
        #[yaserde(attribute = true, rename = "FixedAssignment")]
        pub fixed_assignment: Option<bool>,
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct SuType {
        #[yaserde(attribute = true, rename = "SeparateSu")]
        pub separate_su: Option<bool>,

        #[yaserde(attribute = true, rename = "SeparateFrame")]
        pub separate_frame: Option<bool>,

        // for future use
        #[yaserde(attribute = true, rename = "DependOnInputState")]
        pub depend_on_input_state: Option<bool>,

        #[yaserde(attribute = true, rename = "FrameRepeatSupport")]
        pub frame_repeat_support: Option<bool>,
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct MailboxType {
        #[yaserde(rename = "AoE")]
        pub ao_e: Option<mailbox_type::AoEType>,

        #[yaserde(rename = "EoE")]
        pub eo_e: Option<mailbox_type::EoEType>,

        #[yaserde(rename = "CoE")]
        pub co_e: Option<mailbox_type::CoEType>,

        #[yaserde(rename = "FoE")]
        pub fo_e: Option<mailbox_type::FoEType>,

        #[yaserde(rename = "SoE")]
        pub so_e: Option<mailbox_type::SoEType>,

        #[yaserde(rename = "VoE")]
        pub vo_e: Option<mailbox_type::VoEType>,

        #[yaserde(rename = "VendorSpecific")]
        pub vendor_specific: Option<String>,

        #[yaserde(attribute = true, rename = "DataLinkLayer")]
        pub data_link_layer: Option<bool>,

        // for future use
        #[yaserde(attribute = true, rename = "RealTimeMode")]
        pub real_time_mode: Option<bool>,
    }

    pub mod mailbox_type {
        use super::*;

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct AoEType {
            #[yaserde(rename = "InitCmd")]
            pub init_cmd: Vec<ao_e_type::InitCmdType>,

            #[yaserde(attribute = true, rename = "AdsRouter")]
            pub ads_router: Option<bool>,

            #[yaserde(attribute = true, rename = "GenerateOwnNetId")]
            pub generate_own_net_id: Option<bool>,

            #[yaserde(attribute = true, rename = "InitializeOwnNetId")]
            pub initialize_own_net_id: Option<bool>,
        }

        pub mod ao_e_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct InitCmdType {
                #[yaserde(rename = "Transition")]
                pub transition: Vec<init_cmd_type::TransitionType>,

                #[yaserde(rename = "Data")]
                pub data: String,

                #[yaserde(rename = "Comment")]
                pub comment: Option<String>,
            }

            pub mod init_cmd_type {
                use super::*;

                #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                pub enum TransitionType {
                    #[yaserde(rename = "IP")]
                    Ip,
                    #[yaserde(rename = "PS")]
                    Ps,
                    #[yaserde(rename = "SO")]
                    So,
                    #[yaserde(rename = "SP")]
                    Sp,
                    #[yaserde(rename = "OP")]
                    Op,
                    #[yaserde(rename = "OS")]
                    Os,
                    __Unknown__(String),
                }

                impl Default for TransitionType {
                    fn default() -> TransitionType {
                        Self::__Unknown__("No valid variants".into())
                    }
                }
            }
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct EoEType {
            #[yaserde(rename = "InitCmd")]
            pub init_cmd: Vec<eo_e_type::InitCmdType>,

            #[yaserde(attribute = true, rename = "IP")]
            pub ip: Option<bool>,

            #[yaserde(attribute = true, rename = "MAC")]
            pub mac: Option<bool>,

            #[yaserde(attribute = true, rename = "TimeStamp")]
            pub time_stamp: Option<bool>,
        }

        pub mod eo_e_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct InitCmdType {
                #[yaserde(rename = "Transition")]
                pub transition: Vec<init_cmd_type::TransitionType>,

                #[yaserde(rename = "Type")]
                pub _type: i32,

                #[yaserde(rename = "Data")]
                pub data: String,

                #[yaserde(rename = "Comment")]
                pub comment: Option<String>,
            }

            pub mod init_cmd_type {
                use super::*;

                #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                pub enum TransitionType {
                    #[yaserde(rename = "IP")]
                    Ip,
                    #[yaserde(rename = "PS")]
                    Ps,
                    #[yaserde(rename = "SO")]
                    So,
                    #[yaserde(rename = "SP")]
                    Sp,
                    #[yaserde(rename = "OP")]
                    Op,
                    #[yaserde(rename = "OS")]
                    Os,
                    __Unknown__(String),
                }

                impl Default for TransitionType {
                    fn default() -> TransitionType {
                        Self::__Unknown__("No valid variants".into())
                    }
                }
            }
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct CoEType {
            // obsolete
            #[yaserde(rename = "Object")]
            pub object: Vec<co_e_type::ObjectType>,

            #[yaserde(rename = "InitCmd")]
            pub init_cmd: Vec<co_e_type::InitCmdType>,

            #[yaserde(attribute = true, rename = "SdoInfo")]
            pub sdo_info: Option<bool>,

            #[yaserde(attribute = true, rename = "PdoAssign")]
            pub pdo_assign: Option<bool>,

            #[yaserde(attribute = true, rename = "PdoConfig")]
            pub pdo_config: Option<bool>,

            #[yaserde(attribute = true, rename = "PdoUpload")]
            pub pdo_upload: Option<bool>,

            #[yaserde(attribute = true, rename = "CompleteAccess")]
            pub complete_access: Option<bool>,

            #[yaserde(attribute = true, rename = "EdsFile")]
            pub eds_file: Option<String>,

            // obsolete
            #[yaserde(attribute = true, rename = "DS402Channels")]
            pub ds402_channels: Option<i32>,

            #[yaserde(attribute = true, rename = "SegmentedSdo")]
            pub segmented_sdo: Option<bool>,

            #[yaserde(attribute = true, rename = "DiagHistory")]
            pub diag_history: Option<bool>,

            #[yaserde(attribute = true, rename = "SdoUploadWithMaxLength")]
            pub sdo_upload_with_max_length: Option<bool>,

            #[yaserde(attribute = true, rename = "TimeDistribution")]
            pub time_distribution: Option<bool>,
        }

        pub mod co_e_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct ObjectType {
                // obsolete
                #[yaserde(rename = "Index")]
                pub index: i32,

                // obsolete
                #[yaserde(rename = "SubIndex")]
                pub sub_index: i32,

                // obsolete
                #[yaserde(rename = "Data")]
                pub data: String,

                // obsolete
                #[yaserde(rename = "Comment")]
                pub comment: Option<String>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct InitCmdType {
                #[yaserde(rename = "Transition")]
                pub transition: Vec<init_cmd_type::TransitionType>,

                #[yaserde(rename = "Index")]
                pub index: String,

                #[yaserde(rename = "SubIndex")]
                pub sub_index: String,

                #[yaserde(rename = "Data")]
                pub data: init_cmd_type::DataType,

                #[yaserde(rename = "Comment")]
                pub comment: Option<String>,

                // obsolete
                #[yaserde(attribute = true, rename = "Fixed")]
                pub fixed: Option<bool>,

                #[yaserde(attribute = true, rename = "CompleteAccess")]
                pub complete_access: Option<bool>,

                #[yaserde(attribute = true, rename = "OverwrittenByModule")]
                pub overwritten_by_module: Option<bool>,
            }

            pub mod init_cmd_type {
                use super::*;

                #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                pub enum TransitionType {
                    #[yaserde(rename = "IP")]
                    Ip,
                    #[yaserde(rename = "PS")]
                    Ps,
                    #[yaserde(rename = "SO")]
                    So,
                    #[yaserde(rename = "SP")]
                    Sp,
                    #[yaserde(rename = "OP")]
                    Op,
                    #[yaserde(rename = "OS")]
                    Os,
                    __Unknown__(String),
                }

                impl Default for TransitionType {
                    fn default() -> TransitionType {
                        Self::__Unknown__("No valid variants".into())
                    }
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct DataType {
                    #[yaserde(attribute = true, rename = "AdaptAutomatically")]
                    pub adapt_automatically: Option<bool>,
                }
            }
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct FoEType {}

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct SoEType {
            #[yaserde(rename = "InitCmd")]
            pub init_cmd: Vec<so_e_type::InitCmdType>,

            #[yaserde(rename = "SoEChoice")]
            pub so_e_choice: so_e_type::SoEChoice,

            #[yaserde(attribute = true, rename = "ChannelCount")]
            pub channel_count: Option<i32>,

            #[yaserde(attribute = true, rename = "DriveFollowsBit3Support")]
            pub drive_follows_bit_3_support: Option<bool>,
        }

        pub mod so_e_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct InitCmdType {
                #[yaserde(rename = "Transition")]
                pub transition: Vec<init_cmd_type::TransitionType>,

                #[yaserde(rename = "IDN")]
                pub idn: i32,

                #[yaserde(rename = "Data")]
                pub data: String,

                #[yaserde(rename = "Comment")]
                pub comment: Option<String>,

                #[yaserde(attribute = true, rename = "Chn")]
                pub chn: Option<i32>,
            }

            pub mod init_cmd_type {
                use super::*;

                #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                pub enum TransitionType {
                    #[yaserde(rename = "IP")]
                    Ip,
                    #[yaserde(rename = "PS")]
                    Ps,
                    #[yaserde(rename = "SO")]
                    So,
                    #[yaserde(rename = "SP")]
                    Sp,
                    #[yaserde(rename = "OP")]
                    Op,
                    #[yaserde(rename = "OS")]
                    Os,
                    __Unknown__(String),
                }

                impl Default for TransitionType {
                    fn default() -> TransitionType {
                        Self::__Unknown__("No valid variants".into())
                    }
                }
            }

            #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

            pub enum SoEChoice {
                DiagFile(Vec<String>),
                DiagMessages(Option<DiagnosticsType>),
                __Unknown__(String),
            }

            impl Default for SoEChoice {
                fn default() -> SoEChoice {
                    Self::__Unknown__("No valid variants".into())
                }
            }
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct VoEType {}
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct DcType {
        #[yaserde(rename = "OpMode")]
        pub op_mode: Vec<dc_type::OpModeType>,

        #[yaserde(rename = "VendorSpecific")]
        pub vendor_specific: Option<String>,

        #[yaserde(attribute = true, rename = "UnknownFRMW")]
        pub unknown_frmw: Option<bool>,

        #[yaserde(attribute = true, rename = "Unknown64Bit")]
        pub unknown_64_bit: Option<bool>,

        #[yaserde(attribute = true, rename = "ExternalRefClock")]
        pub external_ref_clock: Option<bool>,

        #[yaserde(attribute = true, rename = "PotentialReferenceClock")]
        pub potential_reference_clock: Option<bool>,

        #[yaserde(attribute = true, rename = "TimeLoopControlOnly")]
        pub time_loop_control_only: Option<bool>,

        #[yaserde(attribute = true, rename = "PdoOversampling")]
        pub pdo_oversampling: Option<bool>,
    }

    pub mod dc_type {
        use super::*;

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct OpModeType {
            #[yaserde(rename = "Name")]
            pub name: String,

            #[yaserde(rename = "Desc")]
            pub desc: Option<String>,

            #[yaserde(rename = "AssignActivate")]
            pub assign_activate: String,

            #[yaserde(rename = "ActivateAdditional")]
            pub activate_additional: Option<String>,

            #[yaserde(rename = "CycleTimeSync0")]
            pub cycle_time_sync_0: Option<op_mode_type::CycleTimeSync0Type>,

            #[yaserde(rename = "ShiftTimeSync0")]
            pub shift_time_sync_0: Option<op_mode_type::ShiftTimeSync0Type>,

            #[yaserde(rename = "CycleTimeSync1")]
            pub cycle_time_sync_1: Option<op_mode_type::CycleTimeSync1Type>,

            #[yaserde(rename = "ShiftTimeSync1")]
            pub shift_time_sync_1: Option<op_mode_type::ShiftTimeSync1Type>,

            #[yaserde(rename = "CycleTimeSync2")]
            pub cycle_time_sync_2: Option<op_mode_type::CycleTimeSync2Type>,

            #[yaserde(rename = "ShiftTimeSync2")]
            pub shift_time_sync_2: Option<op_mode_type::ShiftTimeSync2Type>,

            #[yaserde(rename = "CycleTimeSync3")]
            pub cycle_time_sync_3: Option<op_mode_type::CycleTimeSync3Type>,

            #[yaserde(rename = "ShiftTimeSync3")]
            pub shift_time_sync_3: Option<op_mode_type::ShiftTimeSync3Type>,

            #[yaserde(rename = "Sm")]
            pub sm: Vec<op_mode_type::SmType>,

            #[yaserde(rename = "VendorSpecific")]
            pub vendor_specific: Option<String>,
        }

        pub mod op_mode_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct CycleTimeSync0Type {
                #[yaserde(attribute = true, rename = "Factor")]
                pub factor: Option<i32>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct ShiftTimeSync0Type {
                #[yaserde(attribute = true, rename = "Factor")]
                pub factor: Option<i32>,

                #[yaserde(attribute = true, rename = "Input")]
                pub input: Option<bool>,

                #[yaserde(attribute = true, rename = "OutputDelayTime")]
                pub output_delay_time: Option<i32>,

                #[yaserde(attribute = true, rename = "InputDelayTime")]
                pub input_delay_time: Option<i32>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct CycleTimeSync1Type {
                #[yaserde(attribute = true, rename = "Factor")]
                pub factor: Option<i32>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct ShiftTimeSync1Type {
                // for future use
                #[yaserde(attribute = true, rename = "Factor")]
                pub factor: Option<i32>,

                #[yaserde(attribute = true, rename = "Input")]
                pub input: Option<bool>,

                #[yaserde(attribute = true, rename = "OutputDelayTime")]
                pub output_delay_time: Option<i32>,

                #[yaserde(attribute = true, rename = "InputDelayTime")]
                pub input_delay_time: Option<i32>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct CycleTimeSync2Type {
                #[yaserde(attribute = true, rename = "Factor")]
                pub factor: Option<i32>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct ShiftTimeSync2Type {
                #[yaserde(attribute = true, rename = "Factor")]
                pub factor: Option<i32>,

                #[yaserde(attribute = true, rename = "Input")]
                pub input: Option<bool>,

                #[yaserde(attribute = true, rename = "OutputDelayTime")]
                pub output_delay_time: Option<i32>,

                #[yaserde(attribute = true, rename = "InputDelayTime")]
                pub input_delay_time: Option<i32>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct CycleTimeSync3Type {
                #[yaserde(attribute = true, rename = "Factor")]
                pub factor: Option<i32>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct ShiftTimeSync3Type {
                // for future use
                #[yaserde(attribute = true, rename = "Factor")]
                pub factor: Option<i32>,

                #[yaserde(attribute = true, rename = "Input")]
                pub input: Option<bool>,

                #[yaserde(attribute = true, rename = "OutputDelayTime")]
                pub output_delay_time: Option<i32>,

                #[yaserde(attribute = true, rename = "InputDelayTime")]
                pub input_delay_time: Option<i32>,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct SmType {
                // obsolete
                #[yaserde(rename = "SyncType")]
                pub sync_type: Option<i32>,

                // obsolete
                #[yaserde(rename = "CycleTime")]
                pub cycle_time: Option<sm_type::CycleTimeType>,

                // obsolete
                #[yaserde(rename = "ShiftTime")]
                pub shift_time: Option<sm_type::ShiftTimeType>,

                #[yaserde(rename = "Pdo")]
                pub pdo: Vec<sm_type::PdoType>,

                #[yaserde(attribute = true, rename = "No")]
                pub no: i32,
            }

            pub mod sm_type {
                use super::*;

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct CycleTimeType {
                    // obsolete
                    #[yaserde(attribute = true, rename = "Factor")]
                    pub factor: Option<i32>,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct ShiftTimeType {
                    // obsolete
                    #[yaserde(attribute = true, rename = "MinAfterSync")]
                    pub min_after_sync: Option<i32>,

                    // obsolete
                    #[yaserde(attribute = true, rename = "MinBeforeFrame")]
                    pub min_before_frame: Option<i32>,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct PdoType {
                    #[yaserde(attribute = true, rename = "OSFac")]
                    pub os_fac: Option<i32>,
                }
            }
        }
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct SlotsType {
        #[yaserde(rename = "Slot")]
        pub slot: Vec<SlotType>,

        #[yaserde(rename = "ModuleGroups")]
        pub module_groups: Option<slots_type::ModuleGroupsType>,

        #[yaserde(rename = "SlotGroupData")]
        pub slot_group_data: Vec<slots_type::SlotGroupDataType>,

        #[yaserde(rename = "ModulePdoGroup")]
        pub module_pdo_group: Vec<slots_type::ModulePdoGroupType>,

        #[yaserde(rename = "SlotSelections")]
        pub slot_selections: Vec<slots_type::SlotSelectionsType>,

        #[yaserde(attribute = true, rename = "DownloadModuleIdentList")]
        pub download_module_ident_list: Option<bool>,

        #[yaserde(attribute = true, rename = "DownloadModuleAddressList")]
        pub download_module_address_list: Option<bool>,

        #[yaserde(attribute = true, rename = "DownloadModuleListTransition")]
        pub download_module_list_transition: Option<String>,

        #[yaserde(attribute = true, rename = "MaxSlotCount")]
        pub max_slot_count: Option<String>,

        #[yaserde(attribute = true, rename = "MaxSlotGroupCount")]
        pub max_slot_group_count: Option<String>,

        #[yaserde(attribute = true, rename = "SlotPdoIncrement")]
        pub slot_pdo_increment: Option<String>,

        #[yaserde(attribute = true, rename = "SlotGroupPdoIncrement")]
        pub slot_group_pdo_increment: Option<String>,

        #[yaserde(attribute = true, rename = "SlotIndexIncrement")]
        pub slot_index_increment: Option<String>,

        #[yaserde(attribute = true, rename = "SlotGroupIndexIncrement")]
        pub slot_group_index_increment: Option<String>,

        // obsolete
        #[yaserde(attribute = true, rename = "IdentifyModuleBy")]
        pub identify_module_by: Option<String>,
    }

    pub mod slots_type {
        use super::*;

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct ModuleGroupsType {
            #[yaserde(rename = "ModuleGroup")]
            pub module_group: Vec<module_groups_type::ModuleGroupType>,
        }

        pub mod module_groups_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct ModuleGroupType {
                #[yaserde(rename = "Name")]
                pub name: String,

                #[yaserde(rename = "Type")]
                pub _type: String,

                #[yaserde(rename = "Class")]
                pub class: Option<String>,

                #[yaserde(rename = "ModuleIdent")]
                pub module_ident: Vec<String>,

                #[yaserde(rename = "ModuleGroupChoice")]
                pub module_group_choice: module_group_type::ModuleGroupChoice,

                #[yaserde(attribute = true, rename = "Id")]
                pub id: String,
            }

            pub mod module_group_type {
                use super::*;

                #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

                pub enum ModuleGroupChoice {
                    #[yaserde(rename = "ImageFile16x14")]
                    ImageFile16X14(Option<String>),
                    #[yaserde(rename = "ImageData16x14")]
                    ImageData16X14(Option<String>),
                    __Unknown__(String),
                }

                impl Default for ModuleGroupChoice {
                    fn default() -> ModuleGroupChoice {
                        Self::__Unknown__("No valid variants".into())
                    }
                }
            }
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct SlotGroupDataType {
            #[yaserde(rename = "Name")]
            pub name: Vec<String>,

            #[yaserde(rename = "ModuleGroups")]
            pub module_groups: Option<slot_group_data_type::ModuleGroupsType>,

            #[yaserde(rename = "SlotGroupDataChoice")]
            pub slot_group_data_choice: slot_group_data_type::SlotGroupDataChoice,

            #[yaserde(attribute = true, rename = "SlotGroup")]
            pub slot_group: String,
        }

        pub mod slot_group_data_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct ModuleGroupsType {
                #[yaserde(rename = "Id")]
                pub id: Vec<String>,
            }

            #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

            pub enum SlotGroupDataChoice {
                #[yaserde(rename = "ImageFile16x14")]
                ImageFile16X14(Option<String>),
                #[yaserde(rename = "ImageData16x14")]
                ImageData16X14(Option<String>),
                __Unknown__(String),
            }

            impl Default for SlotGroupDataChoice {
                fn default() -> SlotGroupDataChoice {
                    Self::__Unknown__("No valid variants".into())
                }
            }
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct ModulePdoGroupType {
            #[yaserde(attribute = true, rename = "Alignment")]
            pub alignment: Option<i32>,

            #[yaserde(attribute = true, rename = "RxPdo")]
            pub rx_pdo: Option<String>,

            #[yaserde(attribute = true, rename = "TxPdo")]
            pub tx_pdo: Option<String>,
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct SlotSelectionsType {
            #[yaserde(rename = "Name")]
            pub name: String,

            #[yaserde(rename = "ModuleIdent")]
            pub module_ident: Vec<String>,
        }
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct Esctype {
        #[yaserde(rename = "Reg0108")]
        pub reg_0108: Option<String>,

        #[yaserde(rename = "Reg0400")]
        pub reg_0400: Option<String>,

        #[yaserde(rename = "Reg0410")]
        pub reg_0410: Option<String>,

        #[yaserde(rename = "Reg0420")]
        pub reg_0420: Option<String>,

        #[yaserde(rename = "VendorSpecific")]
        pub vendor_specific: Option<String>,
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct EepromType {
        #[yaserde(attribute = true, rename = "AssignToPdi")]
        pub assign_to_pdi: Option<bool>,

        #[yaserde(rename = "EepromTypeChoice")]
        pub eeprom_type_choice: eeprom_type::EepromTypeChoice,

        #[yaserde(rename = "VendorSpecific")]
        pub vendor_specific: Option<String>,
    }

    pub mod eeprom_type {
        use super::*;

        #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

        pub enum EepromTypeChoice {
            Data(String),
            __Unknown__(String),
        }

        impl Default for EepromTypeChoice {
            fn default() -> EepromTypeChoice {
                Self::__Unknown__("No valid variants".into())
            }
        }
    }

    #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

    pub enum DeviceTypeChoice {
        // obsolete
        #[yaserde(rename = "Image16x14")]
        Image16X14(Option<String>),
        #[yaserde(rename = "ImageFile16x14")]
        ImageFile16X14(Option<String>),
        #[yaserde(rename = "ImageData16x14")]
        ImageData16X14(Option<String>),
        __Unknown__(String),
    }

    impl Default for DeviceTypeChoice {
        fn default() -> DeviceTypeChoice {
            Self::__Unknown__("No valid variants".into())
        }
    }
}

#[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
pub struct GroupType {
    #[yaserde(rename = "Type")]
    pub _type: String,

    #[yaserde(rename = "Name")]
    pub name: Vec<String>,

    #[yaserde(rename = "Comment")]
    pub comment: Vec<String>,

    #[yaserde(rename = "GroupTypeChoice")]
    pub group_type_choice: group_type::GroupTypeChoice,

    #[yaserde(rename = "VendorSpecific")]
    pub vendor_specific: Option<String>,
}

pub mod group_type {
    use super::*;

    #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

    pub enum GroupTypeChoice {
        // obsolete
        #[yaserde(rename = "Image16x14")]
        Image16X14(Option<String>),
        #[yaserde(rename = "ImageFile16x14")]
        ImageFile16X14(Option<String>),
        #[yaserde(rename = "ImageData16x14")]
        ImageData16X14(Option<String>),
        __Unknown__(String),
    }

    impl Default for GroupTypeChoice {
        fn default() -> GroupTypeChoice {
            Self::__Unknown__("No valid variants".into())
        }
    }
}

#[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
pub struct EepromType {
    #[yaserde(rename = "EepromTypeChoice")]
    pub eeprom_type_choice: eeprom_type::EepromTypeChoice,

    #[yaserde(rename = "VendorSpecific")]
    pub vendor_specific: Option<String>,
}

pub mod eeprom_type {
    use super::*;

    #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

    pub enum EepromTypeChoice {
        Data(String),
        __Unknown__(String),
    }

    impl Default for EepromTypeChoice {
        fn default() -> EepromTypeChoice {
            Self::__Unknown__("No valid variants".into())
        }
    }
}

#[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
pub struct InfoType {
    #[yaserde(rename = "Electrical")]
    pub electrical: Option<info_type::ElectricalType>,

    #[yaserde(rename = "StateMachine")]
    pub state_machine: Option<info_type::StateMachineType>,

    #[yaserde(rename = "Mailbox")]
    pub mailbox: Option<info_type::MailboxType>,

    #[yaserde(rename = "EtherCATController")]
    pub ether_cat_controller: Option<info_type::EtherCATControllerType>,

    #[yaserde(rename = "Port")]
    pub port: Vec<info_type::PortType>,

    #[yaserde(rename = "ExecutionUnit")]
    pub execution_unit: Vec<info_type::ExecutionUnitType>,

    #[yaserde(rename = "VendorSpecific")]
    pub vendor_specific: Option<String>,

    // obsolete
    #[yaserde(rename = "StationAliasSupported")]
    pub station_alias_supported: Option<info_type::String>,

    #[yaserde(rename = "IdentificationAdo")]
    pub identification_ado: Option<String>,

    #[yaserde(rename = "IdentificationReg134")]
    pub identification_reg_134: Option<bool>,

    // for future use
    #[yaserde(rename = "DeviceFeature")]
    pub device_feature: Vec<info_type::DeviceFeatureType>,
}

pub mod info_type {
    use super::*;

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct ElectricalType {
        // Set Value = 0, in case of only EtherCATp is needed
        #[yaserde(rename = "EBusCurrent")]
        pub e_bus_current: i32,

        #[yaserde(rename = "EtherCATp")]
        pub ether_ca_tp: Option<electrical_type::EtherCATpType>,
    }

    pub mod electrical_type {
        use super::*;

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct EtherCATpType {
            #[yaserde(rename = "Device")]
            pub device: ether_ca_tp_type::DeviceType,

            // Externe Power Einspeisung
            #[yaserde(rename = "PowerSupply")]
            pub power_supply: Option<ether_ca_tp_type::PowerSupplyType>,
        }

        pub mod ether_ca_tp_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct DeviceType {
                #[yaserde(rename = "Us")]
                pub us: device_type::UsType,

                #[yaserde(rename = "Up")]
                pub up: Option<device_type::UpType>,
            }

            pub mod device_type {
                use super::*;

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct UsType {
                    // If power comes from PowerSupply instead of Port 0
                    #[yaserde(rename = "PowerSupply")]
                    pub power_supply: Option<bool>,

                    // wenn nicht angegeben, wird die MinVoltage von 20,4 V angenommen
                    #[yaserde(rename = "MinVoltage")]
                    pub min_voltage: Option<us_type::MinVoltageType>,

                    // max. Stromverbrauch bei 24V
                    #[yaserde(rename = "Current")]
                    pub current: Option<us_type::CurrentType>,

                    // falls vorhanden wird Us nach aussen geführt
                    #[yaserde(rename = "External")]
                    pub external: Option<us_type::ExternalType>,
                }

                pub mod us_type {
                    use super::*;

                    #[derive(Default, Clone, PartialEq, Debug)]
                    pub struct MinVoltageType(pub f64);

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct CurrentType {
                        // load characteristics
                        #[yaserde(attribute = true, rename = "Type")]
                        pub _type: Option<String>,
                    }

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct ExternalType {
                        #[yaserde(rename = "Channel")]
                        pub channel: Vec<external_type::ChannelType>,
                    }

                    pub mod external_type {
                        use super::*;

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct ChannelType {
                            #[yaserde(rename = "Name")]
                            pub name: Vec<String>,

                            // muss angegeben werden wenn die Spannung ungleich Us ist , e.g. 5V.
                            // Falls nichts angegeben wird, wird Us untranfsormiert ausgegeben.
                            #[yaserde(rename = "Voltage")]
                            pub voltage: Option<channel_type::VoltageType>,
                        }

                        pub mod channel_type {
                            use super::*;

                            #[derive(
                                Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize,
                            )]
                            pub struct VoltageType {
                                // load characteristics (default_ switching regulator)
                                #[yaserde(attribute = true, rename = "Type")]
                                pub _type: Option<String>,
                            }
                        }
                    }
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct UpType {
                    // If power comes from PowerSupply instead of Port 0
                    #[yaserde(rename = "PowerSupply")]
                    pub power_supply: Option<bool>,

                    // wenn nicht angegeben, wird die MinVoltage von 20,4 V angenommen
                    #[yaserde(rename = "MinVoltage")]
                    pub min_voltage: Option<f64>,

                    // max. Stromverbrauch bei 24V
                    #[yaserde(rename = "Current")]
                    pub current: Option<up_type::CurrentType>,

                    // falls vorhanden wird Up nach aussen geführt
                    #[yaserde(rename = "External")]
                    pub external: Option<up_type::ExternalType>,
                }

                pub mod up_type {
                    use super::*;

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct CurrentType {
                        // load characteristics
                        #[yaserde(attribute = true, rename = "Type")]
                        pub _type: Option<String>,
                    }

                    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                    pub struct ExternalType {
                        #[yaserde(rename = "Channel")]
                        pub channel: Vec<external_type::ChannelType>,
                    }

                    pub mod external_type {
                        use super::*;

                        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                        pub struct ChannelType {
                            #[yaserde(rename = "Name")]
                            pub name: Vec<String>,

                            // muss angegeben werden wenn die Spannung ungleich Up ist , e.g. 5V.
                            // Falls nichts angegeben wird, wird Us untranfsormiert ausgegeben.
                            #[yaserde(rename = "Voltage")]
                            pub voltage: Option<channel_type::VoltageType>,
                        }

                        pub mod channel_type {
                            use super::*;

                            #[derive(
                                Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize,
                            )]
                            pub struct VoltageType {
                                // load characteristics (default_ switching regulator)
                                #[yaserde(attribute = true, rename = "Type")]
                                pub _type: Option<String>,
                            }
                        }
                    }
                }
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct PowerSupplyType {
                #[yaserde(rename = "Us")]
                pub us: Option<power_supply_type::UsType>,

                #[yaserde(rename = "Up")]
                pub up: Option<power_supply_type::UpType>,
            }

            pub mod power_supply_type {
                use super::*;

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct UsType {
                    #[yaserde(rename = "MaxCurrent")]
                    pub max_current: f64,
                }

                #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
                pub struct UpType {
                    #[yaserde(rename = "MaxCurrent")]
                    pub max_current: f64,
                }
            }
        }
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct StateMachineType {
        #[yaserde(rename = "Timeout")]
        pub timeout: Option<state_machine_type::TimeoutType>,

        #[yaserde(rename = "Behavior")]
        pub behavior: Option<state_machine_type::BehaviorType>,
    }

    pub mod state_machine_type {
        use super::*;

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct TimeoutType {
            #[yaserde(rename = "PreopTimeout")]
            pub preop_timeout: i32,

            #[yaserde(rename = "SafeopOpTimeout")]
            pub safeop_op_timeout: i32,

            #[yaserde(rename = "BackToInitTimeout")]
            pub back_to_init_timeout: i32,

            #[yaserde(rename = "BackToSafeopTimeout")]
            pub back_to_safeop_timeout: i32,
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct BehaviorType {
            #[yaserde(attribute = true, rename = "StartToInit")]
            pub start_to_init: Option<bool>,

            #[yaserde(attribute = true, rename = "StartToPreop")]
            pub start_to_preop: Option<bool>,

            #[yaserde(attribute = true, rename = "StartToSafeop")]
            pub start_to_safeop: Option<bool>,

            #[yaserde(attribute = true, rename = "StartToSafeopNoSync")]
            pub start_to_safeop_no_sync: Option<bool>,
        }
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct MailboxType {
        #[yaserde(rename = "Timeout")]
        pub timeout: mailbox_type::TimeoutType,
    }

    pub mod mailbox_type {
        use super::*;

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct TimeoutType {
            #[yaserde(rename = "RequestTimeout")]
            pub request_timeout: i32,

            #[yaserde(rename = "ResponseTimeout")]
            pub response_timeout: i32,
        }
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct EtherCATControllerType {
        #[yaserde(rename = "DpramSize")]
        pub dpram_size: Option<i32>,

        #[yaserde(rename = "SmCount")]
        pub sm_count: Option<i32>,

        #[yaserde(rename = "FmmuCount")]
        pub fmmu_count: Option<i32>,

        #[yaserde(rename = "DcSyncCount")]
        pub dc_sync_count: Option<i32>,

        #[yaserde(rename = "DcLatchCount")]
        pub dc_latch_count: Option<i32>,
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct PortType {
        #[yaserde(rename = "Type")]
        pub _type: port_type::TypeType,

        #[yaserde(rename = "Connector")]
        pub connector: Vec<port_type::ConnectorType>,

        #[yaserde(rename = "Label")]
        pub label: Option<String>,

        // For future use
        #[yaserde(rename = "RxDelay")]
        pub rx_delay: Option<i32>,

        // For future use
        #[yaserde(rename = "TxDelay")]
        pub tx_delay: Option<i32>,

        #[yaserde(rename = "PhysicalPhyAddr")]
        pub physical_phy_addr: Option<i32>,

        #[yaserde(rename = "EtherCATp")]
        pub ether_ca_tp: Option<port_type::EtherCATpType>,
    }

    pub mod port_type {
        use super::*;

        #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

        pub enum TypeType {
            #[yaserde(rename = "MII")]
            Mii,
            #[yaserde(rename = "EBUS")]
            Ebus,
            #[yaserde(rename = "NONE")]
            None,
            __Unknown__(String),
        }

        impl Default for TypeType {
            fn default() -> TypeType {
                Self::__Unknown__("No valid variants".into())
            }
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct ConnectorType {
            #[yaserde(attribute = true, rename = "VendorId")]
            pub vendor_id: Option<String>,
        }

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct EtherCATpType {
            #[yaserde(rename = "Us")]
            pub us: Option<ether_ca_tp_type::UsType>,

            #[yaserde(rename = "Up")]
            pub up: Option<ether_ca_tp_type::UpType>,
        }

        pub mod ether_ca_tp_type {
            use super::*;

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct UsType {
                // If power comes from PowerSupply instead of Port 0
                #[yaserde(rename = "PowerSupply")]
                pub power_supply: Option<bool>,

                // Default 3A
                #[yaserde(rename = "MaxCurrent")]
                pub max_current: Option<f64>,

                #[yaserde(rename = "Resistance")]
                pub resistance: f64,
            }

            #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
            pub struct UpType {
                // If power comes from PowerSupply instead of Port 0
                #[yaserde(rename = "PowerSupply")]
                pub power_supply: Option<bool>,

                // Default 3A
                #[yaserde(rename = "MaxCurrent")]
                pub max_current: Option<f64>,

                #[yaserde(rename = "Resistance")]
                pub resistance: f64,
            }
        }
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct ExecutionUnitType {
        #[yaserde(rename = "Type")]
        pub _type: execution_unit_type::TypeType,

        // For future use
        #[yaserde(rename = "RxDelay")]
        pub rx_delay: Option<i32>,

        // For future use
        #[yaserde(rename = "TxDelay")]
        pub tx_delay: Option<i32>,
    }

    pub mod execution_unit_type {
        use super::*;

        #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

        pub enum TypeType {
            #[yaserde(rename = "PRIMARY")]
            Primary,
            #[yaserde(rename = "SECONDARY")]
            Secondary,
            #[yaserde(rename = "NONE")]
            None,
            __Unknown__(String),
        }

        impl Default for TypeType {
            fn default() -> TypeType {
                Self::__Unknown__("No valid variants".into())
            }
        }
    }

    #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
    pub struct DeviceFeatureType {
        // for future use
        #[yaserde(rename = "Name")]
        pub name: String,

        // for future use
        #[yaserde(rename = "Value")]
        pub value: Option<String>,

        // for future use
        #[yaserde(rename = "Description")]
        pub description: Option<String>,

        // for future use
        #[yaserde(rename = "Register")]
        pub register: Vec<device_feature_type::RegisterType>,
    }

    pub mod device_feature_type {
        use super::*;

        #[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
        pub struct RegisterType {
            // for future use;
            // in bytes
            #[yaserde(rename = "StartAddress")]
            pub start_address: i32,

            // for future use;
            // in bytes
            #[yaserde(rename = "Length")]
            pub length: i32,

            // for future use
            #[yaserde(rename = "BitMask")]
            pub bit_mask: Option<String>,
        }
    }
}

#[derive(Default, Clone, PartialEq, Debug, YaSerialize, YaDeserialize)]
pub struct SlotType {
    #[yaserde(rename = "Name")]
    pub name: Vec<String>,

    #[yaserde(rename = "SlotTypeChoice")]
    pub slot_type_choice: slot_type::SlotTypeChoice,

    #[yaserde(rename = "SlotTypeChoice")]
    pub slot_type_choice: slot_type::SlotTypeChoice,

    #[yaserde(attribute = true, rename = "SlotGroup")]
    pub slot_group: Option<String>,

    #[yaserde(attribute = true, rename = "MinInstances")]
    pub min_instances: String,

    #[yaserde(attribute = true, rename = "MaxInstances")]
    pub max_instances: String,

    #[yaserde(attribute = true, rename = "SlotPdoIncrement")]
    pub slot_pdo_increment: Option<String>,

    #[yaserde(attribute = true, rename = "SlotGroupPdoIncrement")]
    pub slot_group_pdo_increment: Option<String>,

    #[yaserde(attribute = true, rename = "SlotIndexIncrement")]
    pub slot_index_increment: Option<String>,

    #[yaserde(attribute = true, rename = "SlotGroupIndexIncrement")]
    pub slot_group_index_increment: Option<String>,

    #[yaserde(attribute = true, rename = "TreeView")]
    pub tree_view: Option<String>,

    #[yaserde(attribute = true, rename = "Default")]
    pub default: Option<String>,

    // automatically plugged when no other module is plugged to this slot
    #[yaserde(attribute = true, rename = "Fallback")]
    pub fallback: Option<String>,
}

pub mod slot_type {
    use super::*;

    #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

    pub enum SlotTypeChoice {
        ModuleIdent,
        ModuleClass,
        __Unknown__(String),
    }

    impl Default for SlotTypeChoice {
        fn default() -> SlotTypeChoice {
            Self::__Unknown__("No valid variants".into())
        }
    }

    #[derive(PartialEq, Debug, Clone, YaSerialize, YaDeserialize)]

    pub enum SlotTypeChoice {
        // obsolete
        #[yaserde(rename = "Image16x14")]
        Image16X14(Option<String>),
        #[yaserde(rename = "ImageFile16x14")]
        ImageFile16X14(Option<String>),
        #[yaserde(rename = "ImageData16x14")]
        ImageData16X14(Option<String>),
        __Unknown__(String),
    }

    impl Default for SlotTypeChoice {
        fn default() -> SlotTypeChoice {
            Self::__Unknown__("No valid variants".into())
        }
    }
}
