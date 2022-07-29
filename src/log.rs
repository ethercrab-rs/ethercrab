//! A logging shim to support both `log`-compatible crates in `std` environments, as well as `defmt`
//! in `no_std`.

#![macro_use]
#![allow(unused_macros)]

macro_rules! trace {
    ($s:literal $(, $x:expr)* $(,)?) => {
        {
            #[cfg(feature = "std")]
            ::log::trace!($s $(, $x)*);
            #[cfg(not(feature = "std"))]
            ::defmt::trace!($s $(, $x)*);
        }
    };
}

macro_rules! debug {
    ($s:literal $(, $x:expr)* $(,)?) => {
        {
            #[cfg(feature = "std")]
            ::log::debug!($s $(, $x)*);
            #[cfg(not(feature = "std"))]
            ::defmt::debug!($s $(, $x)*);
        }
    };
}

macro_rules! info {
    ($s:literal $(, $x:expr)* $(,)?) => {
        {
            #[cfg(feature = "std")]
            ::log::info!($s $(, $x)*);
            #[cfg(not(feature = "std"))]
            ::defmt::info!($s $(, $x)*);
        }
    };
}

macro_rules! warn {
    ($s:literal $(, $x:expr)* $(,)?) => {
        {
            #[cfg(feature = "std")]
            ::log::warn!($s $(, $x)*);
            #[cfg(not(feature = "std"))]
            ::defmt::warn!($s $(, $x)*);
        }
    };
}

macro_rules! error {
    ($s:literal $(, $x:expr)* $(,)?) => {
        {
            #[cfg(feature = "std")]
            ::log::error!($s $(, $x)*);
            #[cfg(not(feature = "std"))]
            ::defmt::error!($s $(, $x)*);
        }
    };
}
