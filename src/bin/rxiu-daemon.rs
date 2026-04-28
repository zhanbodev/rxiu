//! rxiu-daemon - Background P2P service daemon.
//!
//! This runs as a separate process to maintain P2P connectivity
//! even when the TUI is not running.

use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};

use rxiu::daemon::DAEMON_PORT;
use rxiu::daemon::block_server::run_block_server;
use rxiu::daemon::server::run_server;
use rxiu::p2p::service::P2PService;
use rxiu::rs::RsStore;
use rxiu::storage::ZoneManager;

fn main() {
    // Check for command line args
    let args: Vec<String> = std::env::args().collect();
    let foreground = args.contains(&"--foreground".to_string());

    if !foreground {
        // Daemonize: fork to background
        #[cfg(unix)]
        {
            use std::process::Command;

            // Re-exec with --foreground in background
            let exe = std::env::current_exe().expect("Failed to get executable path");
            let mut cmd = Command::new(&exe);
            cmd.arg("--foreground");

            // Detach from terminal
            cmd.stdin(std::process::Stdio::null());
            cmd.stdout(std::process::Stdio::null());
            cmd.stderr(std::process::Stdio::null());

            // Use setsid to detach from controlling terminal
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::process::CommandExt;
                unsafe {
                    cmd.pre_exec(|| {
                        libc::setsid();
                        Ok(())
                    });
                }
            }

            match cmd.spawn() {
                Ok(_) => {
                    println!("Daemon started on port {}", DAEMON_PORT);
                    return;
                }
                Err(e) => {
                    eprintln!("Failed to start daemon: {}", e);
                    std::process::exit(1);
                }
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use std::process::Command;

            let exe = std::env::current_exe().expect("Failed to get executable path");
            let mut cmd = Command::new(&exe);
            cmd.arg("--foreground");

            // Detach from console on Windows
            cmd.creation_flags(0x00000008); // DETACHED_PROCESS

            match cmd.spawn() {
                Ok(_) => {
                    println!("Daemon started on port {}", DAEMON_PORT);
                    return;
                }
                Err(e) => {
                    eprintln!("Failed to start daemon: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    // Running in foreground mode
    run_daemon();
}

fn run_daemon() {
    // Setup file logging
    setup_logging();

    // Build async runtime
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    rt.block_on(async { run_daemon_async().await });
}

fn setup_logging() {
    use std::fs::OpenOptions;
    use std::io::Write;

    let log_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".rxiu")
        .join("daemon.log");

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Simple log rotation: if log file exceeds 50MB, delete it
    if let Ok(metadata) = std::fs::metadata(&log_path) {
        if metadata.len() > 50 * 1024 * 1024 {
            let _ = std::fs::remove_file(&log_path);
        }
    }

    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let mut f = file;
        let _ = writeln!(
            f,
            "\n\n========== DAEMON STARTED {} ==========",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );

        // Set up tracing with log level filter
        // Default to WARN, allow INFO for rxiu modules
        // This dramatically reduces log volume from libp2p and other dependencies
        use tracing_subscriber::EnvFilter;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .unwrap();

        let filter = EnvFilter::new("warn,rxiu=info");

        tracing_subscriber::fmt()
            .with_writer(std::sync::Mutex::new(file))
            .with_ansi(false)
            .with_target(false)
            .with_env_filter(filter)
            .init();
    }
}

async fn run_daemon_async() {
    // Write PID file
    let pid_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".rxiu")
        .join("daemon.pid");

    let _ = std::fs::write(&pid_path, std::process::id().to_string());

    tracing::info!("Daemon starting, PID: {}", std::process::id());

    // Create shared state
    let zone_manager = Arc::new(RwLock::new(
        ZoneManager::new().expect("Failed to create zone manager"),
    ));
    let rs_store = Arc::new(RwLock::new(
        RsStore::new().expect("Failed to create RS store"),
    ));

    // Start P2P service
    let p2p = match P2PService::start(zone_manager.clone(), rs_store.clone()).await {
        Ok(p2p) => {
            tracing::info!("P2P service started, local ID: {}", p2p.local_peer_id());
            p2p
        }
        Err(e) => {
            tracing::error!("Failed to start P2P service: {}", e);
            std::process::exit(1);
        }
    };

    // Shutdown signal
    let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

    // Handle Ctrl+C
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Received Ctrl+C, shutting down...");
        let _ = shutdown_tx_clone.send(());
    });

    // Start block server (for direct block downloads)
    let rs_store_clone = rs_store.clone();
    let block_shutdown_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        if let Err(e) = run_block_server(rs_store_clone, block_shutdown_rx).await {
            tracing::error!("[BlockServer] Error: {}", e);
        }
    });

    // Run the IPC server
    if let Err(e) = run_server(p2p, zone_manager, rs_store, shutdown_rx).await {
        tracing::error!("Server error: {}", e);
    }

    // Cleanup PID file
    let _ = std::fs::remove_file(&pid_path);

    tracing::info!("Daemon stopped");
}
