use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};

use crate::{Result, state::State};
use crate::{
    commands::status::{SessionStatus, format_state_domains},
    env_files::write_to_env_file,
    services,
    state::{config_path, config_to_state, get_config},
};

#[derive(clap::Args)]
pub struct Args {}

pub async fn start(_args: &Args, config_arg: &Option<String>) -> Result<()> {
    let mut state = load_and_save_state(config_arg)?;
    set_linkup_env(state.clone())?;

    services::local_server::start().await?;

    let tunnel_data = match services::local_server::update_state(&mut state).await {
        Ok(tunnel_data) => {
            log::info!("Finished setting up!");

            tunnel_data
        }
        Err(e) => {
            log::error!("Failed to upload state: {e}");

            return Err(e);
        }
    };

    if state.should_use_tunnel() {
        let tunnel_url = services::cloudflared::start(&tunnel_data).await?;

        if let Err(e) = services::cloudflared::update_state(&mut state, &tunnel_url) {
            log::error!("Failed to update state with tunnel information.");

            return Err(e);
        }
    } else {
        log::info!("Skipping. State file requested no tunnel.");
    }

    println!();

    let status = SessionStatus {
        name: state.linkup.session_name.clone(),
        domains: format_state_domains(&state.linkup.session_name, &state.domains),
    };

    status.print();

    Ok(())
}

fn set_linkup_env(state: State) -> Result<()> {
    // Set env vars to linkup
    for service in &state.services {
        if let Some(d) = &service.config.directory {
            set_service_env(d.clone(), state.linkup.config_path.clone())?
        }
    }
    Ok(())
}

fn load_and_save_state(config_arg: &Option<String>) -> Result<State> {
    let config_path = config_path(config_arg)?;
    let input_config = get_config(&config_path)?;

    let mut state = config_to_state(input_config.clone(), config_path);

    if let Ok(previous_state) = State::load() {
        state.linkup.session_name = previous_state.linkup.session_name;
        state.linkup.session_token = previous_state.linkup.session_token;
        state.linkup.tunnel = previous_state.linkup.tunnel;
    }

    state.save()?;

    Ok(state)
}

fn set_service_env(directory: String, config_path: String) -> Result<()> {
    let config_dir = Path::new(&config_path)
        .parent()
        .with_context(|| format!("config_path '{directory}' does not have a parent directory"))?;

    let service_path = PathBuf::from(config_dir).join(&directory);

    let dev_env_files: Vec<_> = fs::read_dir(&service_path)
        .with_context(|| format!("Failed to read service directory {:?}", &service_path))?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_string_lossy().ends_with(".linkup")
                && entry.file_name().to_string_lossy().starts_with(".env.")
        })
        .collect();

    if dev_env_files.is_empty() {
        return Err(anyhow!("No dev env files found on {:?}", directory));
    }

    for dev_env_file in dev_env_files {
        let dev_env_path = dev_env_file.path();
        let env_path =
            PathBuf::from(dev_env_path.parent().unwrap()).join(dev_env_path.file_stem().unwrap());

        write_to_env_file(&directory, &dev_env_path, &env_path)?;
    }

    Ok(())
}
