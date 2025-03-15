//! Basic ESI file parsing.

#[derive(PartialEq, Debug, yaserde::YaDeserialize)]
#[yaserde(rename = "device")]
pub struct EtherCatInfo {
    #[yaserde(rename = "Vendor")]
    pub vendor: Vendor,
    #[yaserde(rename = "Devices")]
    pub devices: Vec<Device>,
}

#[derive(PartialEq, Debug, yaserde::YaDeserialize)]
pub struct Vendor {
    #[yaserde(rename = "Name")]
    pub name: String,
}

#[derive(PartialEq, Debug, yaserde::YaDeserialize)]
pub struct Device {
    #[yaserde(rename = "Name")]
    pub name: String,
}

/// Windows language codes, specified
/// [here](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-lcid/70feba9f-294e-491e-b6eb-56532684c37f).
#[derive(PartialEq, Default, Debug, yaserde::YaDeserialize)]
pub enum LanguageCode {
    /// US English, 1033d, 0x0409.
    #[default]
    EnUs = 1033,
    /// German, 1031d, 0x007.
    De = 1031,
}
