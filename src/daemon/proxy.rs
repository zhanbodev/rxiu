//! P2P proxy that communicates with daemon via IPC.
//!
//! This provides the same interface as P2PService but routes
//! all calls through the daemon.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use libp2p::PeerId;

use crate::error::{AppError, Result};
use crate::p2p::node::PeerInfo;
use crate::p2p::messages::{FileChunk, FileMeta, RsBlock, RsHave};
use crate::daemon::protocol::RsSyncStatus;
use crate::rs::RsFileEntry;
use crate::storage::FileMetadata;

use super::protocol::{encode_message, parse_length, DaemonRequest, DaemonResponse, DAEMON_PORT};

/// P2P proxy that routes calls through daemon IPC.
#[derive(Clone)]
pub struct P2PProxy {
    local_peer_id: PeerId,
}

impl P2PProxy {
    /// Connect to daemon and create proxy.
    pub fn connect() -> Result<Self> {
        // Get local peer ID from daemon
        let mut stream = Self::new_connection()?;
        let req = DaemonRequest::GetLocalPeerId;
        Self::send_request(&mut stream, &req)?;
        let resp = Self::recv_response(&mut stream)?;
        
        let local_peer_id = match resp {
            DaemonResponse::LocalPeerId(id) => {
                id.parse().map_err(|_| AppError::Io(std::io::Error::other("Invalid peer ID")))?
            }
            DaemonResponse::Error(e) => return Err(AppError::Io(std::io::Error::other(e))),
            _ => return Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        };
        
        Ok(Self { local_peer_id })
    }
    
    fn new_connection() -> Result<TcpStream> {
        let addr = format!("127.0.0.1:{}", DAEMON_PORT);
        let stream = TcpStream::connect_timeout(
            &addr.parse().unwrap(),
            Duration::from_secs(2),
        ).map_err(AppError::Io)?;
        
        stream.set_read_timeout(Some(Duration::from_secs(120)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        
        Ok(stream)
    }
    
    fn send_request(stream: &mut TcpStream, req: &DaemonRequest) -> Result<()> {
        let data = encode_message(req);
        stream.write_all(&data)?;
        stream.flush()?;
        Ok(())
    }
    
    fn recv_response(stream: &mut TcpStream) -> Result<DaemonResponse> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf)?;
        let len = parse_length(&len_buf).ok_or_else(|| {
            AppError::Io(std::io::Error::other("Invalid response length"))
        })?;
        
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload)?;
        
