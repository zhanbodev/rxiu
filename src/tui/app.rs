//! TUI Application state and main loop.
use super::block_client::BlockClient;
use super::input::handle_input;
use super::render::render;
use crate::config::AppConfig;
use crate::daemon::P2PProxy;
use crate::error::{AppError, Result};
use crate::p2p::node::PeerInfo;
use crate::renew::version::BUILD_VERSION;
use crate::rs::sync::prune_unowned_blocks;
use crate::rs::{RsFileEntry, RsStore};
use crate::storage::ZoneManager;
use crate::ui::{BrowserMode, FileBrowser};
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p::PeerId;
use ratatui::Terminal;
use ratatui::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Stdout, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
/// Output line with optional styling.
#[derive(Debug, Clone)]
pub struct OutputLine {
    pub content: String,
    pub style: LineStyle,
}
#[derive(Debug, Clone, Copy, Default)]
pub enum LineStyle {
    #[default]
    Normal,
    Success,
    Error,
    Info,
    Header,
}
impl OutputLine {
    pub fn normal(s: impl Into<String>) -> Self {
        Self {
            content: s.into(),
            style: LineStyle::Normal,
        }
    }
    pub fn success(s: impl Into<String>) -> Self {
        Self {
            content: s.into(),
            style: LineStyle::Success,
        }
    }
    pub fn error(s: impl Into<String>) -> Self {
        Self {
            content: s.into(),
            style: LineStyle::Error,
        }
    }
    pub fn info(s: impl Into<String>) -> Self {
        Self {
            content: s.into(),
            style: LineStyle::Info,
        }
    }
    pub fn header(s: impl Into<String>) -> Self {
        Self {
            content: s.into(),
            style: LineStyle::Header,
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
struct ResumeMeta {
    zone: String,
    name: String,
    size: u64,
    hash: String,
    chunk_size: u64,
}
async fn download_file_chunked(
    p2p: &P2PProxy,
    peer_id: PeerId,
    zone: &str,
    name: &str,
    save_path: &PathBuf,
    progress_tx: &tokio::sync::mpsc::Sender<TransferResult>,
) -> Result<(PathBuf, u64, String)> {
    fs::create_dir_all(save_path)?;
    let meta = p2p.get_file_meta(peer_id, zone, name).await?;
    let full_path = save_path.join(name);
    let part_path = save_path.join(format!("{}.part", name));
    let meta_path = save_path.join(format!("{}.part.json", name));
    if full_path.exists() {
        return Err(AppError::Io(std::io::Error::other(
            "File already exists at destination",
        )));
    }
    if meta.size == 0 {
        fs::write(&full_path, &[])?;
        return Ok((full_path, 0, meta.hash));
    }
    let resume_meta = ResumeMeta {
        zone: meta.zone.clone(),
        name: meta.name.clone(),
        size: meta.size,
        hash: meta.hash.clone(),
        chunk_size: meta.chunk_size,
    };
    let mut resume_offset = 0u64;
    if part_path.exists() && meta_path.exists() {
        if let Ok(contents) = fs::read_to_string(&meta_path) {
            if let Ok(saved) = serde_json::from_str::<ResumeMeta>(&contents) {
                if saved.zone == resume_meta.zone
                    && saved.name == resume_meta.name
                    && saved.size == resume_meta.size
                    && saved.hash == resume_meta.hash
                    && saved.chunk_size == resume_meta.chunk_size
                {
                    let current_len = fs::metadata(&part_path)?.len();
                    let aligned = (current_len / meta.chunk_size) * meta.chunk_size;
                    if aligned != current_len {
                        let file = OpenOptions::new().write(true).open(&part_path)?;
                        file.set_len(aligned)?;
                    }
                    resume_offset = aligned.min(meta.size);
                }
            }
        }
    }
    if resume_offset == 0 {
        let _ = fs::remove_file(&part_path);
        let _ = fs::remove_file(&meta_path);
    }
    fs::write(&meta_path, serde_json::to_string_pretty(&resume_meta)?)?;
    if resume_offset == meta.size {
        let file_hash = sha256_file(&part_path)?;
        if file_hash == meta.hash {
            fs::rename(&part_path, &full_path)?;
            let _ = fs::remove_file(&meta_path);
            return Ok((full_path, meta.size, meta.hash));
        }
        resume_offset = 0;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&part_path)?;
    if resume_offset == 0 {
        file.set_len(0)?;
    }
    file.seek(SeekFrom::Start(resume_offset))?;
    if resume_offset > 0 {
        let _ = progress_tx
            .send(TransferResult::Progress {
                file_name: name.to_string(),
                bytes_done: resume_offset,
                bytes_total: meta.size,
            })
            .await;
    }
    let mut offset = resume_offset;
    while offset < meta.size {
        let remaining = meta.size - offset;
        let chunk_size = meta.chunk_size.min(remaining);
        let mut attempt = 0;
        let chunk = loop {
            attempt += 1;
            let chunk = p2p
                .get_file_chunk(peer_id, zone, name, offset, chunk_size)
                .await?;
            if chunk.offset != offset {
                return Err(AppError::Io(std::io::Error::other("Chunk offset mismatch")));
            }
            let hash = sha256_bytes(&chunk.data);
            if hash == chunk.hash {
                break chunk;
            }
            if attempt >= 3 {
                return Err(AppError::Io(std::io::Error::other(
                    "Chunk hash verification failed",
                )));
            }
        };
        file.seek(SeekFrom::Start(chunk.offset))?;
        file.write_all(&chunk.data)?;
        offset += chunk.data.len() as u64;
        let _ = progress_tx
            .send(TransferResult::Progress {
                file_name: name.to_string(),
                bytes_done: offset,
                bytes_total: meta.size,
            })
            .await;
    }
    file.flush()?;
    file.sync_all()?;
    let file_hash = sha256_file(&part_path)?;
    if file_hash != meta.hash {
        return Err(AppError::Io(std::io::Error::other(
            "File hash mismatch after download",
        )));
    }
    fs::rename(&part_path, &full_path)?;
    let _ = fs::remove_file(&meta_path);
    Ok((full_path, meta.size, meta.hash))
}

async fn download_rs_file(
    p2p: Option<P2PProxy>,
    peer_infos: Vec<PeerInfo>,
    file_name: &str,
    save_path: &PathBuf,
    progress_tx: &tokio::sync::mpsc::Sender<TransferResult>,
    concurrency: usize,
) -> Result<(PathBuf, u64, String)> {
    // Extract peer IDs and IP addresses for direct connections
    let peers: Vec<PeerId> = peer_infos.iter().map(|p| p.peer_id).collect();
    let peer_ips: Vec<Option<String>> = peer_infos
        .iter()
        .map(|p| {
            for addr in &p.addrs {
                if let Some(ip) = BlockClient::extract_ip(&addr.to_string()) {
                    return Some(ip);
                }
            }
            None
        })
        .collect();
    let rs_store = RsStore::new()?;
    let mut entry = rs_store
        .get_file(file_name)?
        .ok_or_else(|| AppError::Io(std::io::Error::other("RS metadata not found locally")))?;
    if entry.blocks.is_empty() {
        if let Some(ref p2p) = p2p {
            for peer in &peers {
                // Add timeout to prevent blocking on slow peers
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    p2p.rs_get_meta(*peer, file_name)
                ).await;
                if let Ok(Ok(meta)) = result {
                    rs_store.apply_remote_meta(meta.clone())?;
                    entry = meta;
                    break;
                }
            }
        }
    }
    if entry.blocks.is_empty() {
        return Err(AppError::Io(std::io::Error::other(
            "RS metadata not available from peers",
        )));
    }
    rs_store.set_syncing(&entry.name, true)?;
    let dest_path = save_path.join(&entry.name);
    if dest_path.exists() {
        return Err(AppError::Io(std::io::Error::other(
            "Destination file already exists",
        )));
    }

    // Calculate initial progress from already-synced blocks
    let mut downloaded = 0u64;
    for block in &entry.blocks {
        if rs_store.has_block(&block.hash) {
            downloaded += block.size;
        }
    }

    // Send initial progress if we have some blocks already
    if downloaded > 0 {
        let _ = progress_tx
            .send(TransferResult::Progress {
                file_name: entry.name.clone(),
                bytes_done: downloaded,
                bytes_total: entry.size,
            })
            .await;
    }

    let peer_count = peers.len();
    if peer_count == 0 {
        return Err(AppError::Io(std::io::Error::other(
            "No peers available for RS block download",
        )));
    }

    let mut availability: HashMap<String, Vec<usize>> = HashMap::new();
    if let Some(ref p2p) = p2p {
        let mut tasks = FuturesUnordered::new();
        let name = file_name.to_string();
        for (idx, peer) in peers.iter().enumerate() {
            let p2p = p2p.clone();
            let peer = *peer;
            let name = name.clone();
            tasks.push(async move { 
                // Add timeout to prevent blocking on slow peers
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    p2p.rs_have(peer, &name)
                ).await;
                (idx, result.ok().and_then(|r| r.ok()))
            });
        }
        while let Some((idx, result)) = tasks.next().await {
            if let Some(have) = result {
                for hash in have.hashes {
                    let entry = availability.entry(hash).or_default();
                    if !entry.contains(&idx) {
                        entry.push(idx);
                    }
                }
            }
        }
    }

    struct PendingBlock {
        hash: String,
        size: u64,
        attempts: u8,
    }

    #[derive(Clone)]
    struct PeerStat {
        score: f64,
        in_flight: usize,
    }

    let mut pending = VecDeque::new();
    for block in &entry.blocks {
        if rs_store.has_block(&block.hash) {
            continue;
        }
        pending.push_back(PendingBlock {
            hash: block.hash.clone(),
            size: block.size,
            attempts: 0,
        });
    }

    if pending.is_empty() {
        rs_store.reconstruct_to_path(&entry, &dest_path)?;
        entry.complete = true;
        entry.syncing = false;
        rs_store.upsert_file(entry.clone())?;
        if let Some(ref p2p) = p2p {
            let local_id = p2p.local_peer_id().to_string();
            let mut members = if !entry.members.is_empty() {
                entry.members.clone()
            } else {
                let mut ids: Vec<String> = peers.iter().map(|p| p.to_string()).collect();
                ids.push(local_id.clone());
                ids
            };
            members.sort();
            let _ = prune_unowned_blocks(&rs_store, &local_id, &members, 2);
        }
        return Ok((dest_path, entry.size, entry.hash));
    }

    let stats = Mutex::new(vec![
        PeerStat {
            score: 1.0,
            in_flight: 0
        };
        peer_count
    ]);
    let total_limit = (concurrency.max(1) * peer_count).min(32);
    let max_attempts = 6u8;
    let mut tasks = FuturesUnordered::new();
    let mut pending = pending;
    let all_indices: Vec<usize> = (0..peer_count).collect();

    loop {
        while tasks.len() < total_limit {
            let Some(block) = pending.pop_front() else {
                break;
            };
            let mut stats_guard = stats.lock().await;
            let candidates = availability
                .get(&block.hash)
                .filter(|list| !list.is_empty())
                .unwrap_or(&all_indices);
            let (peer_idx, _) = candidates
                .iter()
                .map(|idx| {
                    let stat = &stats_guard[*idx];
                    let score = stat.score / ((stat.in_flight + 1) as f64);
                    (*idx, score)
                })
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .unwrap();
            stats_guard[peer_idx].in_flight += 1;
            drop(stats_guard);

            let hash = block.hash.clone();
            let peer_ip = peer_ips[peer_idx].clone();
            let peer = peers[peer_idx];
            let p2p = p2p.clone();

            tasks.push(async move {
                let start = Instant::now();

                // Try direct connection first if we have IP
                let result = if let Some(ip) = peer_ip {
                    // Create a new client for this request (avoid mutex contention)
                    let mut client = BlockClient::new();
                    client.get_block(&ip, &hash).await
                } else if let Some(ref p2p) = p2p {
                    // Fallback to IPC
                    p2p.rs_get_block(peer, &hash).await
                } else {
                    Err(AppError::Io(std::io::Error::other(
                        "No connection available",
                    )))
                };

                let elapsed = start.elapsed();
                (peer_idx, block, result, elapsed)
            });
        }

        let Some((peer_idx, mut block, result, elapsed)) = tasks.next().await else {
            break;
        };

        {
            let mut stats_guard = stats.lock().await;
            let stat = &mut stats_guard[peer_idx];
            stat.in_flight = stat.in_flight.saturating_sub(1);
            if result.is_ok() {
                let throughput = block.size as f64 / elapsed.as_secs_f64().max(0.001);
                stat.score = stat.score * 0.8 + throughput * 0.2;
            } else {
                stat.score *= 0.85;
            }
        }

        match result {
            Ok(block_data) => {
                rs_store.verify_and_write_block(&block.hash, &block_data.data)?;
                downloaded += block.size;
                let _ = progress_tx
                    .send(TransferResult::Progress {
                        file_name: entry.name.clone(),
                        bytes_done: downloaded,
                        bytes_total: entry.size,
                    })
                    .await;
            }
            Err(_) => {
                block.attempts += 1;
                if block.attempts >= max_attempts {
                    return Err(AppError::Io(std::io::Error::other(
                        "Failed to fetch RS block from peers",
                    )));
                }
                pending.push_back(block);
            }
        }

        if pending.is_empty() && tasks.is_empty() {
            break;
        }
    }

    rs_store.reconstruct_to_path(&entry, &dest_path)?;
    entry.complete = true;
    entry.syncing = false;
    rs_store.upsert_file(entry.clone())?;

    if let Some(ref p2p) = p2p {
        let local_id = p2p.local_peer_id().to_string();
        let mut members = if !entry.members.is_empty() {
            entry.members.clone()
        } else {
            let mut ids: Vec<String> = peers.iter().map(|p| p.to_string()).collect();
            ids.push(local_id.clone());
            ids
        };
        members.sort();
        let _ = prune_unowned_blocks(&rs_store, &local_id, &members, 2);
    }

    Ok((dest_path, entry.size, entry.hash))
}
fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}
fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
/// Pending action requiring user confirmation.
#[derive(Debug, Clone)]
pub enum PendingAction {
    DeleteFile { name: String },
    RsDeleteFile { name: String },
}
/// Application state machine.
#[derive(Debug)]
pub enum AppMode {
    /// Normal command input mode
    Normal,
    /// Interactive file browser
    Browser {
        browser: FileBrowser,
        mode: BrowserMode,
        callback: BrowserCallback,
    },
    /// Waiting for confirmation (y/n)
    Confirmation {
        message: String,
        action: PendingAction,
    },
}
/// What to do after file browser completes.
#[derive(Debug, Clone)]
pub enum BrowserCallback {
    Put {
        target_name: Option<String>,
    },
    Get {
        file_name: String,
    },
    RemoteGet {
        peer_index: usize,
        zone: String,
        file_name: String,
    },
    RsPut {
        target_name: Option<String>,
    },
    RsGet {
        file_name: String,
    },
}
/// Pending async operation.
#[derive(Debug, Clone)]
pub enum PendingOperation {
    /// Query zones from a peer.
    QueryZones { peer_index: usize },
    /// Query files from a peer's zone.
    QueryFiles { peer_index: usize, zone: String },
    /// Download a file from a peer to a local path.
    DownloadFile {
        peer_index: usize,
        zone: String,
        file_name: String,
        save_path: std::path::PathBuf,
    },
    /// Query zones from selected peer.
    QuerySelectedZones,
    /// Query files from selected peer's zone.
    QuerySelectedFiles,
    /// Refresh RS list from peers.
    RsList,
    /// Query RS sync status.
    RsSyncStatus { verbose: bool },
    /// Download a RS file to a local path.
    RsDownload {
        file_name: String,
        save_path: std::path::PathBuf,
    },
}
/// Remote connection state.
#[derive(Debug, Clone, Default)]
pub struct RemoteConnection {
    /// Selected peer index (0-based).
    pub peer_index: Option<usize>,
    /// Selected remote zone.
    pub zone: Option<String>,
    /// Cached remote zones.
    pub zones_cache: Vec<String>,
    /// Cached remote files.
    pub files_cache: Vec<crate::storage::FileMetadata>,
}
/// Transfer progress state.
#[derive(Debug, Clone, Default)]
pub struct TransferProgress {
    /// Whether a transfer is active.
    pub active: bool,
    /// Transfer type: "upload" or "download".
    pub transfer_type: String,
    /// File name being transferred.
    pub file_name: String,
    /// Bytes transferred so far.
    pub bytes_done: u64,
    /// Total bytes to transfer (0 if unknown).
    pub bytes_total: u64,
}
impl TransferProgress {
    pub fn percent(&self) -> u8 {
        if self.bytes_total == 0 {
            0
        } else {
            ((self.bytes_done as f64 / self.bytes_total as f64) * 100.0) as u8
        }
    }

