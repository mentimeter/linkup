use crate::{commands, state::State, Result};

#[derive(clap::Args)]
pub struct Args {}

pub async fn reset(_args: &Args) -> Result<()> {
    let _ = State::load()?;

    commands::stop(&commands::StopArgs {}, false)?;
    commands::start(&commands::StartArgs { no_tunnel: false }, false, &None).await?;

    Ok(())
}