        cbor4ii::serde::from_slice(&payload)
            .map_err(|e| AppError::Io(std::io::Error::other(format!("CBOR parse error: {}", e))))
    }
    
    fn request(&self, req: DaemonRequest) -> Result<DaemonResponse> {
        let mut stream = Self::new_connection()?;
        Self::send_request(&mut stream, &req)?;
        Self::recv_response(&mut stream)
    }
    
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }
    
    pub async fn get_peers(&self) -> Vec<PeerInfo> {
        match self.request(DaemonRequest::GetPeers) {
            Ok(DaemonResponse::Peers(peers)) => peers,
            _ => Vec::new(),
        }
    }
    
    pub async fn peer_count(&self) -> usize {
        match self.request(DaemonRequest::GetPeerCount) {
            Ok(DaemonResponse::PeerCount(count)) => count,
            _ => 0,
        }
    }
    
    pub async fn list_remote_zones(&self, peer_id: PeerId) -> Result<Vec<String>> {
        match self.request(DaemonRequest::ListRemoteZones { peer_id: peer_id.to_string() })? {
            DaemonResponse::Zones(zones) => Ok(zones),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }
    
    pub async fn list_remote_files(&self, peer_id: PeerId, zone: &str) -> Result<Vec<FileMetadata>> {
        match self.request(DaemonRequest::ListRemoteFiles { 
            peer_id: peer_id.to_string(), 
            zone: zone.to_string() 
        })? {
            DaemonResponse::Files(files) => Ok(files),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }
    
    pub async fn fetch_file(&self, peer_id: PeerId, zone: &str, name: &str) -> Result<(Vec<u8>, String)> {
        match self.request(DaemonRequest::FetchFile { 
            peer_id: peer_id.to_string(), 
            zone: zone.to_string(),
            name: name.to_string(),
        })? {
            DaemonResponse::FileData { content, hash } => Ok((content, hash)),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }
    
    pub async fn get_file_meta(&self, peer_id: PeerId, zone: &str, name: &str) -> Result<FileMeta> {
        match self.request(DaemonRequest::GetFileMeta { 
            peer_id: peer_id.to_string(), 
            zone: zone.to_string(),
            name: name.to_string(),
        })? {
            DaemonResponse::FileMeta(meta) => Ok(meta),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }
    
    pub async fn get_file_chunk(&self, peer_id: PeerId, zone: &str, name: &str, offset: u64, size: u64) -> Result<FileChunk> {
        match self.request(DaemonRequest::GetFileChunk { 
            peer_id: peer_id.to_string(), 
            zone: zone.to_string(),
            name: name.to_string(),
            offset,
            size,
        })? {
            DaemonResponse::FileChunk(chunk) => Ok(chunk),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }
    
    pub async fn rs_list(&self, peer_id: PeerId) -> Result<Vec<RsFileEntry>> {
        match self.request(DaemonRequest::RsList { peer_id: peer_id.to_string() })? {
            DaemonResponse::RsFiles(files) => Ok(files),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }
    
    pub async fn rs_announce(&self, peer_id: PeerId, file: RsFileEntry) -> Result<()> {
        match self.request(DaemonRequest::RsAnnounce { 
            peer_id: peer_id.to_string(), 
            file 
        })? {
            DaemonResponse::Ok => Ok(()),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }
    
    pub async fn rs_get_meta(&self, peer_id: PeerId, name: &str) -> Result<RsFileEntry> {
        match self.request(DaemonRequest::RsGetMeta { 
            peer_id: peer_id.to_string(), 
            name: name.to_string() 
        })? {
            DaemonResponse::RsMeta(meta) => Ok(meta),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }
    
    pub async fn rs_get_block(&self, peer_id: PeerId, hash: &str) -> Result<RsBlock> {
        match self.request(DaemonRequest::RsGetBlock { 
            peer_id: peer_id.to_string(), 
            hash: hash.to_string() 
        })? {
            DaemonResponse::RsBlock(block) => Ok(block),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    pub async fn rs_get_blocks(&self, peer_id: PeerId, hashes: Vec<String>) -> Result<Vec<RsBlock>> {
        match self.request(DaemonRequest::RsGetBlocks {
            peer_id: peer_id.to_string(),
            hashes,
        })? {
            DaemonResponse::RsBlocks(blocks) => Ok(blocks),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    pub async fn rs_have(&self, peer_id: PeerId, name: &str) -> Result<RsHave> {
        match self.request(DaemonRequest::RsHave {
            peer_id: peer_id.to_string(),
            name: name.to_string(),
        })? {
            DaemonResponse::RsHave(have) => Ok(have),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }
    
    pub async fn rs_delete(&self, peer_id: PeerId, name: &str) -> Result<()> {
        match self.request(DaemonRequest::RsDelete { 
            peer_id: peer_id.to_string(), 
            name: name.to_string() 
        })? {
            DaemonResponse::Ok => Ok(()),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    pub async fn rs_sync(&self) -> Result<()> {
        match self.request(DaemonRequest::RsSync)? {
            DaemonResponse::Ok => Ok(()),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    pub async fn rs_sync_status(&self) -> Result<RsSyncStatus> {
        match self.request(DaemonRequest::RsSyncStatus)? {
            DaemonResponse::RsSyncStatus(status) => Ok(status),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    /// Refresh LAN peers by triggering network recovery.
    pub async fn refresh_peers(&self) -> Result<usize> {
        match self.request(DaemonRequest::RefreshPeers)? {
            DaemonResponse::PeerCount(count) => Ok(count),
            DaemonResponse::Error(e) => Err(AppError::Io(std::io::Error::other(e))),
            _ => Err(AppError::Io(std::io::Error::other("Unexpected response"))),
        }
    }
}
