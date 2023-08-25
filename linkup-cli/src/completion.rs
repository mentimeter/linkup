use std::io::stdout;

use clap::{Command, CommandFactory};
use clap_complete::{generate, Generator, Shell};

use crate::{Cli, CliError};

pub fn completion(shell: &Option<Shell>) -> Result<(), CliError> {
    if let Some(shell) = shell.clone() {
        let mut cmd = Cli::command();
        print_completions(shell, &mut cmd);
        Ok(())
    } else {
        Ok(())
    }
}

fn print_completions<G: Generator>(gen: G, cmd: &mut Command) {
    generate(gen, cmd, cmd.get_name().to_string(), &mut stdout());
}
