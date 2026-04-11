use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error, anyhow};
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar};

use crate::{Result, state::State};
use crate::{
    commands::status::{SessionStatus, format_state_domains},
    env_files::write_to_env_file,
    services::{self, BackgroundService},
    state::{config_path, config_to_state, get_config},
};

#[derive(clap::Args)]
pub struct Args {}

pub async fn start(_args: &Args, config_arg: &Option<String>) -> Result<()> {
    let mut state = load_and_save_state(config_arg)?;
    set_linkup_env(state.clone())?;

    let local_server = services::LocalServer::new();
    let cloudflare_tunnel = services::CloudflareTunnel::new();

    let multi_progress = MultiProgress::new();

    multi_progress
        .println("Background services:")
        .expect("printing should not fail");
    multi_progress
        .println(format!("{:<20} {:<10}", "NAME".bold(), "STATUS".bold()))
        .expect("printing should not fail");

    let local_server_progress = multi_progress.add(ProgressBar::new_spinner());
    local_server.prepare_progress_bar(&local_server_progress);

    let cloudflare_tunnel_progress = multi_progress.add(ProgressBar::new_spinner());
    cloudflare_tunnel.prepare_progress_bar(&cloudflare_tunnel_progress);

    // To make sure that we get the last update to the display thread before the error is bubbled up,
    // we store any error that might happen on one of the steps and only return it after we have
    // send the message to the display thread to stop and we join it.
    let mut exit_error: Option<Error> = None;

    match local_server
        .run_with_progress(&mut state, &local_server_progress)
        .await
    {
        Ok(_) => (),
        Err(err) => exit_error = Some(err),
    }

    if exit_error.is_none() {
        match cloudflare_tunnel
            .run_with_progress(&mut state, &cloudflare_tunnel_progress)
            .await
        {
            Ok(_) => (),
            Err(err) => exit_error = Some(err),
        }
    }

    local_server_progress.finish();
    cloudflare_tunnel_progress.finish();

    if let Some(exit_error) = exit_error {
        return Err(exit_error).context("Failed to start CLI");
    }

    let status = SessionStatus {
        name: state.linkup.session_name.clone(),
        domains: format_state_domains(&state.linkup.session_name, &state.domains),
    };

    print!("\n\n");
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
