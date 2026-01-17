//! Command implementations for the REPL.
//!
//! Each command is a function that takes parsed arguments and the zone manager,
//! executes the operation, and returns a result or error message.

use std::fs;
use std::io::Write;

use crate::error::{AppError, Result};
use crate::storage::ZoneManager;
use crate::ui::{BrowserMode, FileBrowser};

/// Create a new zone.
pub fn create(manager: &mut ZoneManager, args: &[&str]) -> Result<String> {
    let name = args.first().ok_or(AppError::MissingArgument("zone_name"))?;

    // Validate zone name (alphanumeric and underscores only)
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Ok("Error: Zone name can only contain letters, numbers, underscores, and hyphens".to_string());
    }

    manager.create_zone(name)?;
    Ok(format!("Zone '{}' created successfully.", name))
}

/// Switch to a zone.
pub fn use_zone(manager: &mut ZoneManager, args: &[&str]) -> Result<String> {
    let name = args.first().ok_or(AppError::MissingArgument("zone_name"))?;
    manager.set_active(name)?;
    Ok(format!("Switched to zone '{}'.", name))
}

/// List files in the current zone.
pub fn list(manager: &ZoneManager) -> Result<String> {
    let zone = manager.active_zone()?;
    let files = zone.list()?;

    if files.is_empty() {
        return Ok("Zone is empty.".to_string());
    }

    // Calculate column widths
    let max_name = files.iter().map(|f| f.name.len()).max().unwrap_or(10).max(10);
    let max_size = 10;

    // Build table
    let mut output = String::new();
    output.push_str(&format!(
        "\n{:<width$}  {:>size_w$}  {}\n",
        "NAME",
        "SIZE",
        "IMPORTED",
        width = max_name,
        size_w = max_size
    ));
    output.push_str(&format!("{}\n", "─".repeat(max_name + max_size + 25)));

    for file in &files {
        let imported = file.imported_at.format("%Y-%m-%d %H:%M").to_string();
        output.push_str(&format!(
            "{:<width$}  {:>size_w$}  {}\n",
            file.name,
            file.formatted_size(),
            imported,
            width = max_name,
            size_w = max_size
        ));
    }

    output.push_str(&format!("\n{} file(s) total", files.len()));
    Ok(output)
}

/// Get a file from the zone and save to a chosen location.
pub fn get(manager: &ZoneManager, args: &[&str]) -> Result<String> {
    let file_name = args.first().ok_or(AppError::MissingArgument("file_name"))?;

    let zone = manager.active_zone()?;

    // Check file exists
    if !zone.exists(file_name) {
        return Err(AppError::FileNotFound(file_name.to_string()));
    }

    // Get content first
    let content = zone.retrieve(file_name)?;

    // Start file browser at Desktop
    let start_dir = dirs::desktop_dir()
        .or_else(dirs::home_dir)
        .ok_or(AppError::NoHomeDirectory)?;

    println!("\nSelect destination directory...");
    println!("(Press 'y' to confirm, 'q' to cancel)\n");

    let mut browser = FileBrowser::new(&start_dir, BrowserMode::SelectDirectory)?;
    let dest_dir = browser.run()?;

    // Write file to destination
    let dest_path = dest_dir.join(file_name);

    if dest_path.exists() {
        return Ok(format!(
            "Error: File '{}' already exists at destination. Choose a different location.",
            file_name
        ));
    }

    let mut file = fs::File::create(&dest_path)?;
    file.write_all(&content)?;

    Ok(format!(
        "File '{}' saved to {}",
        file_name,
        dest_path.display()
    ))
}

/// Put a file into the zone from a chosen location.
pub fn put(manager: &ZoneManager, args: &[&str]) -> Result<String> {
    // The argument is the name to use in the zone (optional - defaults to source filename)
    let target_name = args.first().copied();

    let zone = manager.active_zone()?;

    // Start file browser at home directory
    let start_dir = dirs::home_dir().ok_or(AppError::NoHomeDirectory)?;

    println!("\nSelect file to import...");
    println!("(Navigate to file and press 'y' to confirm, 'q' to cancel)\n");

    let mut browser = FileBrowser::new(&start_dir, BrowserMode::SelectFile)?;
    let source_path = browser.run()?;

    // Determine the name to use in the zone
    let file_name = target_name
        .map(String::from)
        .or_else(|| source_path.file_name().map(|n| n.to_string_lossy().to_string()))
        .ok_or(AppError::NotAFile(source_path.clone()))?;

    // Check if file already exists in zone
    if zone.exists(&file_name) {
        return Err(AppError::FileAlreadyExists(file_name));
    }

    // Read content and store
    let content = fs::read(&source_path)?;
    let metadata = zone.store(&file_name, &content)?;

    Ok(format!(
        "Imported '{}' ({}) into zone.",
        metadata.name,
        metadata.formatted_size()
    ))
}

/// Show help message.
pub fn help() -> String {
    r#"
Available commands:

  create <zone_name>  Create a new file zone
  use <zone_name>     Switch to a file zone
  list                List files in the current zone
  list area           List all file zones
  list storage        Show storage locations
  get <file_name>     Export a file from the zone
  put [name]          Import a file into the zone
  del <file_name>     Delete a file from the zone

P2P Commands:
  peers               Show discovered LAN peers
  ruse <n>            Select peer #n to connect
  ruse <zone>         Select a remote zone
  rarea               List remote zones
  rlist               List files in selected zone
  rget <n>            Download file #n (opens save dialog)

RS (Block Sharing) Commands:
  rs                  Enter RS mode
  rslist              List RS shared files
  rsput [name]        Share a file into RS space
  rsget <number>      Download RS file by number
  rsget <file_name>   Download a file from RS space
  rsdel <number>      Delete RS file by number (propagates to peers)
  rsdel <file_name>   Delete a file from RS space (propagates to peers)
  rsstatus            Show RS mode/sync/transfer status
  rsstats             Show RS local stats (files/blocks/size)
  rshave <n|name>      Show local blocks for an RS file
  rspeers             Refresh and show RS peer list
  rsprogress          Show current RS transfer/sync progress
  rscfg show               Show RS settings
  rscfg concurrency N       Set RS download concurrency (2-16)
  rscfg sync_concurrency N  Set RS sync concurrency (2-16)
  rscfg block_size N        Set RS block size in MB (4-32)
  rscfg gsyn <0|1>          Set RS global sync (0=RS mode only, 1=always sync)
  rxiu                     Return to default mode

  help                Show this help message
  exit, quit          Exit the program

Navigation (in file browser):
  j/↓         Move down
  k/↑         Move up
  Enter/l     Enter directory
  h/Backspace Go to parent directory
  y           Confirm selection
  q/Esc       Cancel
"#
    .to_string()
}
