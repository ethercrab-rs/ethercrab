//! macOS and OpenBSD raw socket using BPF.
//!
//! Copied from SmolTCP's `BpfDevice` with a few extra trait implementations. Thank you, SmolTCP
//! maintainers!

use crate::std::unix::{ifreq, ifreq_for};
use async_io::IoSafe;
use libc;
use smoltcp::wire::ETHERNET_HEADER_LEN;
use std::{
    io, mem,
    os::unix::io::{AsFd, AsRawFd, BorrowedFd, RawFd},
};

/// set interface
#[cfg(any(target_os = "macos", target_os = "openbsd"))]
const BIOCSETIF: libc::c_ulong = 0x8020426c;
/// get buffer length
#[cfg(any(target_os = "macos", target_os = "openbsd"))]
const BIOCGBLEN: libc::c_ulong = 0x40044266;
/// set immediate/nonblocking read
#[cfg(any(target_os = "macos", target_os = "openbsd"))]
const BIOCIMMEDIATE: libc::c_ulong = 0x80044270;
/// set bpf_hdr struct size
#[cfg(target_os = "macos")]
const SIZEOF_BPF_HDR: usize = 18;
/// set bpf_hdr struct size
#[cfg(target_os = "openbsd")]
const SIZEOF_BPF_HDR: usize = 24;
/// The actual header length may be larger than the bpf_hdr struct due to aligning
/// see https://github.com/openbsd/src/blob/37ecb4d066e5566411cc16b362d3960c93b1d0be/sys/net/bpf.c#L1649
/// and https://github.com/apple/darwin-xnu/blob/8f02f2a044b9bb1ad951987ef5bab20ec9486310/bsd/net/bpf.c#L3580
#[cfg(any(target_os = "macos", target_os = "openbsd"))]
const BPF_HDRLEN: usize = (((SIZEOF_BPF_HDR + ETHERNET_HEADER_LEN) + mem::align_of::<u32>() - 1)
    & !(mem::align_of::<u32>() - 1))
    - ETHERNET_HEADER_LEN;

#[cfg_attr(not(unix), allow(unused_macros))]
macro_rules! try_ioctl {
    ($fd:expr,$cmd:expr,$req:expr) => {
        unsafe {
            if libc::ioctl($fd, $cmd, $req) == -1 {
                return Err(io::Error::last_os_error());
            }
        }
    };
}

#[derive(Debug)]
pub struct BpfDevice {
    fd: libc::c_int,
    ifreq: ifreq,
}

impl AsRawFd for BpfDevice {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl AsFd for BpfDevice {
    fn as_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(self.fd) }
    }
}

fn open_device() -> io::Result<libc::c_int> {
    unsafe {
        for i in 0..256 {
            let dev = format!("/dev/bpf{}\0", i);
            match libc::open(
                dev.as_ptr() as *const libc::c_char,
                libc::O_RDWR | libc::O_NONBLOCK,
            ) {
                -1 => continue,
                fd => return Ok(fd),
            };
        }
    }
    // at this point, all 256 BPF devices were busy and we weren't able to open any
    Err(io::Error::last_os_error())
}

impl BpfDevice {
    pub fn new(name: &str) -> io::Result<Self> {
        let mut self_ = BpfDevice {
            fd: open_device()?,
            ifreq: ifreq_for(name),
        };

        self_.bind_interface()?;

        Ok(self_)
    }

    pub fn bind_interface(&mut self) -> io::Result<()> {
        try_ioctl!(self.fd, BIOCSETIF, &mut self.ifreq);

        Ok(())
    }

    /// This in fact does not return the interface's mtu,
    /// but it returns the size of the buffer that the app needs to allocate
    /// for the BPF device
    ///
    /// The `SIOGIFMTU` cant be called on a BPF descriptor. There is a workaround
    /// to get the actual interface mtu, but this should work better
    ///
    /// To get the interface MTU, you would need to create a raw socket first,
    /// and then call `SIOGIFMTU` for the same interface your BPF device is "bound" to.
    /// This MTU that you would get would not include the length of `struct bpf_hdr`
    /// which gets prepended to every packet by BPF,
    /// and your packet will be truncated if it has the length of the MTU.
    ///
    /// The buffer size for BPF is usually 4096 bytes, MTU is typically 1500 bytes.
    /// You could do something like `mtu += BPF_HDRLEN`,
    /// but you must change the buffer size the BPF device expects using `BIOCSBLEN` accordingly,
    /// and you must set it before setting the interface with the `BIOCSETIF` ioctl.
    ///
    /// The reason I said this should work better is because you might see some unexpected behavior,
    /// truncated/unaligned packets, I/O errors on read()
    /// if you change the buffer size to the actual MTU of the interface.
    pub fn interface_mtu(&mut self) -> io::Result<usize> {
        let mut bufsize: libc::c_int = 1;
        try_ioctl!(self.fd, BIOCIMMEDIATE, &mut bufsize as *mut libc::c_int);
        try_ioctl!(self.fd, BIOCGBLEN, &mut bufsize as *mut libc::c_int);

        Ok(bufsize as usize)
    }
}

impl Drop for BpfDevice {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

// SAFETY: Implementing this trait pledges that the underlying socket resource will not be dropped
// by `Read` or `Write` impls. More information can be read
// [here](https://docs.rs/async-io/latest/async_io/trait.IoSafe.html).
unsafe impl IoSafe for BpfDevice {}

impl io::Read for BpfDevice {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        unsafe {
            let len = libc::read(
                self.fd,
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len(),
            );

            if len == -1 || len < BPF_HDRLEN as isize {
                return Err(io::Error::last_os_error());
            }

            let len = len as usize;

            #[allow(trivial_casts)]
            libc::memmove(
                buffer.as_mut_ptr() as *mut libc::c_void,
                &buffer[BPF_HDRLEN] as *const u8 as *const libc::c_void,
                len - BPF_HDRLEN,
            );

            Ok(len)
        }
    }
}

impl io::Write for BpfDevice {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        unsafe {
            let len = libc::write(
                self.fd,
                buffer.as_ptr() as *const libc::c_void,
                buffer.len(),
            );

            if len == -1 {
                Err(io::Error::last_os_error()).unwrap()
            }

            Ok(len as usize)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
