//! TUI input handling.

use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use libp2p::PeerId;

use crate::cli::commands;
use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::rs::RsStore;
use crate::ui::{BrowserMode, FileBrowser};

use super::app::{App, AppMode, BrowserCallback, LineStyle, OutputLine, PendingAction, PendingOperation, TransferProgress, TransferResult};

/// Handle a key event based on current mode.
/// Only processes Press events to avoid double-input on Windows.
pub fn handle_input(app: &mut App, key: KeyEvent) -> Result<()> {
    // Windows sends both Press and Release events; only handle Press
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match &mut app.mode {
        AppMode::Normal => handle_normal_input(app, key),
        AppMode::Browser { .. } => handle_browser_input(app, key),
        AppMode::Confirmation { .. } => handle_confirmation_input(app, key),
    }
}

/// Handle input in normal command mode.
fn handle_normal_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char(c) => {
            app.input_buffer.insert(app.cursor_pos, c);
            app.cursor_pos += 1;
        }
        KeyCode::Backspace => {
            if app.cursor_pos > 0 {
                app.cursor_pos -= 1;
                app.input_buffer.remove(app.cursor_pos);
            }
        }
        KeyCode::Delete => {
            if app.cursor_pos < app.input_buffer.len() {
                app.input_buffer.remove(app.cursor_pos);
            }
        }
        KeyCode::Left => {
            if app.cursor_pos > 0 {
                app.cursor_pos -= 1;
            }
        }
        KeyCode::Right => {
            if app.cursor_pos < app.input_buffer.len() {
                app.cursor_pos += 1;
            }
        }
        KeyCode::Home => {
            app.cursor_pos = 0;
        }
        KeyCode::End => {
            app.cursor_pos = app.input_buffer.len();
        }
        KeyCode::Enter => {
            execute_command(app)?;
        }
        KeyCode::Up => {
            // Scroll up
            if app.scroll_offset > 0 {
                app.scroll_offset -= 1;
            }
        }
        KeyCode::Down => {
            // Scroll down
            if app.scroll_offset < app.output_lines.len().saturating_sub(10) {
                app.scroll_offset += 1;
            }
        }
        KeyCode::Esc => {
            app.input_buffer.clear();
            app.cursor_pos = 0;
        }
        _ => {}
    }
    Ok(())
}

