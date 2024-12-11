use crate::{local_config::LocalState, CliError};

pub async fn reset() -> Result<(), CliError> {
    let _ = LocalState::load()?;

    crate::stop(false)?;
    crate::start(&None, false, false).await?;

    Ok(())
}
