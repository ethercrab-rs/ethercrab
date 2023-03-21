//! Copied from SmolTCP's RawSocketDesc, with inspiration from
//! [https://github.com/embassy-rs/embassy](https://github.com/embassy-rs/embassy/blob/master/examples/std/src/tuntap.rs).

use std::os::unix::io::{AsRawFd, RawFd};
use std::{io, mem};

#[repr(C)]
#[derive(Debug)]
struct ifreq {
    ifr_name: [libc::c_char; libc::IF_NAMESIZE],
    ifr_data: libc::c_int, /* ifr_ifindex or ifr_mtu */
}

#[derive(Debug)]
pub struct RawSocketDesc {
    protocol: libc::c_short,
    lower: libc::c_int,
    ifreq: ifreq,
}

impl RawSocketDesc {
    pub fn new(name: &str) -> io::Result<RawSocketDesc> {
        let protocol = libc::ETH_P_ALL as i16;

        let lower = unsafe {
            let lower = libc::socket(
                // Ethernet II frames
                libc::AF_PACKET,
                libc::SOCK_RAW | libc::SOCK_NONBLOCK,
                // Receive all protocols
                protocol.to_be() as i32,
            );
            if lower == -1 {
                return Err(io::Error::last_os_error());
            }
            lower
        };

        let mut self_ = RawSocketDesc {
            protocol,
            lower,
            ifreq: ifreq_for(name),
        };

        self_.bind_interface()?;

        Ok(self_)
    }

    fn bind_interface(&mut self) -> io::Result<()> {
        let sockaddr = libc::sockaddr_ll {
            sll_family: libc::AF_PACKET as u16,
            sll_protocol: self.protocol.to_be() as u16,
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
        self.lower
    }
}

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