/// Execute the current command.
fn execute_command(app: &mut App) -> Result<()> {
    let input = app.input_buffer.trim().to_string();
    app.input_buffer.clear();
    app.cursor_pos = 0;

    if input.is_empty() {
        return Ok(());
    }

    // Echo command
    let prompt = app.current_zone_name()
        .map(|n| format!("{}> ", n))
        .unwrap_or_else(|| "> ".to_string());
    app.print(OutputLine::info(format!("{}{}", prompt, input)));

    // Parse command
    let parts: Vec<&str> = input.split_whitespace().collect();
    let (cmd, args) = match parts.split_first() {
        Some((cmd, args)) => (*cmd, args.to_vec()),
        None => return Ok(()),
    };

    match cmd.to_lowercase().as_str() {
        "create" => match commands::create(&mut app.zone_manager, &args) {
            Ok(msg) => app.print(OutputLine::success(msg)),
            Err(e) => app.print(OutputLine::error(format!("Error: {}", e))),
        },
        "use" => match commands::use_zone(&mut app.zone_manager, &args) {
            Ok(msg) => app.print(OutputLine::success(msg)),
            Err(e) => app.print(OutputLine::error(format!("Error: {}", e))),
        },
        "list" => {
            // Check for subcommands
            if args.first().copied() == Some("area") {
                execute_list_area(app);
            } else if args.first().copied() == Some("storage") {
                execute_list_storage(app);
            } else if app.rs_mode {
                execute_rs_list(app);
            } else {
                match commands::list(&app.zone_manager) {
                    Ok(msg) => app.print_multiline(&msg, LineStyle::Normal),
                    Err(e) => app.print(OutputLine::error(format!("Error: {}", e))),
                }
            }
        },
        "get" => {
            if app.rs_mode {
                app.print(OutputLine::info("RS mode active. Use 'rsget <file_name>' instead."));
                return Ok(());
            }
            if let Some(file_name) = args.first() {
                start_get_browser(app, file_name.to_string())?;
            } else {
                app.print(OutputLine::error("Error: Missing argument: file_name"));
            }
        },
        "put" => {
            if app.rs_mode {
                app.print(OutputLine::info("RS mode active. Use 'rsput [name]' instead."));
                return Ok(());
            }
            let target_name = args.first().map(|s| s.to_string());
            start_put_browser(app, target_name)?;
        },
        "del" | "delete" | "rm" => {
            if app.rs_mode {
                app.print(OutputLine::info("RS mode active. Use 'rsdel <file_name>' instead."));
                return Ok(());
            }
            if let Some(file_name) = args.first() {
                start_delete_confirmation(app, file_name.to_string())?;
            } else {
                app.print(OutputLine::error("Error: Missing argument: file_name"));
            }
        },
        "peers" => {
            execute_peers_list(app);
        },
        "remote" => {
            // Old syntax: remote <n> to query zones (for backward compat)
            if let Some(peer_index) = args.first() {
                execute_remote_query(app, peer_index);
            } else {
                // List all peers
                execute_peers_list(app);
                app.print(OutputLine::info(""));
                app.print(OutputLine::info("Use 'ruse <n>' to select a peer, then 'rarea' to see zones."));
            }
        },
        "ruse" => {
            // ruse <n> - select peer, or ruse <zone> - select zone
            if let Some(arg) = args.first() {
                execute_ruse(app, arg);
            } else {
                // Show current selection
                show_remote_status(app);
            }
        },
        "rarea" => {
            // List remote zones from selected peer
            execute_rarea(app);
        },
        "rlist" => {
            // List files in selected zone, or rlist <zone> if specified
            if let Some(zone) = args.first() {
                // Directly list a zone
                if let Some(peer_index) = app.remote.peer_index {
                    app.remote.zone = Some((*zone).to_string());
                    app.pending_op = Some(PendingOperation::QueryFiles {
                        peer_index,
                        zone: (*zone).to_string(),
                    });
                } else {
                    app.print(OutputLine::error("No peer selected. Use 'ruse <n>' first."));
                }
            } else {
                // List files in current zone
                execute_rlist(app);
            }
        },
        "rget" => {
            // rget <n> - download file by number
            if let Some(file_num) = args.first() {
                execute_rget(app, file_num);
            } else {
                app.print(OutputLine::error("Usage: rget <file_number>"));
                app.print(OutputLine::info("First use 'rlist' to see files with numbers."));
            }
        },
        "rs" => {
            if app.rs_mode {
                app.print(OutputLine::info("Already in RS mode."));
            } else {
                app.rs_mode = true;
                app.print(OutputLine::success("Entered RS (Block Sharing) mode."));
                app.print(OutputLine::info("Use 'rslist' to see shared files."));
                if let Some(p2p) = app.p2p.clone() {
                    tokio::spawn(async move {
                        let _ = p2p.rs_sync().await;
                    });
                }
            }
        }
        "rslist" => {
            execute_rs_list(app);
        }
        "rsstatus" => {
            execute_rs_status(app);
        }
        "rsstats" => {
            execute_rs_stats(app);
        }
        "rshave" => {
            if let Some(arg) = args.first() {
                execute_rs_have(app, arg);
            } else {
                app.print(OutputLine::error("Usage: rshave <number> | rshave <file_name>"));
            }
        }
        "rspeers" => {
            execute_rs_peers(app);
        }
        "rsprogress" => {
            execute_rs_progress(app);
        }
        "rsput" => {
            let target_name = args.first().map(|s| s.to_string());
            start_rs_put_browser(app, target_name)?;
        }
        "rsget" => {
            if let Some(file_name) = args.first() {
                if let Ok(index) = file_name.parse::<usize>() {
                    if index == 0 || index > app.rs_files_cache.len() {
                        app.print(OutputLine::error("Invalid RS file number. Use 'rslist' first."));
                        return Ok(());
                    }
                    let name = app.rs_files_cache[index - 1].name.clone();
                    start_rs_get_browser(app, name)?;
                } else {
                    start_rs_get_browser(app, file_name.to_string())?;
                }
            } else {
                app.print(OutputLine::error("Usage: rsget <number> | rsget <file_name>"));
            }
        }
        "rsdel" => {
            if let Some(arg) = args.first() {
                if let Ok(index) = arg.parse::<usize>() {
                    if index == 0 || index > app.rs_files_cache.len() {
                        app.print(OutputLine::error("Invalid RS file number. Use 'rslist' first."));
                        return Ok(());
                    }
                    let name = app.rs_files_cache[index - 1].name.clone();
                    start_rs_delete_confirmation(app, name)?;
                } else {
                    start_rs_delete_confirmation(app, arg.to_string())?;
                }
            } else {
                app.print(OutputLine::error("Usage: rsdel <number> | rsdel <file_name>"));
            }
        }
        "rscfg" => {
            execute_rscfg(app, &args);
        }
        "rxiu" => {
            execute_rxiu(app);
        }
        "help" | "?" => {
            app.print_multiline(&commands::help(), LineStyle::Normal);
        },
        "exit" | "quit" | "q" => {
            app.should_quit = true;
        },
        _ => {
            app.print(OutputLine::error("Error: Invalid command. Type 'help' for available commands"));
        }
    }

    Ok(())
}

/// Execute `list area` command.
fn execute_list_area(app: &mut App) {
    // Collect to owned strings to avoid borrow issues
    let zones: Vec<String> = app.zone_manager.list_zones()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let active = app.zone_manager.active_zone_name().map(|s| s.to_string());

    if zones.is_empty() {
        app.print(OutputLine::info("No zones exist. Use 'create <name>' to create one."));
        return;
    }

    app.print(OutputLine::header(""));
    app.print(OutputLine::header("ZONE NAME           ACTIVE"));
    app.print(OutputLine::header("─".repeat(40)));

    for zone in &zones {
        let marker = if active.as_deref() == Some(zone.as_str()) { "  ●" } else { "" };
        app.print(OutputLine::normal(format!("{:<20}{}", zone, marker)));
    }

    app.print(OutputLine::normal(""));
}