    pub fn format_size(bytes: u64) -> String {
        if bytes >= 1_073_741_824 {
            format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
        } else if bytes >= 1_048_576 {
            format!("{:.2} MB", bytes as f64 / 1_048_576.0)
        } else if bytes >= 1024 {
            format!("{:.2} KB", bytes as f64 / 1024.0)
        } else {
            format!("{} B", bytes)
        }
    }

    pub fn start_download(&mut self, file_name: &str, total_bytes: u64) {
        self.active = true;
        self.transfer_type = "download".to_string();
        self.file_name = file_name.to_string();
        self.bytes_done = 0;
        self.bytes_total = total_bytes;
    }

    pub fn start_upload(&mut self, file_name: &str, total_bytes: u64) {
        self.active = true;
        self.transfer_type = "upload".to_string();
        self.file_name = file_name.to_string();
        self.bytes_done = 0;
        self.bytes_total = total_bytes;
    }

    pub fn complete(&mut self) {
        self.active = false;
        self.bytes_done = self.bytes_total;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
/// Result from a background transfer.
pub enum TransferResult {
    Progress {
        file_name: String,
        bytes_done: u64,
        bytes_total: u64,
    },
    Success {
        file_name: String,
        path: std::path::PathBuf,
        size: u64,
        hash: String,
    },
    Error {
        file_name: String,
        error: String,
    },
}
/// Main application state.
pub struct App {
    pub mode: AppMode,
    pub zone_manager: ZoneManager,
    pub rs_store: RsStore,
    pub rs_mode: bool,
    pub rs_concurrency: usize,
    pub rs_sync_concurrency: usize,
    pub rs_block_size_mb: u64,
    pub rs_global_sync: bool,
    pub p2p: Option<P2PProxy>,
    pub peer_count_cache: usize,
    pub peers_cache: Vec<crate::p2p::node::PeerInfo>,
    pub pending_op: Option<PendingOperation>,
    pub rs_files_cache: Vec<RsFileEntry>,
    /// Remote connection state.
    pub remote: RemoteConnection,
    /// Transfer progress.
    pub transfer: TransferProgress,
    /// Receiver for background transfer results.
    pub transfer_rx: Option<tokio::sync::mpsc::Receiver<TransferResult>>,
    pub input_buffer: String,
    pub cursor_pos: usize,
    pub output_lines: Vec<OutputLine>,
    pub scroll_offset: usize,
    pub should_quit: bool,
    pub status_message: Option<String>,
}
impl App {
    /// Create a new application instance.
    pub fn new() -> Result<Self> {
        let config = AppConfig::load().unwrap_or_default();
        Ok(Self {
            mode: AppMode::Normal,
            zone_manager: ZoneManager::new()?,
            rs_store: RsStore::new()?,
            rs_mode: false,
            rs_concurrency: config.rs_concurrency,
            rs_sync_concurrency: config.rs_sync_concurrency,
            rs_block_size_mb: config.rs_block_size_mb,
            rs_global_sync: config.rs_global_sync,
            p2p: None,
            peer_count_cache: 0,
            peers_cache: Vec::new(),
            pending_op: None,
            rs_files_cache: Vec::new(),
            remote: RemoteConnection::default(),
            transfer: TransferProgress::default(),
            transfer_rx: None,
            input_buffer: String::new(),
            cursor_pos: 0,
            output_lines: vec![
                OutputLine::header("╔════════════════════════════════════════╗"),
                OutputLine::header(format!("║ RXIU {} - File Zone Manager ║", BUILD_VERSION)),
                OutputLine::header("║     Type 'help' for commands           ║"),
                OutputLine::header("╚════════════════════════════════════════╝"),
                OutputLine::normal(""),
            ],
            scroll_offset: 0,
            should_quit: false,
            status_message: None,
        })
    }
    /// Add output line(s).
    pub fn print(&mut self, line: OutputLine) {
        self.output_lines.push(line);
        // Auto-scroll to bottom
        self.scroll_to_bottom();
    }
    /// Add multiple lines from a string.
    pub fn print_multiline(&mut self, text: &str, style: LineStyle) {
        for line in text.lines() {
            self.output_lines.push(OutputLine {
                content: line.to_string(),
                style,
            });
        }
        self.scroll_to_bottom();
    }
    /// Scroll output to bottom.
    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.output_lines.len();
    }
    /// Set status message.
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }
    /// Clear status message.
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }
    /// Get current zone name for display.
    pub fn current_zone_name(&self) -> Option<&str> {
        if self.rs_mode {
            Some("rs")
        } else {
            self.zone_manager.active_zone_name()
        }
    }
    /// Get peer count for display.
    pub fn peer_count(&self) -> usize {
        self.peer_count_cache
    }
}

