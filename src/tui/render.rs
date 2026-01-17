//! TUI rendering.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Clear};

use super::app::{App, AppMode, LineStyle};

/// Render the application to the frame.
pub fn render(frame: &mut Frame, app: &App) {
    match &app.mode {
        AppMode::Browser { browser, .. } => {
            render_browser(frame, browser);
        }
        AppMode::Confirmation { message, .. } => {
            render_confirmation(frame, app, message);
        }
        AppMode::Normal => {
            render_normal(frame, app);
        }
    }
}

/// Render normal command mode.
fn render_normal(frame: &mut Frame, app: &App) {
    let area = frame.size();

    // Layout: Header (1), Output (flex), Input (3), Status (1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Header
            Constraint::Min(5),     // Output
            Constraint::Length(3),  // Input
            Constraint::Length(1),  // Status
        ])
        .split(area);

    // Header
    render_header(frame, chunks[0], app);

    // Output area
    render_output(frame, chunks[1], app);

    // Input area
    render_input(frame, chunks[2], app);

    // Status bar
    render_status(frame, chunks[3], app);
}

/// Render the header bar.
fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let theme_color = Color::Rgb(0xE7, 0x6F, 0x51);
    let zone_name = app.current_zone_name().unwrap_or("no zone");
    let peer_count = app.peer_count();
    let mode_tag = if app.rs_mode { "RS" } else { "ZONE" };

    let left = " RXIU v0.3.0".to_string();
    let right = format!("[{}:{}] 🔗 LAN: {} ", mode_tag, zone_name, peer_count);

    let header_text = format!("{:<width$}{}", left, right, width = area.width as usize - right.len());

    let header = Paragraph::new(header_text)
        .style(Style::default().bg(theme_color).fg(Color::Black));

    frame.render_widget(header, area);
}

/// Render the output area with scrolling.
fn render_output(frame: &mut Frame, area: Rect, app: &App) {
    let inner_height = area.height.saturating_sub(2) as usize;

    // Calculate visible range
    let total_lines = app.output_lines.len();
    let start = app.scroll_offset.min(total_lines.saturating_sub(inner_height));
    let end = (start + inner_height).min(total_lines);

    let mut lines: Vec<Line> = Vec::new();
    for output_line in app.output_lines.iter().skip(start).take(end - start) {
        let style = match output_line.style {
            LineStyle::Normal => Style::default(),
            LineStyle::Success => Style::default().fg(Color::Green),
            LineStyle::Error => Style::default().fg(Color::Red),
            LineStyle::Info => Style::default().fg(Color::Cyan),
            LineStyle::Header => Style::default().fg(Color::Yellow).bold(),
        };
        lines.push(Line::styled(&output_line.content, style));
    }

    let output = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Output "));

    frame.render_widget(output, area);

    // Scrollbar
    if total_lines > inner_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines)
            .position(start);

        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        let scrollbar_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };
        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

/// Render the input area.
fn render_input(frame: &mut Frame, area: Rect, app: &App) {
    let prompt = app.current_zone_name()
        .map(|n| format!("{}> ", n))
        .unwrap_or_else(|| "> ".to_string());

    let input_text = format!("{}{}", prompt, app.input_buffer);

    let input = Paragraph::new(input_text.clone())
        .block(Block::default().borders(Borders::ALL).title(" Command "));

    frame.render_widget(input, area);

    // Show cursor
    let cursor_x = area.x + 1 + prompt.len() as u16 + app.cursor_pos as u16;
    let cursor_y = area.y + 1;
    frame.set_cursor(cursor_x, cursor_y);
}