/// Execute `list storage` command.
fn execute_list_storage(app: &mut App) {
    let home = match dirs::home_dir() {
        Some(p) => p,
        None => {
            app.print(OutputLine::error("Error: Could not determine home directory"));
            return;
        }
    };

    let storage_root = home.join(".rxiu");
    
    app.print(OutputLine::header(""));
    app.print(OutputLine::header("Storage Location"));
    app.print(OutputLine::header("─".repeat(50)));
    app.print(OutputLine::normal(format!("Root: {}", storage_root.display())));
    app.print(OutputLine::normal(""));

    let zones_dir = storage_root.join("zones");
    if zones_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&zones_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let zone_name = entry.file_name().to_string_lossy().to_string();
                    let files_path = entry.path().join("files");
                    app.print(OutputLine::info(format!("Zone '{}':", zone_name)));
                    app.print(OutputLine::normal(format!("  └─ {}", files_path.display())));
                }
            }
        }
    }

    app.print(OutputLine::normal(""));
}

/// Execute `peers` command - list all discovered peers with their index.
fn execute_peers_list(app: &mut App) {
    if app.p2p.is_none() {
        app.print(OutputLine::error("P2P network not started."));
        return;
    }

    if app.peers_cache.is_empty() {
        app.print(OutputLine::info("No peers discovered on LAN yet."));
        app.print(OutputLine::info("Make sure other rxiu instances are running on your network."));
        app.print(OutputLine::info(""));
        app.print(OutputLine::info("Peers are discovered automatically via mDNS."));
    } else {
        // Collect peer info first to avoid borrow issues
        let peer_count = app.peers_cache.len();
        let peer_lines: Vec<String> = app.peers_cache.iter().enumerate().map(|(i, peer)| {
            let peer_id_str = peer.peer_id.to_string();
            let short_id = if peer_id_str.len() >= 12 { &peer_id_str[..12] } else { &peer_id_str };
            let addr = peer.addrs.first()
                .map(|a| a.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            format!("  [{}] {} - {}", i + 1, short_id, addr)
        }).collect();

        app.print(OutputLine::header(""));
        app.print(OutputLine::header(format!("LAN PEERS ({})", peer_count)));
        app.print(OutputLine::header("─".repeat(60)));
        for line in peer_lines {
            app.print(OutputLine::normal(line));
        }
        app.print(OutputLine::normal(""));
    }
}

/// Execute `remote <index>` command - queue async query of peer's zones.
fn execute_remote_query(app: &mut App, index_str: &str) {
    if app.p2p.is_none() {
        app.print(OutputLine::error("P2P network not started."));
        return;
    }

    let peers_len = app.peers_cache.len();
    if peers_len == 0 {
        app.print(OutputLine::error("No peers available. Run 'peers' to see discovered peers."));
        return;
    }

    // Parse index
    let index: usize = match index_str.parse::<usize>() {
        Ok(n) if n >= 1 && n <= peers_len => n - 1,
        _ => {
            app.print(OutputLine::error(format!(
                "Invalid peer number. Use 1-{}", peers_len
            )));
            return;
        }
    };

    // Select the peer and query zones
    app.remote.peer_index = Some(index);
    app.remote.zone = None;
    app.remote.files_cache.clear();
    app.pending_op = Some(PendingOperation::QueryZones { peer_index: index });
    app.print(OutputLine::info("Querying remote zones..."));
}

/// Execute `ruse <arg>` - select peer by number or zone by name.
fn execute_ruse(app: &mut App, arg: &str) {
    if app.p2p.is_none() {
        app.print(OutputLine::error("P2P network not started."));
        return;
    }

    // Try to parse as number (peer selection)
    if let Ok(n) = arg.parse::<usize>() {
        let peers_len = app.peers_cache.len();
        if n >= 1 && n <= peers_len {
            app.remote.peer_index = Some(n - 1);
            app.remote.zone = None;
            app.remote.zones_cache.clear();
            app.remote.files_cache.clear();
            
            let peer_id_str = app.peers_cache[n - 1].peer_id.to_string();
            let short_id = if peer_id_str.len() >= 12 { &peer_id_str[..12] } else { &peer_id_str };
            app.print(OutputLine::success(format!("✓ Connected to peer [{}] {}", n, short_id)));
            app.print(OutputLine::info("Use 'rarea' to see zones."));
        } else {
            app.print(OutputLine::error(format!("Invalid peer number. Use 1-{}", peers_len)));
        }
    } else {
        // Treat as zone name
        if app.remote.peer_index.is_none() {
            app.print(OutputLine::error("No peer selected. Use 'ruse <n>' first to select a peer."));
            return;
        }
        
        app.remote.zone = Some(arg.to_string());
        app.remote.files_cache.clear();
        app.print(OutputLine::success(format!("✓ Selected zone: {}", arg)));
        app.print(OutputLine::info("Use 'rlist' to see files."));
    }
}

/// Show current remote selection status.
fn show_remote_status(app: &mut App) {
    app.print(OutputLine::header(""));
    app.print(OutputLine::header("REMOTE CONNECTION STATUS"));
    app.print(OutputLine::header("─".repeat(50)));
    
    match app.remote.peer_index {
        Some(idx) if idx < app.peers_cache.len() => {
            let peer_id_str = app.peers_cache[idx].peer_id.to_string();
            let short_id = if peer_id_str.len() >= 12 { &peer_id_str[..12] } else { &peer_id_str };
            app.print(OutputLine::normal(format!("  Peer: [{}] {}", idx + 1, short_id)));
        }
        _ => {
            app.print(OutputLine::normal("  Peer: (none)"));
        }
    }
    
    match &app.remote.zone {
        Some(z) => app.print(OutputLine::normal(format!("  Zone: {}", z))),
        None => app.print(OutputLine::normal("  Zone: (none)")),
    }
    
    app.print(OutputLine::normal(format!("  Cached files: {}", app.remote.files_cache.len())));
    app.print(OutputLine::normal(""));
    app.print(OutputLine::info("Commands: ruse <n>, rarea, rlist, rget <n>"));
}

/// Execute `rarea` - list zones from selected peer.
fn execute_rarea(app: &mut App) {
    if app.p2p.is_none() {
        app.print(OutputLine::error("P2P network not started."));
        return;
    }
    
    if app.remote.peer_index.is_none() {
        app.print(OutputLine::error("No peer selected. Use 'ruse <n>' first."));
        execute_peers_list(app);
        return;
    }
    
    app.pending_op = Some(PendingOperation::QuerySelectedZones);
    app.print(OutputLine::info("Querying zones..."));
}

/// Execute `rlist` - list files in selected zone.
fn execute_rlist(app: &mut App) {
    if app.p2p.is_none() {
        app.print(OutputLine::error("P2P network not started."));
        return;
    }
    
    if app.remote.peer_index.is_none() {
        app.print(OutputLine::error("No peer selected. Use 'ruse <n>' first."));
        return;
    }
    
    if app.remote.zone.is_none() {
        app.print(OutputLine::error("No zone selected. Use 'ruse <zone>' or 'rlist <zone>'."));
        return;
    }
    
    app.pending_op = Some(PendingOperation::QuerySelectedFiles);
    app.print(OutputLine::info("Listing files..."));
}

/// Execute `rslist` - list files in RS shared space.
fn execute_rs_list(app: &mut App) {
    app.pending_op = Some(PendingOperation::RsList);
    app.print(OutputLine::info("Listing RS files..."));
}

fn execute_rs_status(app: &mut App) {
    app.pending_op = Some(PendingOperation::RsSyncStatus { verbose: true });
    app.print(OutputLine::info("Fetching RS sync status..."));
}

fn execute_rs_stats(app: &mut App) {
    let files = match app.rs_store.list_files() {
        Ok(files) => files,
        Err(e) => {
            app.print(OutputLine::error(format!("Failed to read RS metadata: {}", e)));
            return;
        }
    };
    let total_files = files.len();
    let complete = files.iter().filter(|f| f.complete).count();
    let partial = total_files.saturating_sub(complete);
    let total_blocks: usize = files.iter().map(|f| f.blocks.len()).sum();
    let mut local_block_count = 0usize;
    let mut local_block_bytes = 0u64;
    if let Ok(entries) = fs::read_dir(app.rs_store.blocks_path()) {
        for entry in entries.flatten() {
            local_block_count += 1;
            if let Ok(meta) = entry.metadata() {
                local_block_bytes += meta.len();
            }
        }
    }
    let block_mb = app.rs_block_size_mb;
    app.print(OutputLine::header(""));
    app.print(OutputLine::header("RS STATS"));
    app.print(OutputLine::header("─".repeat(40)));
    app.print(OutputLine::normal(format!("  Files: {} ({} complete, {} partial)", total_files, complete, partial)));
    app.print(OutputLine::normal(format!("  Blocks in metadata: {}", total_blocks)));
    app.print(OutputLine::normal(format!("  Local blocks: {}", local_block_count)));
    app.print(OutputLine::normal(format!("  Local block size: {}", TransferProgress::format_size(local_block_bytes))));
    app.print(OutputLine::normal(format!("  Block size: {} MB", block_mb)));
}

fn execute_rs_have(app: &mut App, arg: &str) {
    let name = if let Ok(index) = arg.parse::<usize>() {
        if index == 0 || index > app.rs_files_cache.len() {
            app.print(OutputLine::error("Invalid RS file number. Use 'rslist' first."));
            return;
        }
        app.rs_files_cache[index - 1].name.clone()
    } else {
        arg.to_string()
    };
    let entry = match app.rs_store.get_file(&name) {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            app.print(OutputLine::error(format!("RS file '{}' not found", name)));
            return;
        }
        Err(e) => {
            app.print(OutputLine::error(format!("Failed to read RS file: {}", e)));
            return;
        }
    };
    let mut have_blocks = 0usize;
    let mut have_bytes = 0u64;
    for block in &entry.blocks {
        if app.rs_store.has_block(&block.hash) {
            have_blocks += 1;
            have_bytes += block.size;
        }
    }
    let percent = if entry.size == 0 {
        0
    } else {
        ((have_bytes as f64 / entry.size as f64) * 100.0).round() as u64
    };
    app.print(OutputLine::header(""));
    app.print(OutputLine::header(format!("RS HAVE: {}", entry.name)));
    app.print(OutputLine::header("─".repeat(40)));
    app.print(OutputLine::normal(format!("  Blocks: {}/{}", have_blocks, entry.blocks.len())));
    app.print(OutputLine::normal(format!("  Bytes: {}/{} ({}%)",
        TransferProgress::format_size(have_bytes),
        TransferProgress::format_size(entry.size),
        percent
    )));
}

