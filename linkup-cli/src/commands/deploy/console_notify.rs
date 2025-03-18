use crate::prompt;

use super::cf_deploy::DeployNotifier;

pub struct ConsoleNotifier;

impl ConsoleNotifier {
    pub fn new() -> Self {
        ConsoleNotifier
    }
}

impl DeployNotifier for ConsoleNotifier {
    fn ask_confirmation(&self) -> bool {
        let response = prompt("Do you want to proceed? [y/N]: ")
            .trim()
            .to_lowercase();

        matches!(response.as_str(), "y" | "yes")
    }

    fn notify(&self, message: &str) {
        println!("{}", message);
    }
}
