use anyhow::{Context, Result, bail};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

const LOOPBACK_INTERFACE: &str = "lo";
const LOOPBACK_IPV4: &str = "127.0.0.1";
const LOOPBACK_PREFIX_LEN: u8 = 8;
const IFNAMSIZ: usize = 16;
const IFF_UP: libc::c_short = 0x1;

pub(in crate::guest_init) fn ensure_loopback_ipv4() -> Result<()> {
    ensure_loopback_ipv4_with(&mut IoctlLoopbackConfigurator::new()?)
}

fn ensure_loopback_ipv4_with(configurator: &mut impl LoopbackConfigurator) -> Result<()> {
    configurator.add_ipv4_address(LOOPBACK_INTERFACE, LOOPBACK_IPV4, LOOPBACK_PREFIX_LEN)?;
    configurator.set_link_up(LOOPBACK_INTERFACE)
}

trait LoopbackConfigurator {
    fn add_ipv4_address(&mut self, interface: &str, address: &str, prefix_len: u8) -> Result<()>;
    fn set_link_up(&mut self, interface: &str) -> Result<()>;
}

struct IoctlLoopbackConfigurator {
    socket: OwnedFd,
}

impl IoctlLoopbackConfigurator {
    fn new() -> Result<Self> {
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
        if fd < 0 {
            bail!(
                "failed to open loopback configuration socket: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(Self {
            socket: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }
}

impl LoopbackConfigurator for IoctlLoopbackConfigurator {
    fn add_ipv4_address(&mut self, interface: &str, address: &str, prefix_len: u8) -> Result<()> {
        let address_text = address.to_owned();
        let address = parse_ipv4(address)?;
        let netmask = prefix_len_to_netmask(prefix_len)?;
        set_interface_address(self.socket.as_raw_fd(), interface, address).with_context(|| {
            format!("failed to assign {address_text}/{prefix_len} to {interface}")
        })?;
        set_interface_netmask(self.socket.as_raw_fd(), interface, netmask)
            .with_context(|| format!("failed to set {interface} netmask"))
    }

    fn set_link_up(&mut self, interface: &str) -> Result<()> {
        let mut ifr = Ifreq::named(interface);
        ioctl_readwrite(self.socket.as_raw_fd(), libc::SIOCGIFFLAGS, &mut ifr)
            .with_context(|| format!("failed to read {interface} interface flags"))?;
        unsafe {
            ifr.ifr_ifru.ifru_flags |= IFF_UP;
        }
        ioctl_readwrite(self.socket.as_raw_fd(), libc::SIOCSIFFLAGS, &mut ifr)
            .with_context(|| format!("failed to bring {interface} interface up"))
    }
}

fn set_interface_address(fd: i32, interface: &str, address: [u8; 4]) -> Result<()> {
    let mut ifr = Ifreq::named(interface);
    ifr.ifr_ifru.ifru_addr = sockaddr_in(address);
    ioctl_readwrite(fd, libc::SIOCSIFADDR, &mut ifr)
}

fn set_interface_netmask(fd: i32, interface: &str, netmask: [u8; 4]) -> Result<()> {
    let mut ifr = Ifreq::named(interface);
    ifr.ifr_ifru.ifru_addr = sockaddr_in(netmask);
    ioctl_readwrite(fd, libc::SIOCSIFNETMASK, &mut ifr)
}

fn ioctl_readwrite(fd: i32, request: libc::c_ulong, ifr: &mut Ifreq) -> Result<()> {
    let rc = unsafe { libc::ioctl(fd, request, ifr as *mut Ifreq) };
    if rc < 0 {
        bail!(std::io::Error::last_os_error());
    }
    Ok(())
}

fn parse_ipv4(address: &str) -> Result<[u8; 4]> {
    address
        .parse::<std::net::Ipv4Addr>()
        .map(|address| address.octets())
        .with_context(|| format!("invalid IPv4 address {address}"))
}

fn prefix_len_to_netmask(prefix_len: u8) -> Result<[u8; 4]> {
    if prefix_len > 32 {
        bail!("invalid IPv4 prefix length {prefix_len}");
    }
    if prefix_len == 0 {
        return Ok([0; 4]);
    }
    Ok((u32::MAX << (32 - prefix_len)).to_be_bytes())
}

fn sockaddr_in(ip: [u8; 4]) -> libc::sockaddr {
    let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    addr.sin_family = libc::AF_INET as libc::sa_family_t;
    addr.sin_addr.s_addr = u32::from_ne_bytes(ip);
    unsafe { std::mem::transmute(addr) }
}

#[repr(C)]
struct Ifreq {
    ifr_name: [u8; IFNAMSIZ],
    ifr_ifru: IfreqIfru,
}

impl Ifreq {
    fn named(name: &str) -> Self {
        let mut ifr: Self = unsafe { std::mem::zeroed() };
        let bytes = name.as_bytes();
        let len = bytes.len().min(IFNAMSIZ - 1);
        ifr.ifr_name[..len].copy_from_slice(&bytes[..len]);
        ifr
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
union IfreqIfru {
    ifru_flags: libc::c_short,
    ifru_addr: libc::sockaddr,
    _pad: [u8; 24],
}

#[cfg(test)]
#[path = "loopback_tests.rs"]
mod tests;
