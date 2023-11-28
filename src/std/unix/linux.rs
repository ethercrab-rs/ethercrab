//! Copied from SmolTCP's RawSocketDesc, with inspiration from
//! [https://github.com/embassy-rs/embassy](https://github.com/embassy-rs/embassy/blob/master/examples/std/src/tuntap.rs).

use crate::{
    std::unix::{ifreq, ifreq_for},
    ETHERCAT_ETHERTYPE_RAW,
};
use async_io::IoSafe;
use std::{
    io, mem,
    os::{
        fd::{AsFd, BorrowedFd},
        unix::io::{AsRawFd, RawFd},
    },
};

pub struct RawSocketDesc {
    lower: i32,
    ifreq: ifreq,
}

impl RawSocketDesc {
    pub fn new(name: &str) -> io::Result<Self> {
        let protocol = ETHERCAT_ETHERTYPE_RAW as i16;

        let lower = unsafe {
            let lower = libc::socket(
                // Ethernet II frames
                libc::AF_PACKET,
                libc::SOCK_RAW | libc::SOCK_NONBLOCK,
                protocol.to_be() as i32,
            );
            if lower == -1 {
                return Err(io::Error::last_os_error());
            }
            lower
        };

        let mut self_ = RawSocketDesc {
            lower,
            ifreq: ifreq_for(name),
        };

        self_.bind_interface()?;

        Ok(self_)
    }

    fn bind_interface(&mut self) -> io::Result<()> {
        let protocol = ETHERCAT_ETHERTYPE_RAW as i16;

        let sockaddr = libc::sockaddr_ll {
            sll_family: libc::AF_PACKET as u16,
            sll_protocol: protocol.to_be() as u16,
            sll_ifindex: ifreq_ioctl(self.lower, &mut self.ifreq, libc::SIOCGIFINDEX)?,
            sll_hatype: 1,
            sll_pkttype: 0,
            sll_halen: 6,
            sll_addr: [0; 8],
        };

        unsafe {
            #[allow(trivial_casts)]
            let res = libc::bind(
                self.lower,
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
        ifreq_ioctl(self.lower, &mut self.ifreq, libc::SIOCGIFMTU).map(|mtu| mtu as usize)
    }
}

impl AsRawFd for RawSocketDesc {
    fn as_raw_fd(&self) -> RawFd {
        self.lower
    }
}

impl AsFd for RawSocketDesc {
    fn as_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(self.lower) }
    }
}

// SAFETY: Implementing this trait pledges that the underlying socket resource will not be dropped
// by `Read` or `Write` impls. More information can be read
// [here](https://docs.rs/async-io/latest/async_io/trait.IoSafe.html).
unsafe impl IoSafe for RawSocketDesc {}

impl Drop for RawSocketDesc {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.lower);
        }
    }
}

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
