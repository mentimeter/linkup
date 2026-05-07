use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::LINKUP_CONFIG_ENV;

pub fn load_config_with_override(
    override_env_config: Option<&Path>,
) -> Result<(linkup::config::Config, PathBuf)> {
    let resolved_config_path = match override_env_config {
        Some(path) => fs::canonicalize(path)
            .with_context(|| format!("Unable to resolve absolute path for {path:?}"))?,
        None => {
            let path = env::var(LINKUP_CONFIG_ENV).context(
                "No config argument provided and LINKUP_CONFIG environment variable not set",
            )?;

            fs::canonicalize(&path)
                .with_context(|| format!("Unable to resolve absolute path for {path:?}"))?
        }
    };

    let config = load_config(&resolved_config_path)?;

    Ok((config, resolved_config_path))
}

pub fn load_config(path: &Path) -> Result<linkup::config::Config> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read config file {path:?}"))?;

    serde_yaml::from_str(&content)
        .with_context(|| "Failed to deserialize config file {config_path:?}")
}