/// Run the TUI application with async runtime.
pub fn run_app() -> Result<()> {
    // Build async runtime
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(async { run_app_async().await })
}
/// Async entry point.
async fn run_app_async() -> Result<()> {
    // Setup terminal
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    // Create app
    let mut app = App::new()?;
    // Connect to P2P daemon
    match P2PProxy::connect() {
        Ok(proxy) => {
            app.print(OutputLine::success(
                "Connected to P2P daemon. Discovering peers...",
            ));
            app.p2p = Some(proxy);
        }
        Err(e) => {
            app.print(OutputLine::error(format!(
                "Failed to connect to daemon: {}",
                e
            )));
            app.print(OutputLine::info("Try running: rxiu daemon start"));
        }
    }
    // Main loop
    let result = main_loop_async(&mut terminal, &mut app).await;
    // Cleanup
    terminal::disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}
async fn main_loop_async(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<()> {
    let mut peer_update_interval = tokio::time::interval(Duration::from_secs(2));
    loop {
        // Process pending async operations (non-blocking ones)
        if let Some(op) = app.pending_op.take() {
            process_pending_operation(app, op).await;
        }

        // Check for background transfer results
        if let Some(ref mut rx) = app.transfer_rx {
            match rx.try_recv() {
                Ok(TransferResult::Progress {
                    file_name,
                    bytes_done,
                    bytes_total,
                }) => {
                    if app.transfer.file_name == file_name {
                        app.transfer.bytes_done = bytes_done;
                        app.transfer.bytes_total = bytes_total;
                    }
                }
                Ok(TransferResult::Success {
                    file_name,
                    path,
                    size,
                    hash,
                }) => {
                    app.transfer.reset();
                    app.print(OutputLine::success(format!(
                        "✅ Downloaded '{}' to {}",
                        file_name,
                        path.display()
                    )));
                    app.print(OutputLine::info(format!(
                        "   {} bytes, hash: {}...",
                        size,
                        if hash.len() >= 8 { &hash[..8] } else { &hash }
                    )));
                    app.transfer_rx = None;
                }
                Ok(TransferResult::Error { file_name, error }) => {
                    app.transfer.reset();
                    app.print(OutputLine::error(format!(
                        "❌ Failed to download '{}': {}",
                        file_name, error
                    )));
                    app.transfer_rx = None;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    // Still in progress - keep showing active transfer
                    // Don't modify bytes_done here, leave at 0 until complete
                    // The render will show a "transferring" animation
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    app.transfer.reset();
                    app.print(OutputLine::error(
                        "Download task disconnected unexpectedly.",
                    ));
                    app.transfer_rx = None;
                }
            }
        }
        // Draw
        terminal.draw(|frame| render(frame, app))?;
        tokio::select! {
            // Handle terminal events
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                if event::poll(Duration::from_millis(0))? {
                    match event::read()? {
                        Event::Key(key) => {
                            // Global quit
                            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                                app.should_quit = true;
                            }
                            handle_input(app, key)?;
                        }
                        Event::Paste(text) => {
                            let cleaned = text.replace(['\r', '\n'], " ");
                            app.input_buffer.push_str(&cleaned);
                            app.cursor_pos = app.input_buffer.len();
                        }
                        _ => {}
                    }
                }
            }
            // Update peer count periodically
            _ = peer_update_interval.tick() => {
                if let Some(ref p2p) = app.p2p {
                    let peers = p2p.get_peers().await;
                    app.peer_count_cache = peers.len();
                    app.peers_cache = peers;
                }
            }
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}
/// Process a pending async operation.
async fn process_pending_operation(app: &mut App, op: PendingOperation) {
    match op {
        PendingOperation::QueryZones { peer_index } => {
            if peer_index >= app.peers_cache.len() {
                app.print(OutputLine::error("Peer no longer available."));
                return;
            }

            let peer_id = app.peers_cache[peer_index].peer_id;
            let peer_id_short = peer_id.to_string();
            let peer_id_short = if peer_id_short.len() >= 12 {
                &peer_id_short[..12]
            } else {
                &peer_id_short
            };

            app.print(OutputLine::info(format!(
                "Querying zones from {}...",
                peer_id_short
            )));

            if let Some(ref p2p) = app.p2p {
                match p2p.list_remote_zones(peer_id).await {
                    Ok(zones) => {
                        if zones.is_empty() {
                            app.print(OutputLine::info("Peer has no zones."));
                        } else {
                            app.print(OutputLine::header(""));
                            app.print(OutputLine::header(format!(
                                "REMOTE ZONES from {}",
                                peer_id_short
                            )));
                            app.print(OutputLine::header("─".repeat(50)));
                            for zone in &zones {
                                app.print(OutputLine::normal(format!("  📁 {}", zone)));
                            }
                            app.print(OutputLine::normal(""));
                            app.print(OutputLine::info(
                                "Use 'ruse <zone>' to select a zone, then 'rlist' to see files.",
                            ));
                            // Cache zones for ruse command
                            app.remote.zones_cache = zones;
                        }
                    }
                    Err(e) => {
                        app.print(OutputLine::error(format!("Failed to query zones: {}", e)));
                    }
                }
            }
        }

        PendingOperation::QueryFiles { peer_index, zone } => {
            if peer_index >= app.peers_cache.len() {
                app.print(OutputLine::error("Peer no longer available."));
                return;
            }

            let peer_id = app.peers_cache[peer_index].peer_id;

            app.print(OutputLine::info(format!(
                "Querying files in zone '{}'...",
                zone
            )));

            if let Some(ref p2p) = app.p2p {
                match p2p.list_remote_files(peer_id, &zone).await {
                    Ok(files) => {
                        if files.is_empty() {
                            app.print(OutputLine::info("Zone is empty."));
                            app.remote.files_cache.clear();
                        } else {
                            app.print(OutputLine::header(""));
                            app.print(OutputLine::header(format!(
                                "FILES in '{}' ({} files)",
                                zone,
                                files.len()
                            )));
                            app.print(OutputLine::header("─".repeat(50)));
                            for (i, file) in files.iter().enumerate() {
                                app.print(OutputLine::normal(format!(
                                    "  [{}] 📄 {} ({})",
                                    i + 1,
                                    file.name,
                                    file.formatted_size()
                                )));
                            }
                            app.print(OutputLine::normal(""));
                            app.print(OutputLine::info("Use 'rget <number>' to download a file."));
                            // Cache files for rget command
                            app.remote.files_cache = files;
                        }
                    }
                    Err(e) => {
                        app.print(OutputLine::error(format!("Failed to list files: {}", e)));
                    }
                }
            }
        }

        PendingOperation::DownloadFile {
            peer_index,
            zone,
            file_name,
            save_path,
        } => {
            if peer_index >= app.peers_cache.len() {
                app.print(OutputLine::error("Peer no longer available."));
                return;
            }

            let peer_id = app.peers_cache[peer_index].peer_id;

            // Get expected file size from cache (if available)
            let expected_size = app
                .remote
                .files_cache
                .iter()
                .find(|f| f.name == file_name)
                .map(|f| f.size)
                .unwrap_or(0);

            // Start progress indicator
            app.transfer.start_download(&file_name, expected_size);
            app.print(OutputLine::info(format!(
                "⏳ Downloading '{}' ({})...",
                file_name,
                TransferProgress::format_size(expected_size)
            )));

            // Clone what we need for the spawned task
            let p2p = app.p2p.clone();
            let zone = zone.clone();
            let file_name = file_name.clone();
            let save_path = save_path.clone();

            // Create channel for result
            let (tx, rx) = tokio::sync::mpsc::channel::<TransferResult>(16);
            app.transfer_rx = Some(rx);

            // Spawn background download task
            tokio::spawn(async move {
                if let Some(p2p) = p2p {
                    match download_file_chunked(&p2p, peer_id, &zone, &file_name, &save_path, &tx)
                        .await
                    {
                        Ok((full_path, size, hash)) => {
                            let _ = tx
                                .send(TransferResult::Success {
                                    file_name,
                                    path: full_path,
                                    size,
                                    hash,
                                })
                                .await;
                        }
                        Err(e) => {
                            let _ = tx
                                .send(TransferResult::Error {
                                    file_name,
                                    error: e.to_string(),
                                })
                                .await;
                        }
                    }
                }
            });
        }

        PendingOperation::QuerySelectedZones => {
            if let Some(peer_index) = app.remote.peer_index {
                if peer_index >= app.peers_cache.len() {
                    app.print(OutputLine::error("Selected peer no longer available."));
                    app.remote.peer_index = None;
                    return;
                }

                let peer_id = app.peers_cache[peer_index].peer_id;

                if let Some(ref p2p) = app.p2p {
                    match p2p.list_remote_zones(peer_id).await {
                        Ok(zones) => {
                            if zones.is_empty() {
                                app.print(OutputLine::info("Remote peer has no zones."));
                            } else {
                                app.print(OutputLine::header(""));
                                app.print(OutputLine::header("REMOTE ZONES"));
                                app.print(OutputLine::header("─".repeat(50)));
                                for zone in &zones {
                                    app.print(OutputLine::normal(format!("  📁 {}", zone)));
                                }
                                app.print(OutputLine::normal(""));
                                app.print(OutputLine::info("Use 'ruse <zone>' to select a zone."));
                            }
                            app.remote.zones_cache = zones;
                        }
                        Err(e) => {
                            app.print(OutputLine::error(format!("Failed: {}", e)));
                        }
                    }
                }
            }
        }

        PendingOperation::QuerySelectedFiles => {
            if let (Some(peer_index), Some(zone)) = (app.remote.peer_index, app.remote.zone.clone())
            {
                if peer_index >= app.peers_cache.len() {
                    app.print(OutputLine::error("Selected peer no longer available."));
                    return;
                }

                let peer_id = app.peers_cache[peer_index].peer_id;

                if let Some(ref p2p) = app.p2p {
                    match p2p.list_remote_files(peer_id, &zone).await {
                        Ok(files) => {
                            if files.is_empty() {
                                app.print(OutputLine::info("Zone is empty."));
                                app.remote.files_cache.clear();
                            } else {
                                app.print(OutputLine::header(""));
                                app.print(OutputLine::header(format!(
                                    "📁 {} ({} files)",
                                    zone,
                                    files.len()
                                )));
                                app.print(OutputLine::header("─".repeat(50)));
                                for (i, file) in files.iter().enumerate() {
                                    app.print(OutputLine::normal(format!(
                                        "  [{}] 📄 {} ({})",
                                        i + 1,
                                        file.name,
                                        file.formatted_size()
                                    )));
                                }
                                app.print(OutputLine::normal(""));
                                app.print(OutputLine::info("Use 'rget <number>' to download."));
                            }
                            app.remote.files_cache = files;
                        }
                        Err(e) => {
                            app.print(OutputLine::error(format!("Failed: {}", e)));
                        }
                    }
                }
            }
        }
        PendingOperation::RsList => {
            if let Some(ref p2p) = app.p2p {
                for peer in app.peers_cache.iter().map(|p| p.peer_id) {
                    if let Ok(files) = p2p.rs_list(peer).await {
                        for file in files {
                            let _ = app.rs_store.apply_remote_meta(file);
                        }
                    }
                }
            }
            match app.rs_store.list_files() {
                Ok(files) => {
                    app.rs_files_cache = files.clone();
                    if files.is_empty() {
                        app.print(OutputLine::info("RS shared space is empty."));
                    } else {
                        app.print(OutputLine::header(""));
                        app.print(OutputLine::header(format!(
                            "RS FILES ({} files)",
                            files.len()
                        )));
                        app.print(OutputLine::header("─".repeat(50)));
                        for (i, file) in files.iter().enumerate() {
                            let status = if file.complete { "complete" } else { "partial" };
                            app.print(OutputLine::normal(format!(
                                "  [{}] 📦 {} ({} bytes, {})",
                                i + 1,
                                file.name,
                                file.size,
                                status
                            )));
                        }
                        app.print(OutputLine::normal(""));
                        app.print(OutputLine::info(
                            "Use 'rsget <number>' or 'rsget <file_name>' to download.",
                        ));
                    }
                }
                Err(e) => {
                    app.print(OutputLine::error(format!("Failed to list RS files: {}", e)));
                }
            }
        }
        PendingOperation::RsSyncStatus { verbose } => {
            let Some(ref p2p) = app.p2p else {
                app.print(OutputLine::error("P2P network not started."));
                return;
            };
            match p2p.rs_sync_status().await {
                Ok(status) => {
                    if verbose {
                        let mode = if app.rs_mode { "RS" } else { "default" };
                        let sync_state = if status.in_progress {
                            "running"
                        } else {
                            "idle"
                        };
                        app.print(OutputLine::header(""));
                        app.print(OutputLine::header("RS STATUS"));
                        app.print(OutputLine::header("─".repeat(40)));
                        app.print(OutputLine::normal(format!("  Mode: {}", mode)));
                        app.print(OutputLine::normal(format!("  Sync: {}", sync_state)));
                        app.print(OutputLine::normal(format!(
                            "  Download concurrency: {}",
                            status.download_concurrency
                        )));
                        app.print(OutputLine::normal(format!(
                            "  Sync concurrency: {}",
                            status.sync_concurrency
                        )));
                        app.print(OutputLine::normal(format!(
                            "  Global sync: {}",
                            if status.global_sync { 1 } else { 0 }
                        )));
                        app.print(OutputLine::normal(format!(
                            "  Block size: {} MB",
                            status.block_size_mb
                        )));
                        if status.last_updated_files > 0 {
                            app.print(OutputLine::normal(format!(
                                "  Last updated: {} file(s)",
                                status.last_updated_files
                            )));
                        }
                        if let Some(error) = status.last_error {
                            app.print(OutputLine::error(format!("  Last error: {}", error)));
                        }
                        if app.transfer.active {
                            app.print(OutputLine::normal(format!(
                                "  Transfer: {} {} ({}%)",
                                app.transfer.transfer_type,
                                app.transfer.file_name,
                                app.transfer.percent()
                            )));
                        } else {
                            app.print(OutputLine::normal("  Transfer: idle"));
                        }
                    } else if app.transfer.active {
                        app.print(OutputLine::info(format!(
                            "{} {} {}/{} ({}%)",
                            app.transfer.transfer_type,
                            app.transfer.file_name,
                            TransferProgress::format_size(app.transfer.bytes_done),
                            TransferProgress::format_size(app.transfer.bytes_total),
                            app.transfer.percent()
                        )));
                    } else if status.in_progress {
                        app.print(OutputLine::info("RS sync running."));
                    } else if let Some(error) = status.last_error {
                        app.print(OutputLine::error(format!("RS sync error: {}", error)));
                    } else {
                        app.print(OutputLine::info("No active RS transfers."));
                    }
                }
                Err(e) => {
                    app.print(OutputLine::error(format!(
                        "Failed to fetch RS sync status: {}",
                        e
                    )));
                }
            }
        }
        PendingOperation::RsDownload {
            file_name,
            save_path,
        } => {
            let expected_size = app
                .rs_store
                .get_file(&file_name)
                .ok()
                .and_then(|f| f.map(|e| e.size))
                .unwrap_or(0);
            app.transfer.start_download(&file_name, expected_size);
            app.print(OutputLine::info(format!(
                "⏳ Downloading RS '{}' ({})...",
                file_name,
                TransferProgress::format_size(expected_size)
            )));
            let p2p = app.p2p.clone();
            let peer_infos: Vec<PeerInfo> = app.peers_cache.clone();
            let save_path = save_path.clone();
            let (tx, rx) = tokio::sync::mpsc::channel::<TransferResult>(16);
            app.transfer_rx = Some(rx);
            let concurrency = app.rs_concurrency;
            tokio::spawn(async move {
                match download_rs_file(p2p, peer_infos, &file_name, &save_path, &tx, concurrency)
                    .await
                {
                    Ok((full_path, size, hash)) => {
                        let _ = tx
                            .send(TransferResult::Success {
                                file_name,
                                path: full_path,
                                size,
                                hash,
                            })
                            .await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(TransferResult::Error {
                                file_name,
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            });
        }
    }
}
