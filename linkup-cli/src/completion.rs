use std::io::stdout;

use clap::{Command, CommandFactory};
use clap_complete::{generate, Generator, Shell};

use crate::{Cli, CliError};

pub fn completion(shell: &Option<Shell>) -> Result<(), CliError> {
    if let Some(shell) = shell {
        let mut cmd = Cli::command();
        print_completions(shell, &mut cmd);
    }
    Ok(())
}
fn print_completions<G: Generator + Clone>(gen: &G, cmd: &mut Command) {
    generate(gen.clone(), cmd, cmd.get_name().to_string(), &mut stdout());
}