/// Render the status bar.
fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let theme_color = Color::Rgb(0xE7, 0x6F, 0x51);
    let status_text = if app.transfer.active {
        // Show progress bar during transfer
        let percent = app.transfer.percent();
        let done_str = super::app::TransferProgress::format_size(app.transfer.bytes_done);
        let total_str = super::app::TransferProgress::format_size(app.transfer.bytes_total);
        
        // Build progress bar
        let bar_width = 20;
        let bar = if app.transfer.bytes_done == 0 && app.transfer.bytes_total > 0 {
            // Pulse animation for "in progress" state
            // Use time-based animation (simple toggle based on system time)
            let pulse_pos = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() / 200) as usize % bar_width;
            
            let mut chars: Vec<char> = "░".repeat(bar_width).chars().collect();
            chars[pulse_pos] = '█';
            if pulse_pos > 0 { chars[pulse_pos - 1] = '▓'; }
            if pulse_pos < bar_width - 1 { chars[pulse_pos + 1] = '▓'; }
            format!("[{}]", chars.into_iter().collect::<String>())
        } else {
            // Normal progress bar
            let filled = (bar_width * percent as usize) / 100;
            let empty = bar_width - filled;
            format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
        };
        
        format!(
            " {} {} {} / {} {} {}%",
            if app.transfer.transfer_type == "download" { "⬇" } else { "⬆" },
            app.transfer.file_name,
            done_str,
            total_str,
            bar,
            percent,
        )
    } else if let Some(ref msg) = app.status_message {
        format!(" {}", msg)
    } else {
        " j/k: scroll  Enter: execute  Ctrl+C: quit  ?: help".to_string()
    };

    let bg_color = if app.transfer.active {
        Color::Rgb(30, 80, 30)
    } else {
        theme_color
    };

    let status_bar = Paragraph::new(status_text)
        .style(Style::default().bg(bg_color).fg(Color::Black));

    frame.render_widget(status_bar, area);
}

/// Render file browser mode.
fn render_browser(frame: &mut Frame, browser: &crate::ui::FileBrowser) {
    let area = frame.size();

    // Get browser content
    let (current_dir, entries, selected) = browser.get_state();

    // Layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header with path
            Constraint::Min(5),     // File list
            Constraint::Length(1),  // Help
        ])
        .split(area);

    // Header with current path
    let path_text = format!(" 📂 {}", current_dir.display());
    let header = Paragraph::new(path_text)
        .block(Block::default().borders(Borders::ALL).title(" Navigate "));
    frame.render_widget(header, chunks[0]);

    // File list
    let inner_height = chunks[1].height.saturating_sub(2) as usize;
    let scroll_offset = if selected >= inner_height {
        selected - inner_height + 1
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();
    for (i, entry) in entries.iter().enumerate().skip(scroll_offset).take(inner_height) {
        let is_selected = i == selected;
        let icon = if entry.is_dir { "📁" } else { "📄" };
        let name = if entry.is_dir {
            format!("{}/", entry.name)
        } else {
            entry.name.clone()
        };

        let prefix = if is_selected { " ▶ " } else { "   " };
        let line_text = format!("{}{} {}", prefix, icon, name);

        let style = if is_selected {
            Style::default().fg(Color::Cyan).bold()
        } else if entry.is_dir {
            Style::default().fg(Color::Blue)
        } else {
            Style::default()
        };

        lines.push(Line::styled(line_text, style));
    }

    let file_list = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Files "));
    frame.render_widget(file_list, chunks[1]);

    // Help
    let help = Paragraph::new(" j/k: navigate  Enter/l: open  h/Backspace: back  y: confirm  q: cancel")
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(help, chunks[2]);
}

/// Render confirmation dialog.
fn render_confirmation(frame: &mut Frame, app: &App, message: &str) {
    let area = frame.size();

    // First render normal view as background
    render_normal(frame, app);

    // Overlay confirmation dialog
    let dialog_width = (message.len() + 6).min(area.width as usize - 4) as u16;
    let dialog_height = 5;

    let dialog_area = Rect {
        x: (area.width - dialog_width) / 2,
        y: (area.height - dialog_height) / 2,
        width: dialog_width,
        height: dialog_height,
    };

    let dialog = Paragraph::new(vec![
        Line::from(""),
        Line::styled(message, Style::default().fg(Color::Yellow)),
        Line::from(""),
        Line::styled("  [y] Yes    [n/Esc] No", Style::default().fg(Color::Gray)),
    ])
    .block(Block::default()
        .borders(Borders::ALL)
        .title(" Confirm ")
        .style(Style::default().bg(Color::Black)));

    // Clear area first
    frame.render_widget(Clear, dialog_area);
    frame.render_widget(dialog, dialog_area);
}
