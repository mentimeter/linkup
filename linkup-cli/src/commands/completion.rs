use std::io::stdout;

use clap::{Command, CommandFactory};
use clap_complete::{generate, Generator, Shell};

use crate::{Cli, CliError};

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, value_enum)]
    shell: Option<Shell>,
}

pub fn completion(args: &Args) -> Result<(), CliError> {
    if let Some(shell) = &args.shell {
        let mut cmd = Cli::command();
        print_completions(shell, &mut cmd);
    }

    Ok(())
}
fn print_completions<G: Generator + Clone>(gen: &G, cmd: &mut Command) {
    generate(gen.clone(), cmd, cmd.get_name().to_string(), &mut stdout());
}
