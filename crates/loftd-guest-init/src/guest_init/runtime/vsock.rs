use anyhow::{Context, Result};
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};

pub(in crate::guest_init) fn connect_host(port: u32) -> Result<File> {
    let fd = unsafe { libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error())
            .context("failed to create AF_VSOCK client socket");
    }
    let stream = unsafe { File::from_raw_fd(fd) };
    let address = libc::sockaddr_vm {
        svm_family: libc::AF_VSOCK as libc::sa_family_t,
        svm_reserved1: 0,
        svm_port: port,
        svm_cid: libc::VMADDR_CID_HOST,
        svm_zero: [0; 4],
    };
    let rc = unsafe {
        libc::connect(
            stream.as_raw_fd(),
            (&address as *const libc::sockaddr_vm).cast::<libc::sockaddr>(),
            std::mem::size_of::<libc::sockaddr_vm>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .context("failed to connect AF_VSOCK client socket");
    }
    Ok(stream)
}

pub(in crate::guest_init::runtime) struct VsockListener {
    pub(in crate::guest_init::runtime) fd: RawFd,
}

impl VsockListener {
    pub(in crate::guest_init) fn bind(port: u32) -> Result<Self> {
        let fd = unsafe { libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to create AF_VSOCK socket");
        }
        let listener = Self { fd };
        let addr = libc::sockaddr_vm {
            svm_family: libc::AF_VSOCK as libc::sa_family_t,
            svm_reserved1: 0,
            svm_port: port,
            svm_cid: libc::VMADDR_CID_ANY,
            svm_zero: [0; 4],
        };
        let rc = unsafe {
            libc::bind(
                listener.fd,
                (&addr as *const libc::sockaddr_vm).cast::<libc::sockaddr>(),
                std::mem::size_of::<libc::sockaddr_vm>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to bind AF_VSOCK listener");
        }
        if unsafe { libc::listen(listener.fd, 16) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to listen on AF_VSOCK socket");
        }
        Ok(listener)
    }

    pub(in crate::guest_init) fn as_raw_fd(&self) -> RawFd {
        self.fd
    }

    pub(in crate::guest_init) fn accept(&self) -> Result<File> {
        let fd = unsafe { libc::accept(self.fd, std::ptr::null_mut(), std::ptr::null_mut()) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error()).context("failed to accept vsock client");
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

impl Drop for VsockListener {
    fn drop(&mut self) {
        let _ = unsafe { libc::close(self.fd) };
    }
}
