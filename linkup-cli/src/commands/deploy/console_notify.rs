use std::io::{self, Write};

use super::cf_deploy::DeployNotifier;

pub struct ConsoleNotifier;

impl ConsoleNotifier {
    pub fn new() -> Self {
        ConsoleNotifier
    }
}

impl DeployNotifier for ConsoleNotifier {
    fn ask_confirmation(&self) -> bool {
        print!("Do you want to proceed? [y/N]: ");
        // Flush stdout to ensure prompt is shown before reading input
        io::stdout().flush().ok();

        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        let response = input.trim().to_lowercase();

        matches!(response.as_str(), "y" | "yes")
    }

    fn notify(&self, message: &str) {
        println!("{}", message);
    }
}