fn execute_rs_peers(app: &mut App) {
    if app.p2p.is_none() {
        app.print(OutputLine::error("P2P network not started."));
        return;
    }
    
    // Trigger LAN peer refresh
    app.print(OutputLine::info("Refreshing LAN peers..."));
    if let Some(p2p) = app.p2p.clone() {
        tokio::spawn(async move {
            let _ = p2p.refresh_peers().await;
        });
    }
    
    // Show current peers (refresh happens async, user can run rspeers again to see updated list)
    if app.peers_cache.is_empty() {
        app.print(OutputLine::info("No peers discovered on LAN yet. Please wait and try again."));
        return;
    }
    let peer_count = app.peers_cache.len();
    let peer_lines: Vec<String> = app.peers_cache.iter().enumerate().map(|(i, peer)| {
        let peer_id_str = peer.peer_id.to_string();
        let short_id = if peer_id_str.len() >= 12 { &peer_id_str[..12] } else { &peer_id_str };
        let addr = peer.addrs.first()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        format!("  [{}] {} - {}", i + 1, short_id, addr)
    }).collect();
    app.print(OutputLine::header(""));
    app.print(OutputLine::header(format!("RS PEERS ({})", peer_count)));
    app.print(OutputLine::header("─".repeat(60)));
    for line in peer_lines {
        app.print(OutputLine::normal(line));
    }
    app.print(OutputLine::normal(""));
}

