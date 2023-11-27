//! Copied from SmolTCP's RawSocketDesc, with inspiration from
//! [https://github.com/embassy-rs/embassy](https://github.com/embassy-rs/embassy/blob/master/examples/std/src/tuntap.rs).

use async_io::IoSafe;
use rustix::net::{AddressFamily, Protocol, RawProtocol, SocketFlags, SocketType};

use crate::{ETHERCAT_ETHERTYPE, ETHERCAT_ETHERTYPE_RAW};
use core::num::NonZeroU32;
use std::{
    io, mem,
    os::{
        fd::{AsFd, BorrowedFd},
        unix::io::{AsRawFd, RawFd},
    },
};

#[repr(C)]
#[derive(Debug)]
struct ifreq {
    ifr_name: [libc::c_char; libc::IF_NAMESIZE],
    ifr_data: libc::c_int, /* ifr_ifindex or ifr_mtu */
}

#[derive(Debug)]
pub struct RawSocketDesc {
    ifreq: ifreq,
    sock: rustix::fd::OwnedFd,
}

impl RawSocketDesc {
    pub fn new(name: &str) -> io::Result<Self> {
        let sock = rustix::net::socket_with(
            AddressFamily::PACKET,
            SocketType::RAW,
            SocketFlags::NONBLOCK,
            Some(Protocol::from_raw(
                RawProtocol::try_from(ETHERCAT_ETHERTYPE_RAW).unwrap(),
            )),
        )?;

        let mut self_ = Self {
            ifreq: ifreq_for(name),
            sock,
        };

        self_.bind_interface()?;

        Ok(self_)
    }

    fn bind_interface(&mut self) -> io::Result<()> {
        let sockaddr = libc::sockaddr_ll {
            sll_family: libc::AF_PACKET as u16,
            sll_protocol: ETHERCAT_ETHERTYPE_RAW.get().to_be(),
            sll_ifindex: ifreq_ioctl(self.sock.as_raw_fd(), &mut self.ifreq, libc::SIOCGIFINDEX)?,
            sll_hatype: 1,
            sll_pkttype: 0,
            sll_halen: 6,
            sll_addr: [0; 8],
        };

        unsafe {
            #[allow(trivial_casts)]
            let res = libc::bind(
                self.sock.as_raw_fd(),
                &sockaddr as *const libc::sockaddr_ll as *const libc::sockaddr,
                mem::size_of::<libc::sockaddr_ll>() as libc::socklen_t,
            );
            if res == -1 {
                return Err(io::Error::last_os_error());
            }
        }

        Ok(())
    }

    // NOTE: Leave these around in case we need them in the future.

    // pub fn interface_mtu(&mut self) -> io::Result<usize> {
    //     ifreq_ioctl(self.lower, &mut self.ifreq, libc::SIOCGIFMTU).map(|mtu| mtu as usize)
    // }

    // pub fn recv(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
    //     unsafe {
    //         let len = libc::recv(
    //             self.lower,
    //             buffer.as_mut_ptr() as *mut libc::c_void,
    //             buffer.len(),
    //             0,
    //         );

    //         if len == -1 {
    //             return Err(io::Error::last_os_error());
    //         }

    //         Ok(len as usize)
    //     }
    // }

    // pub fn send(&mut self, buffer: &[u8]) -> io::Result<usize> {
    //     unsafe {
    //         let len = libc::send(
    //             self.lower,
    //             buffer.as_ptr() as *const libc::c_void,
    //             buffer.len(),
    //             0,
    //         );

    //         if len == -1 {
    //             return Err(io::Error::last_os_error());
    //         }

    //         Ok(len as usize)
    //     }
    // }
}

impl AsRawFd for RawSocketDesc {
    fn as_raw_fd(&self) -> RawFd {
        self.sock.as_raw_fd()
    }
}

impl AsFd for RawSocketDesc {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // unsafe { BorrowedFd::borrow_raw(self.lower) }
        self.sock.as_fd()
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

fn ifreq_ioctl(
    lower: libc::c_int,
    ifreq: &mut ifreq,
    cmd: libc::c_ulong,
) -> io::Result<libc::c_int> {
    unsafe {
        #[allow(trivial_casts)]
        let res = libc::ioctl(lower, cmd, ifreq as *mut ifreq);

        if res == -1 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(ifreq.ifr_data)
}

fn ifreq_for(name: &str) -> ifreq {
    let mut ifreq = ifreq {
        ifr_name: [0; libc::IF_NAMESIZE],
        ifr_data: 0,
    };
    for (i, byte) in name.as_bytes().iter().enumerate() {
        ifreq.ifr_name[i] = *byte as libc::c_char
    }
    ifreq
}
