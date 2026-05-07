use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};
use colored::Colorize;
use linkup::SessionKind;

use crate::{
    Result, commands,
    env_files::write_to_env_file,
    services::{self, local_server},
    session::{SessionRow, print_sessions_table},
    state::{State, find_isolated_suffixes},
};

#[derive(clap::Args)]
pub struct Args {
    #[arg(
        long,
        help = "Start as an isolated session with no Cloudflare connectivity"
    )]
    pub isolated: bool,
}

pub async fn start(args: &Args, config_arg: Option<&Path>) -> Result<()> {
    if let Ok(existing_state) = State::load()
        && local_server::is_reachable().await
    {
        let requested = if args.isolated {
            SessionKind::Isolated
        } else {
            SessionKind::Tunneled
        };

        if existing_state.linkup.kind != requested {
            println!(
                "Linkup is already running as {}. Run 'linkup stop' first to switch modes.",
                existing_state.linkup.kind
            );

            return Ok(());
        }
    }

    let mut state = load_and_save_state(config_arg)?;
    set_linkup_env(state.clone())?;

    if args.isolated && !commands::local_dns::is_installed(Some(&state), config_arg) {
        println!(
            "{}",
            "Isolated sessions requires Local DNS to be configured.\nPlease run 'linkup local-dns install' first."
                .yellow()
        );

        return Ok(());
    }

    services::local_server::start().await?;

    let main_session_kind = if args.isolated {
        state.linkup.kind = SessionKind::Isolated;
        services::local_server::update_isolated_state(&mut state).await?;
        state
            .save()
            .expect("failed to update local state file with session name");

        SessionKind::Isolated
    } else {
        state.linkup.kind = SessionKind::Tunneled;

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

        SessionKind::Tunneled
    };

    let mut rows = vec![SessionRow::from_state(&state, main_session_kind)];

    for suffix in find_isolated_suffixes() {
        match State::load_with_suffix(&suffix) {
            Ok(mut isolated_state) => {
                match services::local_server::update_isolated_state(&mut isolated_state).await {
                    Ok(()) => {
                        isolated_state
                            .save_with_suffix(&isolated_state.linkup.session_name.clone())
                            .unwrap_or_else(|e| {
                                log::warn!(
                                    "Failed to save isolated session state '{}': {}",
                                    suffix,
                                    e
                                )
                            });
                        rows.push(SessionRow::from_state(
                            &isolated_state,
                            SessionKind::Isolated,
                        ));
                    }
                    Err(e) => {
                        log::warn!("Failed to restore isolated session '{}': {}", suffix, e)
                    }
                }
            }
            Err(e) => log::warn!("Failed to load isolated session state '{}': {}", suffix, e),
        }
    }

    println!();
    print_sessions_table(&rows, None);

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

fn load_and_save_state(config_arg: Option<&Path>) -> Result<State> {
    let mut state = State::from_config(config_arg)?;

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
