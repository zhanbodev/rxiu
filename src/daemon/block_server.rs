//! Block Server for direct binary block transfer.
//!
//! Provides fast binary streaming of RS blocks without JSON/CBOR overhead.
//! Listens on port BLOCK_SERVER_PORT and serves block data directly.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

use crate::rs::RsStore;

/// Block server port (separate from daemon IPC port)
pub const BLOCK_SERVER_PORT: u16 = 19821;

/// Status codes for block responses
pub const STATUS_OK: u32 = 0;
pub const STATUS_NOT_FOUND: u32 = 1;
pub const STATUS_ERROR: u32 = 2;

/// Run the block server.
pub async fn run_block_server(
    rs_store: Arc<RwLock<RsStore>>,
    shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) -> crate::Result<()> {
    let addr = format!("0.0.0.0:{}", BLOCK_SERVER_PORT);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("[BlockServer] Listening on {}", addr);

    let mut shutdown_rx = shutdown_rx;

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        tracing::debug!("[BlockServer] Client connected from {}", addr);
                        let rs_store = rs_store.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_block_client(stream, rs_store).await {
                                tracing::debug!("[BlockServer] Client error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("[BlockServer] Accept error: {}", e);
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                tracing::info!("[BlockServer] Shutdown signal received");
                break;
            }
        }
    }

    Ok(())
}

async fn handle_block_client(
    mut stream: TcpStream,
    rs_store: Arc<RwLock<RsStore>>,
) -> crate::Result<()> {
    loop {
        // Read hash length (4 bytes)
        let mut len_buf = [0u8; 4];
        if stream.read_exact(&mut len_buf).await.is_err() {
            break; // Client disconnected
        }
        let hash_len = u32::from_be_bytes(len_buf) as usize;

        // Sanity check
        if hash_len > 256 {
            send_error(&mut stream, STATUS_ERROR).await?;
            continue;
        }

        // Read hash
        let mut hash_buf = vec![0u8; hash_len];
        stream.read_exact(&mut hash_buf).await?;
        let hash = String::from_utf8_lossy(&hash_buf).to_string();

        // Read block from store
        let result = {
            let store = rs_store.read().await;
            store.read_block(&hash)
        };

        match result {
            Ok(data) => {
                // Send success response
                // [4 bytes: status] + [4 bytes: data length] + [data]
                let status = STATUS_OK.to_be_bytes();
                let data_len = (data.len() as u32).to_be_bytes();
                
                stream.write_all(&status).await?;
                stream.write_all(&data_len).await?;
                stream.write_all(&data).await?;
            }
            Err(_) => {
                send_error(&mut stream, STATUS_NOT_FOUND).await?;
            }
        }
    }

    Ok(())
}

async fn send_error(stream: &mut TcpStream, status: u32) -> crate::Result<()> {
    let status_bytes = status.to_be_bytes();
    let zero_len = 0u32.to_be_bytes();
    stream.write_all(&status_bytes).await?;
    stream.write_all(&zero_len).await?;
    Ok(())
}
