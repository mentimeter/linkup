use crate::prompt;

pub trait DeployNotifier {
    fn ask_confirmation(&self) -> bool;
    fn notify(&self, message: &str);
}

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
