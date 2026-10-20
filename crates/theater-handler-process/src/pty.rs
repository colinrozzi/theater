//! Pseudo-terminal plumbing `tokio::process` does not expose: opening a PTY
//! pair, making the slave the child's controlling terminal, async I/O on the
//! master, and window-size changes.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use tokio::io::unix::AsyncFd;

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

fn cvt(r: libc::c_int) -> io::Result<libc::c_int> {
    if r == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

fn set_cloexec(fd: RawFd) -> io::Result<()> {
    let flags = cvt(unsafe { libc::fcntl(fd, libc::F_GETFD) })?;
    cvt(unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) })?;
    Ok(())
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    let flags = cvt(unsafe { libc::fcntl(fd, libc::F_GETFL) })?;
    cvt(unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) })?;
    Ok(())
}

/// Open a PTY pair sized `cols` x `rows`. Returns `(master, slave)`, both
/// CLOEXEC (the slave reaches the child only via the stdio dups); the master
/// is non-blocking for [`PtyMaster`].
pub(crate) fn open_pty(cols: u16, rows: u16) -> io::Result<(OwnedFd, OwnedFd)> {
    let mut master: libc::c_int = -1;
    let mut slave: libc::c_int = -1;
    let ws = winsize(cols, rows);
    cvt(unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            // termp/winp are `*const` on Linux, `*mut` on macOS; `as _` fits both.
            std::ptr::null::<libc::termios>() as _,
            &ws as *const libc::winsize as _,
        )
    })?;
    // SAFETY: openpty succeeded, so both are fresh fds we now own.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    set_cloexec(master.as_raw_fd())?;
    set_cloexec(slave.as_raw_fd())?;
    set_nonblocking(master.as_raw_fd())?;
    Ok((master, slave))
}

/// `pre_exec` hook for a PTY child: start a new session (so the child leads
/// its own process group) and make the PTY slave — already dup'd onto fd 0 —
/// its controlling terminal, so job control and ^C work. Runs between fork
/// and exec: async-signal-safe calls only.
pub(crate) fn make_controlling_tty() -> io::Result<()> {
    cvt(unsafe { libc::setsid() } as libc::c_int)?;
    cvt(unsafe { libc::ioctl(0, libc::TIOCSCTTY as _, 0) })?;
    Ok(())
}

/// The parent's side of a PTY.
pub(crate) struct PtyMaster {
    fd: AsyncFd<OwnedFd>,
}

impl PtyMaster {
    pub(crate) fn new(fd: OwnedFd) -> io::Result<Self> {
        Ok(Self {
            fd: AsyncFd::new(fd)?,
        })
    }

    /// Read available output. `Ok(0)` is EOF: Linux reports EIO on the master
    /// once every slave fd is closed (the child and its descendants are gone).
    /// Cancel-safe: nothing is consumed until the read completes.
    pub(crate) async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut guard = self.fd.readable().await?;
            match guard.try_io(|fd| {
                let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }) {
                Ok(Ok(n)) => return Ok(n),
                Ok(Err(e)) if e.raw_os_error() == Some(libc::EIO) => return Ok(0),
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
    }

    pub(crate) async fn write_all(&self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            let mut guard = self.fd.writable().await?;
            match guard.try_io(|fd| {
                let n = unsafe { libc::write(fd.as_raw_fd(), data.as_ptr().cast(), data.len()) };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }) {
                Ok(Ok(n)) => data = &data[n..],
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
        Ok(())
    }

    /// Set the window size; the kernel delivers SIGWINCH to the foreground
    /// process group.
    pub(crate) fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let ws = winsize(cols, rows);
        cvt(unsafe { libc::ioctl(self.fd.as_raw_fd(), libc::TIOCSWINSZ as _, &ws) })?;
        Ok(())
    }
}
