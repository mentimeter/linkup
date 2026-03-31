use crate::{Result, commands, state::State};

#[derive(clap::Args)]
pub struct Args {}

pub async fn reset(_args: &Args) -> Result<()> {
    let _ = State::load()?;

    commands::stop(&commands::StopArgs {}, false)?;
    commands::start(&commands::StartArgs {}, false, &None).await?;

    Ok(())
}
