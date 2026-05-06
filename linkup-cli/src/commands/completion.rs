use std::io::stdout;

use anyhow::anyhow;
use clap::CommandFactory;
use clap_complete::{Shell, generate};

use crate::{Cli, Result};

#[derive(clap::Args)]
pub struct Args {
    #[arg(
        long,
        value_enum,
        help = "Shell to generate completions for. If omitted, $SHELL is used to auto-detect."
    )]
    shell: Option<Shell>,
}

pub fn completion(args: &Args) -> Result<()> {
    let shell = match args.shell {
        Some(shell) => shell,
        None => Shell::from_env().ok_or_else(|| {
            anyhow!(
                "Could not detect your shell from $SHELL. \
                 Pass --shell explicitly (supported: bash, zsh, fish, elvish, powershell)."
            )
        })?,
    };

    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut stdout());

    Ok(())
}
