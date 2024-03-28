//! Copied from SmolTCP's RawSocketDesc, with inspiration from
//! [https://github.com/embassy-rs/embassy](https://github.com/embassy-rs/embassy/blob/master/examples/std/src/tuntap.rs).

use crate::ETHERCAT_ETHERTYPE_RAW;
use async_io::IoSafe;
use rustix::{
    fd::OwnedFd,
    ioctl::{Opcode, RawOpcode},
    net::{AddressFamily, Protocol, RawProtocol, SocketFlags, SocketType},
};
use std::{
    io, mem,
    os::{
        fd::{AsFd, BorrowedFd},
        unix::io::{AsRawFd, RawFd},
    },
};

pub struct RawSocketDesc {
    /// Interface descriptor.
    lower: OwnedFd,
    /// Interface name.
    if_name: String,
}

impl RawSocketDesc {
    pub fn new(name: &str) -> io::Result<Self> {
        let lower = rustix::net::socket_with(
            AddressFamily::PACKET,
            SocketType::RAW,
            SocketFlags::NONBLOCK,
            Some(Protocol::from_raw(
                // SAFETY: EtherCAT protocol is 0x88a4. If you've set the constant to 0, what is
                // wrong with you?
                unsafe { RawProtocol::new_unchecked(ETHERCAT_ETHERTYPE_RAW.into()) },
            )),
        )?;

        let mut self_ = RawSocketDesc {
            lower,
            if_name: name.to_string(),
        };

        self_.bind_interface()?;

        Ok(self_)
    }

    fn bind_interface(&mut self) -> io::Result<()> {
        let protocol = ETHERCAT_ETHERTYPE_RAW as i16;

        let if_index = rustix::net::netdevice::name_to_index(&self.lower, &self.if_name)?
            .try_into()
            .map_err(|e| io::Error::other(e))?;

        let sockaddr = libc::sockaddr_ll {
            sll_family: libc::AF_PACKET as u16,
            sll_protocol: protocol.to_be() as u16,
            sll_ifindex: if_index,
            sll_hatype: 1,
            sll_pkttype: 0,
            sll_halen: 6,
            sll_addr: [0; 8],
        };

        unsafe {
            #[allow(trivial_casts)]
            let res = libc::bind(
                self.lower.as_raw_fd(),
                &sockaddr as *const libc::sockaddr_ll as *const libc::sockaddr,
                mem::size_of::<libc::sockaddr_ll>() as libc::socklen_t,
            );

            if res == -1 {
                return Err(io::Error::last_os_error());
            }
        }

        Ok(())
    }

    pub fn interface_mtu(&mut self) -> io::Result<usize> {
        let mtu = unsafe { rustix::ioctl::ioctl(&self.lower, IoctlMtu::new(&self.if_name))? };

        usize::try_from(mtu).map_err(|e| io::Error::other(e))
    }
}

impl AsRawFd for RawSocketDesc {
    fn as_raw_fd(&self) -> RawFd {
        self.lower.as_raw_fd()
    }
}

impl AsFd for RawSocketDesc {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.lower.as_fd()
    }
}

// SAFETY: Implementing this trait pledges that the underlying socket resource will not be dropped
// by `Read` or `Write` impls. More information can be read
// [here](https://docs.rs/async-io/latest/async_io/trait.IoSafe.html).
unsafe impl IoSafe for RawSocketDesc {}

impl io::Read for RawSocketDesc {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = unsafe {
            libc::read(
                self.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
            )
        };
        if len == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(len as usize)
        }
    }
}

impl io::Write for RawSocketDesc {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = unsafe {
            libc::write(
                self.as_raw_fd(),
                buf.as_ptr() as *mut libc::c_void,
                buf.len(),
            )
        };
        if len == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(len as usize)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[repr(transparent)]
struct IoctlMtu(libc::ifreq);

impl IoctlMtu {
    fn new(if_name: &str) -> Self {
        let mut ifreq = libc::ifreq {
            ifr_name: [0; libc::IF_NAMESIZE],
            ifr_ifru: libc::__c_anonymous_ifr_ifru { ifru_mtu: 0 },
        };

        for (i, byte) in if_name.as_bytes().iter().enumerate() {
            ifreq.ifr_name[i] = *byte as libc::c_char
        }

        Self(ifreq)
    }
}

unsafe impl rustix::ioctl::Ioctl for IoctlMtu {
    type Output = i32;

    const OPCODE: Opcode = Opcode::old(libc::SIOCGIFMTU as RawOpcode);

    const IS_MUTATING: bool = true;

    fn as_ptr(&mut self) -> *mut libc::c_void {
        #[allow(trivial_casts)]
        {
            (&mut self.0) as *const _ as *mut _
        }
    }

    unsafe fn output_from_ptr(
        _out: rustix::ioctl::IoctlOutput,
        extract_output: *mut libc::c_void,
    ) -> rustix::io::Result<Self::Output> {
        let result = extract_output.cast::<libc::ifreq>().read();

        Ok(result.ifr_ifru.ifru_mtu)
    }
}