fn execute_rs_progress(app: &mut App) {
    app.pending_op = Some(PendingOperation::RsSyncStatus { verbose: false });
    app.print(OutputLine::info("Fetching RS sync status..."));
}

fn execute_rxiu(app: &mut App) {
    app.rs_mode = false;
    app.rs_files_cache.clear();
    app.remote = super::app::RemoteConnection::default();
    app.pending_op = None;
    app.transfer.reset();
    app.status_message = None;
    app.zone_manager.clear_active();
    app.print(OutputLine::success("Returned to default mode."));
}

fn execute_rscfg(app: &mut App, args: &[&str]) {
    let save_config = |app: &mut App| {
        let config = AppConfig {
            rs_concurrency: app.rs_concurrency,
            rs_sync_concurrency: app.rs_sync_concurrency,
            rs_block_size_mb: app.rs_block_size_mb,
            rs_global_sync: app.rs_global_sync,
        };
        if let Err(e) = config.save() {
            app.print(OutputLine::error(format!("Failed to save config: {}", e)));
        }
    };

    match args {
        ["concurrency", value] => match value.parse::<usize>() {
            Ok(v) if (2..=16).contains(&v) => {
                app.rs_concurrency = v;
                app.print(OutputLine::success(format!(
                    "RS download concurrency set to {}",
                    v
                )));
                save_config(app);
            }
            _ => {
                app.print(OutputLine::error(
                    "Usage: rscfg concurrency <2-16>",
                ));
            }
        },
        ["sync_concurrency", value] => match value.parse::<usize>() {
            Ok(v) if (2..=16).contains(&v) => {
                app.rs_sync_concurrency = v;
                app.print(OutputLine::success(format!(
                    "RS sync concurrency set to {}",
                    v
                )));
                save_config(app);
            }
            _ => {
                app.print(OutputLine::error(
                    "Usage: rscfg sync_concurrency <2-16>",
                ));
            }
        },
        ["block_size", value] => match value.parse::<u64>() {
            Ok(v) if (4..=32).contains(&v) => {
                app.rs_block_size_mb = v;
                app.print(OutputLine::success(format!(
                    "RS block size set to {} MB",
                    v
                )));
                save_config(app);
            }
            _ => {
                app.print(OutputLine::error(
                    "Usage: rscfg block_size <4-32>",
                ));
            }
        },
        ["gsyn", value] => match *value {
            "0" => {
                app.rs_global_sync = false;
                app.print(OutputLine::success("RS global sync set to 0 (RS mode only)."));
                save_config(app);
            }
            "1" => {
                app.rs_global_sync = true;
                app.print(OutputLine::success("RS global sync set to 1 (always sync)."));
                save_config(app);
            }
            _ => {
                app.print(OutputLine::error("Usage: rscfg gsyn <0|1>"));
            }
        },
        ["show"] | [] => {
            let block_mb = app.rs_block_size_mb;
            let gsyn = if app.rs_global_sync { 1 } else { 0 };
            app.print(OutputLine::info(format!(
                "RS download concurrency: {}",
                app.rs_concurrency
            )));
            app.print(OutputLine::info(format!(
                "RS sync concurrency: {}",
                app.rs_sync_concurrency
            )));
            app.print(OutputLine::info(format!(
                "RS block size: {} MB",
                block_mb
            )));
            app.print(OutputLine::info(format!(
                "RS global sync: {}",
                gsyn
            )));
            app.print(OutputLine::info("Use 'rscfg concurrency <2-16>' to change download concurrency."));
            app.print(OutputLine::info("Use 'rscfg sync_concurrency <2-16>' to change sync concurrency."));
            app.print(OutputLine::info("Use 'rscfg block_size <4-32>' to change RS block size."));
            app.print(OutputLine::info("Use 'rscfg gsyn <0|1>' to toggle global sync."));
        }
        _ => {
            app.print(OutputLine::error("Usage: rscfg show | rscfg concurrency <2-16> | rscfg sync_concurrency <2-16> | rscfg block_size <4-32> | rscfg gsyn <0|1>"));
        }
    }
}
/// Start file browser for `rsput` command.
fn start_rs_put_browser(app: &mut App, target_name: Option<String>) -> Result<()> {
    let start_dir = dirs::home_dir().ok_or(AppError::NoHomeDirectory)?;
    let browser = FileBrowser::new(&start_dir, BrowserMode::SelectFile)?;
    app.mode = AppMode::Browser {
        browser,
        mode: BrowserMode::SelectFile,
        callback: BrowserCallback::RsPut { target_name },
    };
    app.set_status("Select file to share in RS (y to confirm, q to cancel)");
    Ok(())
}

