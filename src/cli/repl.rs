//! Interactive REPL (Read-Eval-Print Loop) for the file zone manager.
//!
//! Provides the main interaction loop with prompt, command parsing,
//! and result/error display.

use std::io::{self, BufRead, Write};

use crate::error::{AppError, Result};
use crate::storage::ZoneManager;

use super::commands;

/// The main REPL controller.
pub struct Repl {
    manager: ZoneManager,
    running: bool,
}

impl Repl {
    /// Create a new REPL with fresh zone manager.
    pub fn new() -> Result<Self> {
        Ok(Self {
            manager: ZoneManager::new()?,
            running: true,
        })
    }

    /// Run the REPL loop until exit.
    pub fn run(&mut self) -> Result<()> {
        self.print_banner();

        let stdin = io::stdin();
        let mut stdout = io::stdout();

        while self.running {
            // Print prompt
            self.print_prompt(&mut stdout)?;

            // Read input
            let mut input = String::new();
            if stdin.lock().read_line(&mut input).is_err() {
                break;
            }

            // Handle EOF (Ctrl+D)
            if input.is_empty() {
                println!();
                break;
            }

            // Process command
            let input = input.trim();
            if input.is_empty() {
                continue;
            }

            match self.execute(input) {
                Ok(Some(output)) => println!("{}", output),
                Ok(None) => {}
                Err(e) => eprintln!("Error: {}", e),
            }
        }

        println!("Goodbye!");
        Ok(())
    }

    /// Print the startup banner.
    fn print_banner(&self) {
        println!();
        println!("╔════════════════════════════════════════╗");
        println!("║     RXIU - File Zone Manager         ║");
        println!("║     Type 'help' for commands           ║");
        println!("╚════════════════════════════════════════╝");
        println!();
    }

    /// Print the command prompt.
    fn print_prompt(&self, stdout: &mut io::Stdout) -> Result<()> {
        match self.manager.active_zone_name() {
            Some(name) => print!("{}> ", name),
            None => print!("> "),
        }
        stdout.flush()?;
        Ok(())
    }

    /// Parse and execute a command.
    fn execute(&mut self, input: &str) -> Result<Option<String>> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let (cmd, args) = match parts.split_first() {
            Some((cmd, args)) => (*cmd, args),
            None => return Ok(None),
        };

        match cmd.to_lowercase().as_str() {
            "create" => Ok(Some(commands::create(&mut self.manager, args)?)),
            "use" => Ok(Some(commands::use_zone(&mut self.manager, args)?)),
            "list" | "ls" => Ok(Some(commands::list(&self.manager)?)),
            "get" => Ok(Some(commands::get(&self.manager, args)?)),
            "put" => Ok(Some(commands::put(&self.manager, args)?)),
            "help" | "?" => Ok(Some(commands::help())),
            "exit" | "quit" | "q" => {
                self.running = false;
                Ok(None)
            }
            _ => Err(AppError::InvalidCommand),
        }
    }
}
