use std::fs::{self};
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::env_files::clear_env_file;
use crate::local_config::LocalState;
use crate::{services, Result};

#[derive(clap::Args)]
pub struct Args {}

pub fn stop(_args: &Args, clear_env: bool) -> Result<()> {
    match (LocalState::load(), clear_env) {
        (Ok(state), true) => {
            // Reset env vars back to what they were before
            for service in &state.services {
                let remove_res = match &service.directory {
                    Some(d) => remove_service_env(d.clone(), state.linkup.config_path.clone()),
                    None => Ok(()),
                };

                if let Err(e) = remove_res {
                    println!("Could not remove env for service {}: {}", service.name, e);
                }
            }
        }
        (Ok(_), false) => (),
        (Err(err), _) => {
            log::warn!("Failed to fetch local state: {}", err);
        }
    }

    services::LocalServer::new().stop();
    services::CloudflareTunnel::new().stop();
    services::LocalDnsServer::new().stop();

    println!("Stopped linkup");

    Ok(())
}

fn remove_service_env(directory: String, config_path: String) -> Result<()> {
    let config_dir = Path::new(&config_path)
        .parent()
        .with_context(|| format!("config_path '{directory}' does not have a parent directory"))?;

    let service_path = PathBuf::from(config_dir).join(&directory);

    let env_files: Vec<_> = fs::read_dir(&service_path)
        .with_context(|| format!("Failed to read service directory {:?}", &service_path))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".env"))
        .collect();

    for env_file in env_files {
        let env_path = env_file.path();

        clear_env_file(&directory, &env_path)?;
    }

    Ok(())
}
