use crate::{commands, local_config::LocalState, CliError};

#[derive(clap::Args)]
pub struct Args {}

pub async fn reset(_args: &Args) -> Result<(), CliError> {
    let _ = LocalState::load()?;

    commands::stop(&commands::StopArgs {}, false)?;
    commands::start(&commands::StartArgs { no_tunnel: false }, false, &None).await?;

    Ok(())
}
