//! Block Client for direct binary block download.
//!
//! Connects directly to remote nodes' Block Server to download blocks,
//! bypassing the local daemon IPC overhead.

use std::collections::HashMap;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::daemon::block_server::{BLOCK_SERVER_PORT, STATUS_OK};
use crate::error::{AppError, Result};
use crate::p2p::messages::RsBlock;

/// Block client for direct async connections to remote block servers.
pub struct BlockClient {
    /// Cache of connections by IP address
    connections: HashMap<String, TcpStream>,
    /// Connection timeout
    timeout: Duration,
}

impl BlockClient {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            timeout: Duration::from_secs(30),
        }
    }

    /// Get a block directly from a remote address.
    /// 
    /// `ip` should be the IP address of the remote node (e.g., "172.18.36.82")
    pub async fn get_block(&mut self, ip: &str, hash: &str) -> Result<RsBlock> {
        let addr = format!("{}:{}", ip, BLOCK_SERVER_PORT);

        // Get or create connection
        let stream = if let Some(stream) = self.connections.get_mut(ip) {
            stream
        } else {
            let stream = tokio::time::timeout(
                self.timeout,
                TcpStream::connect(&addr)
            ).await
                .map_err(|_| AppError::Io(std::io::Error::other("Connection timeout")))?
                .map_err(|e| AppError::Io(e))?;
            
            self.connections.insert(ip.to_string(), stream);
            self.connections.get_mut(ip).unwrap()
        };

        // Send request: [4 bytes: hash length] + [hash bytes]
        let hash_bytes = hash.as_bytes();
        let hash_len = (hash_bytes.len() as u32).to_be_bytes();
        
        if let Err(e) = stream.write_all(&hash_len).await {
            // Connection failed, remove and return error
            self.connections.remove(ip);
            return Err(AppError::Io(e));
        }
        if let Err(e) = stream.write_all(hash_bytes).await {
            self.connections.remove(ip);
            return Err(AppError::Io(e));
        }

        // Read response: [4 bytes: status] + [4 bytes: data length] + [data]
        let mut status_buf = [0u8; 4];
        if let Err(e) = stream.read_exact(&mut status_buf).await {
            self.connections.remove(ip);
            return Err(AppError::Io(e));
        }
        let status = u32::from_be_bytes(status_buf);

        let mut len_buf = [0u8; 4];
        if let Err(e) = stream.read_exact(&mut len_buf).await {
            self.connections.remove(ip);
            return Err(AppError::Io(e));
        }
        let data_len = u32::from_be_bytes(len_buf) as usize;

        if status != STATUS_OK {
            self.connections.remove(ip);
            return Err(AppError::Io(std::io::Error::other(
                format!("Block server error: status={}", status)
            )));
        }

        // Read data
        let mut data = vec![0u8; data_len];
        if let Err(e) = stream.read_exact(&mut data).await {
            self.connections.remove(ip);
            return Err(AppError::Io(e));
        }

        Ok(RsBlock {
            hash: hash.to_string(),
            data,
        })
    }

    /// Extract IP address from a multiaddr string.
    /// 
    /// Example: "/ip4/172.18.36.82/tcp/10826/p2p/12D3..." -> "172.18.36.82"
    pub fn extract_ip(multiaddr: &str) -> Option<String> {
        let parts: Vec<&str> = multiaddr.split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            if *part == "ip4" && i + 1 < parts.len() {
                // Skip local addresses
                let ip = parts[i + 1];
                if ip != "127.0.0.1" && !ip.starts_with("198.18.") {
                    return Some(ip.to_string());
                }
            }
        }
        None
    }

    /// Close all connections.
    pub fn close_all(&mut self) {
        self.connections.clear();
    }
}

impl Default for BlockClient {
    fn default() -> Self {
        Self::new()
    }
}
