use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::task;

use anyhow::{Error, Result};

pub struct SerialPort {
    file: File,
}

impl SerialPort {
    pub async fn open(path: &str, baud_rate: u32) -> Result<Self, Error> {
        let path = Path::new(path);

        // Open serial port with O_NOCTTY to prevent it from becoming controlling terminal
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| Error::msg(format!("Failed to open {}: {}", path.display(), e)))?;

        let fd = file.as_raw_fd();

        // Configure serial port
        task::spawn_blocking(move || configure_serial_port(fd, baud_rate))
            .await
            .map_err(|e| Error::msg(format!("Failed to configure serial port: {}", e)))??;

        Ok(SerialPort { file })
    }
}

fn configure_serial_port(fd: RawFd, baud_rate: u32) -> Result<()> {
    // Get current termios settings
    let mut termios = unsafe {
        let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
        if libc::tcgetattr(fd, termios.as_mut_ptr()) != 0 {
            return Err(Error::msg(format!(
                "Failed to get termios: {}",
                std::io::Error::last_os_error()
            )));
        }
        termios.assume_init()
    };

    // Convert baud rate
    let baud = match baud_rate {
        50 => libc::B50,
        75 => libc::B75,
        110 => libc::B110,
        134 => libc::B134,
        150 => libc::B150,
        200 => libc::B200,
        300 => libc::B300,
        600 => libc::B600,
        1200 => libc::B1200,
        1800 => libc::B1800,
        2400 => libc::B2400,
        4800 => libc::B4800,
        9600 => libc::B9600,
        19200 => libc::B19200,
        38400 => libc::B38400,
        57600 => libc::B57600,
        115200 => libc::B115200,
        230400 => libc::B230400,
        _ => return Err(Error::msg(format!("Unsupported baud rate: {}", baud_rate))),
    };

    // Set baud rate
    let baud_speed = baud;
    unsafe {
        if libc::cfsetispeed(&mut termios, baud_speed) != 0 {
            return Err(Error::msg(format!(
                "Failed to set input speed: {}",
                std::io::Error::last_os_error()
            )));
        }
        if libc::cfsetospeed(&mut termios, baud_speed) != 0 {
            return Err(Error::msg(format!(
                "Failed to set output speed: {}",
                std::io::Error::last_os_error()
            )));
        }
    }

    // Configure for raw mode (8N1)
    termios.c_iflag = 0;
    termios.c_oflag = 0;
    termios.c_cflag = libc::CS8 | libc::CREAD | libc::CLOCAL;
    termios.c_lflag = 0;

    // Set minimum characters and timeout
    termios.c_cc[libc::VMIN] = 0;
    termios.c_cc[libc::VTIME] = 0;

    // Apply settings
    unsafe {
        if libc::tcsetattr(fd, libc::TCSANOW, &termios) != 0 {
            return Err(Error::msg(format!(
                "Failed to set termios: {}",
                std::io::Error::last_os_error()
            )));
        }

        // Flush input/output buffers
        if libc::tcflush(fd, libc::TCIOFLUSH) != 0 {
            return Err(Error::msg(format!(
                "Failed to flush buffers: {}",
                std::io::Error::last_os_error()
            )));
        }
    }

    Ok(())
}

impl Read for SerialPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl Write for SerialPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl AsyncRead for SerialPort {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let mut temp_buf = vec![0u8; buf.remaining()];
        match self.file.read(&mut temp_buf) {
            Ok(n) => {
                buf.put_slice(&temp_buf[..n]);
                std::task::Poll::Ready(Ok(()))
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
            Err(e) => std::task::Poll::Ready(Err(e)),
        }
    }
}

impl AsyncWrite for SerialPort {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        match self.file.write(buf) {
            Ok(n) => std::task::Poll::Ready(Ok(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
            Err(e) => std::task::Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        match self.file.flush() {
            Ok(()) => std::task::Poll::Ready(Ok(())),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
            Err(e) => std::task::Poll::Ready(Err(e)),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}
