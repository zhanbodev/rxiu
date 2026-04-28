//! Daemon client for TUI to communicate with background daemon.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::error::{AppError, Result};

use super::protocol::{DAEMON_PORT, DaemonRequest, DaemonResponse, encode_message, parse_length};

/// Client to communicate with the daemon.
pub struct DaemonClient {
    stream: TcpStream,
}

impl DaemonClient {
    /// Connect to the daemon.
    pub fn connect() -> Result<Self> {
        let addr = format!("127.0.0.1:{}", DAEMON_PORT);
        let stream = TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(2))
            .map_err(|e| AppError::Io(e))?;

        stream.set_read_timeout(Some(Duration::from_secs(60)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;

        Ok(Self { stream })
    }

    /// Check if daemon is running.
    pub fn is_daemon_running() -> bool {
        Self::connect().is_ok()
    }

    /// Send a request and receive response.
    pub fn request(&mut self, req: DaemonRequest) -> Result<DaemonResponse> {
        // Send request
        let data = encode_message(&req);
        self.stream.write_all(&data)?;
        self.stream.flush()?;

        // Read length prefix
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf)?;
        let len = parse_length(&len_buf)
            .ok_or_else(|| AppError::Io(std::io::Error::other("Invalid response length")))?;

        // Read payload
        let mut payload = vec![0u8; len];
        self.stream.read_exact(&mut payload)?;

        // Parse response (CBOR)
        let resp: DaemonResponse = cbor4ii::serde::from_slice(&payload)
            .map_err(|e| AppError::Io(std::io::Error::other(format!("CBOR parse error: {}", e))))?;

        Ok(resp)
    }

    /// Ping the daemon.
    pub fn ping(&mut self) -> Result<()> {
        match self.request(DaemonRequest::Ping)? {
            DaemonResponse::Pong => Ok(()),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    /// Shutdown the daemon.
    pub fn shutdown(&mut self) -> Result<()> {
        let _ = self.request(DaemonRequest::Shutdown);
        Ok(())
    }
}
