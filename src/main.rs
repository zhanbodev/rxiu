//! rxiu - Terminal-based file zone management system
//!
//! Entry point that initializes the TUI and handles graceful shutdown.

use rxiu::daemon::DaemonClient;
use rxiu::tui;

fn main() {
    // Handle special daemon commands
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "daemon" => {
                handle_daemon_command(&args[2..]);
                return;
            }
            _ => {}
        }
    }

    // Set up file logging for debugging
    setup_file_logging();

    // Ensure daemon is running
    ensure_daemon_running();

    // Set up panic hook for clean terminal state on crash
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Ensure terminal is restored before printing panic
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
        default_hook(info);
    }));

    // Run the TUI application
    if let Err(e) = tui::app::run_app() {
        eprintln!("Fatal error: {}", e);
        std::process::exit(1);
    }
}

fn handle_daemon_command(args: &[String]) {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");

    match cmd {
        "start" => {
            if DaemonClient::is_daemon_running() {
                println!("Daemon is already running.");
            } else {
                start_daemon();
            }
        }
        "stop" => {
            if let Ok(mut client) = DaemonClient::connect() {
                let _ = client.shutdown();
                println!("Daemon stopped.");
            } else {
                println!("Daemon is not running.");
            }
        }
        "status" => {
            if DaemonClient::is_daemon_running() {
                println!("Daemon is running on port 19820.");
            } else {
                println!("Daemon is not running.");
            }
        }
        "restart" => {
            if let Ok(mut client) = DaemonClient::connect() {
                let _ = client.shutdown();
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            start_daemon();
        }
        _ => {
            println!("Usage: rxiu daemon [start|stop|status|restart]");
        }
    }
}

fn ensure_daemon_running() {
    if !DaemonClient::is_daemon_running() {
        start_daemon();
        // Wait for daemon to be ready
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if DaemonClient::is_daemon_running() {
                return;
            }
        }
        eprintln!("Warning: Failed to start daemon");
    }
}

fn start_daemon() {
    use std::process::Command;

    // Find daemon executable
    let current_exe = std::env::current_exe().expect("Failed to get executable path");
    let daemon_exe = current_exe
        .parent()
        .map(|p| p.join("rxiu-daemon"))
        .unwrap_or_else(|| std::path::PathBuf::from("rxiu-daemon"));

    #[cfg(unix)]
    {
        let mut cmd = Command::new(&daemon_exe);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        match cmd.spawn() {
            Ok(_) => println!("Daemon started."),
            Err(e) => eprintln!("Failed to start daemon: {}", e),
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        let mut cmd = Command::new(&daemon_exe);
        cmd.creation_flags(0x00000008); // DETACHED_PROCESS

        match cmd.spawn() {
            Ok(_) => println!("Daemon started."),
            Err(e) => eprintln!("Failed to start daemon: {}", e),
        }
    }
}

fn setup_file_logging() {
    use std::fs::OpenOptions;
    use std::io::Write;

    // Create log file path
    let log_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".rxiu")
        .join("debug.log");

    // Ensure directory exists
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Simple log rotation: if log file exceeds 10MB, delete it
    if let Ok(metadata) = std::fs::metadata(&log_path) {
        if metadata.len() > 10 * 1024 * 1024 {
            let _ = std::fs::remove_file(&log_path);
        }
    }

    // Open log file (append mode)
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        // Write startup marker
        let mut f = file;
        let _ = writeln!(
            f,
            "\n\n========== RXIU STARTED {} ==========",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );

        // Set up tracing to file with log level filter
        // Default to WARN, but allow INFO for rxiu modules
        // This prevents libp2p and other verbose dependencies from flooding the log
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
