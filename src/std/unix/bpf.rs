//! macOS and OpenBSD raw socket using BPF.
//!
//! Copied from SmolTCP's `BpfDevice` with a few extra trait implementations and additions for
//! handling multiple frames. Thank you, SmolTCP maintainers!

use crate::{
    ethernet::{EthernetAddress, ETHERNET_HEADER_LEN},
    fmt,
    std::unix::{ifreq, ifreq_for},
};
use async_io::IoSafe;
use std::{
    io, mem,
    os::unix::io::{AsFd, AsRawFd, BorrowedFd, RawFd},
};

/// set interface
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "freebsd"
))]
const BIOCSETIF: libc::c_ulong = 0x8020426c;
/// get buffer length
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "freebsd"
))]
const BIOCGBLEN: libc::c_ulong = 0x40044266;
/// set immediate/nonblocking read
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "freebsd"
))]
const BIOCIMMEDIATE: libc::c_ulong = 0x80044270;
/// set bpf_hdr struct size
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "netbsd"))]
const SIZEOF_BPF_HDR: usize = 18;
/// set bpf_hdr struct size
#[cfg(any(target_os = "openbsd", target_os = "freebsd"))]
const SIZEOF_BPF_HDR: usize = 24;
/// The actual header length may be larger than the bpf_hdr struct due to aligning
/// see https://github.com/openbsd/src/blob/37ecb4d066e5566411cc16b362d3960c93b1d0be/sys/net/bpf.c#L1649
/// and https://github.com/apple/darwin-xnu/blob/8f02f2a044b9bb1ad951987ef5bab20ec9486310/bsd/net/bpf.c#L3580
/// and https://github.com/NetBSD/src/blob/13d937d9ba3db87c9a898a40a8ed9d2aab2b1b95/sys/net/bpf.c#L1988
/// and https://github.com/freebsd/freebsd-src/blob/b8afdda360e5915be3c2cf0d1438f511779b03db/sys/net/bpf.c#L133-L134
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "freebsd"
))]
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
    /// Interface file handle.
    fd: libc::c_int,
    /// Interface... stuff.
    ifreq: ifreq,
    /// Interface name like `en11`.
    name: String,
    /// Holds additional frame data if more than one frame was returned in the `read` call.
    buf: Vec<u8>,
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
            name: name.to_string(),
            buf: Vec::with_capacity(4096),
        };

        self_.bind_interface()?;

        Ok(self_)
    }

    #[allow(trivial_casts)]
    pub fn bind_interface(&mut self) -> io::Result<()> {
        let mut bufsize: libc::c_int = 1;

        try_ioctl!(self.fd, BIOCIMMEDIATE, &mut bufsize as *mut libc::c_int);
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
    #[allow(trivial_casts)]
    pub fn interface_mtu(&mut self) -> io::Result<usize> {
        let mut bufsize: libc::c_int = 1;
        try_ioctl!(self.fd, BIOCIMMEDIATE, &mut bufsize as *mut libc::c_int);
        try_ioctl!(self.fd, BIOCGBLEN, &mut bufsize as *mut libc::c_int);

        Ok(bufsize as usize)
    }

    pub fn mac(&self) -> io::Result<Option<EthernetAddress>> {
        Ok(nix::ifaddrs::getifaddrs()?
            .find(|iface| iface.interface_name == self.name)
            .and_then(|iface| iface.address)
            .and_then(|addr| addr.as_link_addr()?.addr())
            .map(EthernetAddress))
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
        // If more than one packet was returned in the previous call to `read`, the second and
        // further packets will be present in our buffer. We'll read the rest of the buffer out for
        // processing instead of reading the network interface.
        let len = if !self.buf.is_empty() {
            let len = self.buf.len().min(buffer.len());

            debug_assert!(
                len >= BPF_HDRLEN,
                "not enough previous buffer {} B to hold BPF header {} B",
                len,
                BPF_HDRLEN
            );

            fmt::trace!("{} bytes left from previous read", len);

            let (cached_chunk, rest) = self.buf.split_at(len);

            buffer[0..len].copy_from_slice(cached_chunk);

            self.buf = rest.to_vec();

            len
        } else {
            let len = unsafe {
                libc::read(
                    self.fd,
                    buffer.as_mut_ptr() as *mut libc::c_void,
                    buffer.len(),
                )
            };

            if len == -1 || len < BPF_HDRLEN as isize {
                return Err(io::Error::last_os_error());
            }

            len as usize
        };

        // Get the Ethernet FRAME length (header, ethertype, payload) from the BPF header
        let frame_len = {
            let bpf_header = &buffer[0..BPF_HDRLEN];

            let bpf_header = unsafe {
                core::ptr::NonNull::new(bpf_header.as_ptr() as *mut libc::bpf_hdr)
                    .ok_or(io::Error::other("no BPF header"))?
                    .as_ref()
            };

            debug_assert_eq!(
                bpf_header.bh_caplen, bpf_header.bh_datalen,
                "not all frame data was read"
            );

            bpf_header.bh_datalen as usize
        };

        // Number of remaining bytes in the read after the first packet
        let remaining = len as u32 - BPF_HDRLEN as u32 - frame_len as u32;

        // Returned data is aligned to bpf_hdr boundaries
        let remaining = remaining.next_multiple_of(core::mem::align_of::<libc::bpf_hdr>() as u32);

        // There is at least one more frame in the returned data. Store this in our buffer to return
        // next time `read` is called.
        if remaining > 0 {
            let start =
                (BPF_HDRLEN + frame_len).next_multiple_of(core::mem::align_of::<libc::bpf_hdr>());

            // Store next chunk(s - there could be more than one packet waiting) of [BPF header,
            // Ethernet II frame] in cache for next time round.
            self.buf = buffer[start..len].to_vec();
        }

        // Strip BPF header from beginning of buffer
        #[allow(trivial_casts)]
        unsafe {
            libc::memmove(
                buffer.as_mut_ptr() as *mut libc::c_void,
                &buffer[BPF_HDRLEN] as *const u8 as *const libc::c_void,
                frame_len,
            )
        };

        Ok(frame_len)
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
                return Err(io::Error::last_os_error());
            }

            Ok(len as usize)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
