//! Helpers for code that runs in a `fork()`ed child.
//!
//! `fork()` copies the locks the parent's other threads held at that instant -
//! glibc's malloc arenas, the stdio locks, the Rust runtime's own - so a child
//! that allocates, prints through `std` or leaves through `std::process::exit`
//! can block forever on a lock nobody owns any more. A child that stays in the
//! forked process reaches for the helpers here instead: raw descriptors, no
//! `std` I/O, and [`exit_child`] rather than the Rust runtime's exit path.

/// Largest decimal rendering of a `u32`, used by [`report_child_errno`].
const DECIMAL_DIGITS_MAX: usize = 10;

/// Leave a `fork()`ed child.
///
/// `std::process::exit` flushes stdio and runs atexit handlers, both of which
/// take locks the parent may have held at fork time. `_exit` is one syscall.
pub(crate) fn exit_child(code: i32) -> ! {
    // SAFETY: `_exit` terminates this process without returning.
    unsafe { libc::_exit(code) }
}

/// `write(2)` until every byte of `data` reaches `fd`; false on a short write.
pub(crate) fn write_all(fd: libc::c_int, data: &[u8]) -> bool {
    let mut written = 0;
    while written < data.len() {
        // SAFETY: writing an initialized byte slice to an open descriptor.
        let n = unsafe { libc::write(fd, data[written..].as_ptr().cast(), data.len() - written) };
        if n <= 0 {
            return false;
        }
        written += n as usize;
    }
    true
}

/// Report a child failure on stderr without taking the stdio lock.
pub(crate) fn report_child_error(message: &[u8]) {
    let _ = write_all(libc::STDERR_FILENO, message);
}

/// Report `prefix` plus the current `errno` on stderr, without allocating.
///
/// The errno is rendered by hand because `format!` would allocate, and an
/// allocation is exactly what a forked child cannot rely on.
pub(crate) fn report_child_errno(prefix: &[u8]) {
    let errno = std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or_default();
    let mut digits = [0_u8; DECIMAL_DIGITS_MAX];
    let len = write_decimal(errno.unsigned_abs(), &mut digits);
    let _ = write_all(libc::STDERR_FILENO, prefix);
    if errno < 0 {
        let _ = write_all(libc::STDERR_FILENO, b"-");
    }
    let _ = write_all(libc::STDERR_FILENO, &digits[..len]);
    let _ = write_all(libc::STDERR_FILENO, b"\n");
}

/// Write `value` as decimal digits, most significant first; returns the count.
fn write_decimal(value: u32, digits: &mut [u8; DECIMAL_DIGITS_MAX]) -> usize {
    let mut len = 0;
    let mut rest = value;
    loop {
        digits[len] = b'0' + (rest % 10) as u8;
        rest /= 10;
        len += 1;
        if rest == 0 {
            break;
        }
    }
    digits[..len].reverse();
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_decimal_renders_every_digit_most_significant_first() {
        let mut digits = [0_u8; DECIMAL_DIGITS_MAX];

        assert_eq!(write_decimal(0, &mut digits), 1);
        assert_eq!(&digits[..1], b"0");

        assert_eq!(write_decimal(95, &mut digits), 2);
        assert_eq!(&digits[..2], b"95");

        assert_eq!(write_decimal(4096, &mut digits), 4);
        assert_eq!(&digits[..4], b"4096");

        assert_eq!(write_decimal(u32::MAX, &mut digits), DECIMAL_DIGITS_MAX);
        assert_eq!(&digits, b"4294967295");
    }

    #[test]
    fn write_all_reaches_a_pipe_reader_with_every_byte() {
        let mut fds = [0; 2];
        // SAFETY: pipe writes two valid descriptors into `fds` on success.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let message = b"cang child failure";

        assert!(write_all(fds[1], message));

        let mut read_back = [0_u8; 18];
        // SAFETY: reading into the buffer just sized for the message.
        let read = unsafe { libc::read(fds[0], read_back.as_mut_ptr().cast(), read_back.len()) };
        assert_eq!(read, 18);
        assert_eq!(&read_back, message);
        // SAFETY: the test owns both descriptors.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
    }
}
