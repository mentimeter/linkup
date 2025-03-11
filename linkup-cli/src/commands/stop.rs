use std::fs::{self};
use std::path::{Path, PathBuf};

use crate::env_files::clear_env_file;
use crate::local_config::LocalState;
use crate::{is_sudo, services, sudo_su, CliError};

#[derive(clap::Args)]
pub struct Args {}

pub fn stop(_args: &Args, clear_env: bool) -> Result<(), CliError> {
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

    // TODO(augustoccesar)[2025-03-11]: Since we are binding now on 80 and 443 ourselves, we need
    //   to get sudo permission. Ideally this wouldn't be necessary, so we should take a look if/how
    //   we can avoid needing it. Caddy was able to bind on them without sudo (at least on macos),
    //   so there could be a way.
    if !is_sudo() {
        sudo_su()?;
    }

    services::LocalServer::new().stop();
    services::CloudflareTunnel::new().stop();
    #[cfg(target_os = "macos")]
    services::Dnsmasq::new().stop();

    println!("Stopped linkup");

    Ok(())
}

fn remove_service_env(directory: String, config_path: String) -> Result<(), CliError> {
    let config_dir = Path::new(&config_path).parent().ok_or_else(|| {
        CliError::SetServiceEnv(
            directory.clone(),
            "config_path does not have a parent directory".to_string(),
        )
    })?;

    let service_path = PathBuf::from(config_dir).join(&directory);

    let env_files_result = fs::read_dir(service_path);
    let env_files: Vec<_> = match env_files_result {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".env"))
            .collect(),
        Err(e) => {
            return Err(CliError::SetServiceEnv(
                directory.clone(),
                format!("Failed to read directory: {}", e),
            ))
        }
    };

    for env_file in env_files {
        let env_path = env_file.path();

        clear_env_file(&directory, &env_path)?;
    }

    Ok(())
}