/// Start file browser for `rsget` command.
fn start_rs_get_browser(app: &mut App, file_name: String) -> Result<()> {
    let start_dir = dirs::desktop_dir()
        .or_else(dirs::home_dir)
        .ok_or(AppError::NoHomeDirectory)?;
    let browser = FileBrowser::new(&start_dir, BrowserMode::SelectDirectory)?;
    app.mode = AppMode::Browser {
        browser,
        mode: BrowserMode::SelectDirectory,
        callback: BrowserCallback::RsGet { file_name },
    };
    app.set_status("Select destination directory for RS file");
    Ok(())
}

/// Start confirmation for `rsdel` command.
fn start_rs_delete_confirmation(app: &mut App, file_name: String) -> Result<()> {
    match app.rs_store.get_file(&file_name)? {
        Some(_) => {
            app.print(OutputLine::info(format!(
                "Confirm RS delete: press 'y' to delete '{}'",
                file_name
            )));
            app.mode = AppMode::Confirmation {
                message: format!("Delete RS file '{}' from shared space? (y/N)", file_name),
                action: PendingAction::RsDeleteFile { name: file_name },
            };
        }
        None => {
            app.print(OutputLine::error(format!(
                "Error: RS file '{}' not found",
                file_name
            )));
        }
    }
    Ok(())
}

/// Execute `rget <n>` - download file by number, open browser to select save location.
fn execute_rget(app: &mut App, file_num_str: &str) {
    if app.p2p.is_none() {
        app.print(OutputLine::error("P2P network not started."));
        return;
    }
    
    if app.remote.peer_index.is_none() {
        app.print(OutputLine::error("No peer selected. Use 'ruse <n>' first."));
        return;
    }
    
    if app.remote.zone.is_none() {
        app.print(OutputLine::error("No zone selected. Use 'ruse <zone>' first."));
        return;
    }
    
    if app.remote.files_cache.is_empty() {
        app.print(OutputLine::error("No files cached. Use 'rlist' first."));
        return;
    }
    
    // Parse file number
    let file_num: usize = match file_num_str.parse::<usize>() {
        Ok(n) if n >= 1 && n <= app.remote.files_cache.len() => n - 1,
        _ => {
            app.print(OutputLine::error(format!(
                "Invalid file number. Use 1-{}", app.remote.files_cache.len()
            )));
            return;
        }
    };
    
    let file_name = app.remote.files_cache[file_num].name.clone();
    let peer_index = app.remote.peer_index.unwrap();
    let zone = app.remote.zone.clone().unwrap();
    
    // Try directories in order, checking if we can actually access them
    let possible_dirs = [
        dirs::home_dir(),
        dirs::desktop_dir(),
        dirs::download_dir(),
        Some(std::path::PathBuf::from(".")),
    ];
    
    let start_dir = possible_dirs.into_iter()
        .flatten()
        .find(|p| p.exists() && std::fs::read_dir(p).is_ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    
    match FileBrowser::new(&start_dir, BrowserMode::SelectDirectory) {
        Ok(browser) => {
            app.mode = AppMode::Browser {
                browser,
                mode: BrowserMode::SelectDirectory,
                callback: BrowserCallback::RemoteGet {
                    peer_index,
                    zone,
                    file_name: file_name.clone(),
                },
            };
            app.set_status(format!("Select save location for '{}'", file_name));
        }
        Err(e) => {
            // If browser still fails, offer to save to current directory
            app.print(OutputLine::error(format!("Failed to open directory browser: {}", e)));
            app.print(OutputLine::info("Try running from a directory you have permission to access."));
        }
    }
}



/// Start file browser for `get` command.
fn start_get_browser(app: &mut App, file_name: String) -> Result<()> {
    // Verify file exists
    let zone = app.zone_manager.active_zone()?;
    if !zone.exists(&file_name) {
        app.print(OutputLine::error(format!("Error: File '{}' not found in zone", file_name)));
        return Ok(());
    }

    let start_dir = dirs::desktop_dir()
        .or_else(dirs::home_dir)
        .ok_or(AppError::NoHomeDirectory)?;

    let browser = FileBrowser::new(&start_dir, BrowserMode::SelectDirectory)?;

    app.mode = AppMode::Browser {
        browser,
        mode: BrowserMode::SelectDirectory,
        callback: BrowserCallback::Get { file_name },
    };

    app.set_status("Select destination directory (y to confirm, q to cancel)");
    Ok(())
}

/// Start file browser for `put` command.
fn start_put_browser(app: &mut App, target_name: Option<String>) -> Result<()> {
    // Verify zone is active
    let _ = app.zone_manager.active_zone()?;

    let start_dir = dirs::home_dir().ok_or(AppError::NoHomeDirectory)?;
    let browser = FileBrowser::new(&start_dir, BrowserMode::SelectFile)?;

    app.mode = AppMode::Browser {
        browser,
        mode: BrowserMode::SelectFile,
        callback: BrowserCallback::Put { target_name },
    };

    app.set_status("Select file to import (y to confirm, q to cancel)");
    Ok(())
}

/// Start delete confirmation.
fn start_delete_confirmation(app: &mut App, file_name: String) -> Result<()> {
    // Verify file exists
    let zone = app.zone_manager.active_zone()?;
    if !zone.exists(&file_name) {
        app.print(OutputLine::error(format!("Error: File '{}' not found in zone", file_name)));
        return Ok(());
    }

    let zone_name = app.zone_manager.active_zone_name()
        .map(|s| s.to_string())
        .unwrap_or_default();

    app.mode = AppMode::Confirmation {
        message: format!("Delete '{}' from zone '{}'? (y/N)", file_name, zone_name),
        action: PendingAction::DeleteFile { name: file_name },
    };

    Ok(())
}

/// Handle input in browser mode.
fn handle_browser_input(app: &mut App, key: KeyEvent) -> Result<()> {
    // Extract browser temporarily
    let (mut browser, mode, callback) = match std::mem::replace(&mut app.mode, AppMode::Normal) {
        AppMode::Browser { browser, mode, callback } => (browser, mode, callback),
        other => {
            app.mode = other;
            return Ok(());
        }
    };

    match browser.handle_key_tui(key) {
        Ok(Some(selected_path)) => {
            // Selection made
            app.clear_status();
            complete_browser_action(app, selected_path, callback)?;
        }
        Ok(None) => {
            // Continue browsing
            app.mode = AppMode::Browser { browser, mode, callback };
        }
        Err(AppError::Cancelled) => {
            app.clear_status();
            app.print(OutputLine::info("Cancelled."));
        }
        Err(e) => {
            app.clear_status();
            app.print(OutputLine::error(format!("Error: {}", e)));
        }
    }

    Ok(())
}

/// Complete the action after browser selection.
fn complete_browser_action(app: &mut App, path: PathBuf, callback: BrowserCallback) -> Result<()> {
    match callback {
        BrowserCallback::Get { file_name } => {
            // Get file content
            let zone = app.zone_manager.active_zone()?;
            let content = zone.retrieve(&file_name)?;

            // Write to destination
            let dest_path = path.join(&file_name);
            if dest_path.exists() {
                app.print(OutputLine::error(format!(
                    "Error: File '{}' already exists at destination",
                    file_name
                )));
                return Ok(());
            }

            std::fs::write(&dest_path, &content)?;
            app.print(OutputLine::success(format!(
                "Saved '{}' to {}",
                file_name,
                dest_path.display()
            )));
        }
        BrowserCallback::Put { target_name } => {
            // Read source file
            let content = std::fs::read(&path)?;
            let name = target_name
                .or_else(|| path.file_name().map(|n| n.to_string_lossy().to_string()))
                .ok_or_else(|| AppError::NotAFile(path.clone()))?;

            // Store in zone
            let zone = app.zone_manager.active_zone()?;
            if zone.exists(&name) {
                app.print(OutputLine::error(format!(
                    "Error: File '{}' already exists in zone",
                    name
                )));
                return Ok(());
            }

            let metadata = zone.store(&name, &content)?;
            app.print(OutputLine::success(format!(
                "Imported '{}' ({})",
                metadata.name,
                metadata.formatted_size()
            )));
        }
        BrowserCallback::RemoteGet { peer_index, zone, file_name } => {
            // Queue the download operation with the selected save path
            app.pending_op = Some(PendingOperation::DownloadFile {
                peer_index,
                zone,
                file_name: file_name.clone(),
                save_path: path,
            });
            app.print(OutputLine::info(format!("Downloading '{}'...", file_name)));
        }
        BrowserCallback::RsPut { target_name } => {
            let name = target_name
                .or_else(|| path.file_name().map(|n| n.to_string_lossy().to_string()))
                .ok_or_else(|| AppError::NotAFile(path.clone()))?;
            if app.rs_store.get_file(&name)?.is_some() {
                app.print(OutputLine::error(format!(
                    "Error: RS file '{}' already exists",
                    name
                )));
                return Ok(());
            }

            // Get file size for progress
            let file_size = std::fs::metadata(&path)?.len();
            
            // Start upload progress
            app.transfer.start_upload(&name, file_size);
            app.print(OutputLine::info(format!(
                "⏳ Uploading '{}' ({})...",
                name,
                crate::tui::app::TransferProgress::format_size(file_size)
            )));

            let (members, algo) = if let Some(ref p2p) = app.p2p {
                let mut ids: Vec<String> = app.peers_cache.iter().map(|p| p.peer_id.to_string()).collect();
                ids.push(p2p.local_peer_id().to_string());
                ids.sort();
                (ids, "hrw-v1".to_string())
            } else {
                (Vec::new(), "legacy".to_string())
            };

            // Clone what we need for background task
            let p2p = app.p2p.clone();
            let peers: Vec<PeerId> = app.peers_cache.iter().map(|p| p.peer_id).collect();
            let path = path.clone();
            let name = name.clone();
            let block_size_bytes = app.rs_block_size_mb * 1024 * 1024;
            
            // Create channel for result
            let (tx, rx) = tokio::sync::mpsc::channel::<TransferResult>(16);
            app.transfer_rx = Some(rx);

            // Spawn background upload task
            tokio::spawn(async move {
                // Create a new RsStore for the background task
                let rs_store = match RsStore::new() {
                    Ok(store) => store,
                    Err(e) => {
                        let _ = tx.send(TransferResult::Error {
                            file_name: name,
                            error: e.to_string(),
                        }).await;
                        return;
                    }
                };

                // Put file to RS store
                let entry = match rs_store.put_file_from_path(&path, &name, members, &algo, block_size_bytes) {
                    Ok(entry) => entry,
                    Err(e) => {
                        let _ = tx.send(TransferResult::Error {
                            file_name: name,
                            error: e.to_string(),
                        }).await;
                        return;
                    }
                };

                // Send progress update
                let _ = tx.send(TransferResult::Progress {
                    file_name: name.clone(),
                    bytes_done: file_size / 2,
                    bytes_total: file_size,
                }).await;

                // Announce to peers
                if let Some(p2p) = p2p {
                    for peer in peers {
                        let _ = p2p.rs_announce(peer, entry.clone()).await;
                    }
                }

                // Send success
                let hash = entry.blocks.first().map(|b| b.hash.clone()).unwrap_or_default();
                let _ = tx.send(TransferResult::Success {
                    file_name: name,
                    path,
                    size: file_size,
                    hash,
                }).await;
            });
        }
        BrowserCallback::RsGet { file_name } => {
            app.pending_op = Some(PendingOperation::RsDownload {
                file_name: file_name.clone(),
                save_path: path,
            });
            app.print(OutputLine::info(format!("Downloading RS file '{}'...", file_name)));
        }
    }
    Ok(())
}

/// Handle input in confirmation mode.
fn handle_confirmation_input(app: &mut App, key: KeyEvent) -> Result<()> {
    let (_message, action) = match std::mem::replace(&mut app.mode, AppMode::Normal) {
        AppMode::Confirmation { message, action } => (message, action),
        other => {
            app.mode = other;
            return Ok(());
        }
    };

    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            execute_pending_action(app, action)?;
        }
        _ => {
            app.print(OutputLine::info("Cancelled."));
        }
    }

    Ok(())
}

/// Execute a confirmed pending action.
fn execute_pending_action(app: &mut App, action: PendingAction) -> Result<()> {
    match action {
        PendingAction::DeleteFile { name } => {
            let zone = app.zone_manager.active_zone()?;
            zone.delete(&name)?;
            app.print(OutputLine::success(format!("Deleted '{}'", name)));
        }
        PendingAction::RsDeleteFile { name } => {
            app.rs_store.remove_file(&name)?;
            app.print(OutputLine::success(format!("Deleted RS file '{}'", name)));
            if let Some(p2p) = app.p2p.clone() {
                let peers: Vec<PeerId> = app.peers_cache.iter().map(|p| p.peer_id).collect();
                tokio::spawn(async move {
                    for peer in peers {
                        let _ = p2p.rs_delete(peer, &name).await;
                    }
                });
            }
        }
    }
    Ok(())
}
