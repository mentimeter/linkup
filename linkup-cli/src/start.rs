use std::{
    collections::HashMap,
    fs,
    io::stdout,
    path::{Path, PathBuf},
    sync,
    thread::{self, sleep, JoinHandle},
    time::Duration,
};

use colored::Colorize;
use crossterm::{cursor, ExecutableCommand};

use crate::{
    env_files::write_to_env_file,
    local_config::{config_path, config_to_state, get_config},
    services::{self, BackgroundService},
};
use crate::{local_config::LocalState, CliError};

const LOADING_CHARS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// # Arguments
/// * `config_arg`  - Path to the Linkup config to be used as base in case `fresh_state` argument is `true`.
/// * `no_tunnel`   - If there should not be a Cloudflare tunnel.
/// * `fresh_state` - Boolean representing if should refresh the state to what is defined on `config_arg`.
pub async fn start(
    config_arg: &Option<String>,
    no_tunnel: bool,
    fresh_state: bool,
) -> Result<(), CliError> {
    env_logger::init();

    let mut state = if fresh_state {
        let is_paid = services::CloudflareTunnel::use_paid_tunnels();
        let state = load_and_save_state(config_arg, no_tunnel, is_paid)?;
        set_linkup_env(state.clone())?;

        state
    } else {
        LocalState::load()?
    };

    let status_update_channel = sync::mpsc::channel::<services::RunUpdate>();

    let local_server = services::LocalServer::new();
    let cloudflare_tunnel = services::CloudflareTunnel::new(state.linkup.session_name.clone());
    let caddy = services::Caddy::new(state.domain_strings());
    let dnsmasq = services::Dnsmasq::new(state.linkup.session_name.clone(), state.domain_strings());

    let mut display_thread: Option<JoinHandle<()>> = None;
    let display_channel = sync::mpsc::channel::<bool>();

    // If we are doing RUST_LOG=debug to debug if there is anything wrong, having the display thread make so it
    // overwrites some of the output since it does some cursor moving.
    // So in that case, we do not start the display thread.
    if !log::log_enabled!(log::Level::Debug) {
        display_thread = Some(spawn_display_thread(
            &[
                services::LocalServer::NAME,
                services::CloudflareTunnel::NAME,
                services::Caddy::NAME,
                services::Dnsmasq::NAME,
            ],
            status_update_channel.1,
            display_channel.1,
        ));
    }

    // To make sure that we get the last update to the display thread before the error is bubbled up,
    // we store any error that might happen on one of the steps and only return it after we have
    // send the message to the display thread to stop and we join it.
    let mut exit_error: Option<Box<dyn std::error::Error>> = None;

    match local_server
        .run_with_progress(&mut state, status_update_channel.0.clone())
        .await
    {
        Ok(_) => (),
        Err(err) => exit_error = Some(Box::new(err)),
    }

    if exit_error.is_none() {
        match cloudflare_tunnel
            .run_with_progress(&mut state, status_update_channel.0.clone())
            .await
        {
            Ok(_) => (),
            Err(err) => exit_error = Some(Box::new(err)),
        }
    }

    if exit_error.is_none() {
        match caddy
            .run_with_progress(&mut state, status_update_channel.0.clone())
            .await
        {
            Ok(_) => (),
            Err(err) => exit_error = Some(Box::new(err)),
        }
    }

    if exit_error.is_none() {
        match dnsmasq
            .run_with_progress(&mut state, status_update_channel.0.clone())
            .await
        {
            Ok(_) => (),
            Err(err) => exit_error = Some(Box::new(err)),
        }
    }

    if let Some(display_thread) = display_thread {
        display_channel.0.send(true).unwrap();
        display_thread.join().unwrap();
    }

    match exit_error {
        Some(exit_error) => Err(CliError::StatusErr(exit_error.to_string())),
        None => Ok(()),
    }
}

/// This spawns a background thread that is responsible for updating the terminal with the information
/// about the start of the services.
///
/// # Arguments
/// * `names` - These are the names of the services that are going to be displayed here. These is also
///   the "keys" that the status receiver will listen to for updating.
///
/// * `status_update_receiver` - This is a [`sync::mpsc::Receiver`] on which this thread will listen
///   for updates.
///
/// * `exit_signal_receiver` - This is also a [`sync::mpsc::Receiver`], where, to make sure that we always
///   show the last update, the exit of the display thread is done by receiving any message on this receiver.
fn spawn_display_thread(
    names: &[&str],
    status_update_receiver: sync::mpsc::Receiver<services::RunUpdate>,
    exit_signal_receiver: sync::mpsc::Receiver<bool>,
) -> thread::JoinHandle<()> {
    let rows = names.len();

    println!("Background services:");
    println!("{:<20} {:<10}", "NAME".bold(), "STATUS".bold());

    let names: Vec<String> = names.iter().map(|name| String::from(*name)).collect();
    thread::spawn(move || {
        std::io::stdout().execute(cursor::Hide).unwrap();
        let mut loop_iter = 0;
        let mut statuses = HashMap::<String, services::RunUpdate>::new();

        loop {
            if loop_iter == 0 {
                // For the first loop, make sure we add all the services with a pending status.
                for name in &names {
                    statuses.insert(
                        name.clone(),
                        services::RunUpdate {
                            id: name.clone(),
                            status: services::RunStatus::Pending,
                            details: None,
                        },
                    );
                }
            } else {
                crossterm::execute!(std::io::stdout(), cursor::MoveUp(rows as u16)).unwrap();
            }

            for name in &names {
                let latest_update = statuses.get(name).unwrap();
                let mut formatted_status = match &latest_update.status {
                    services::RunStatus::Starting => {
                        LOADING_CHARS[loop_iter % LOADING_CHARS.len()].to_string()
                    }
                    status => status.to_string(),
                };

                if let Some(details) = &latest_update.details {
                    formatted_status.push_str(&format!(" ({})", details));
                }

                let colored_status = match latest_update.status {
                    services::RunStatus::Started => formatted_status.blue(),
                    services::RunStatus::Error => formatted_status.yellow(),
                    _ => formatted_status.normal(),
                };

                // This is necessary in case the previous update was a longer line
                // than the one that is going to be shown now.
                stdout()
                    .execute(crossterm::terminal::Clear(
                        crossterm::terminal::ClearType::CurrentLine,
                    ))
                    .unwrap();

                println!("{:<20} {:<10}", name, colored_status)
            }

            match &status_update_receiver.try_recv() {
                Ok(status_update) => {
                    statuses.insert(status_update.id.clone(), status_update.clone());
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // To make sure we exit on the right order, only check for the exit signal in case
                    // we are not receiving more updates on the `status_update_receiver`.
                    match exit_signal_receiver.try_recv() {
                        Ok(_) | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                        _ => (),
                    }
                }
            }

            loop_iter += 1;
            sleep(Duration::from_millis(50));
        }

        std::io::stdout().execute(cursor::Show).unwrap();
    })
}

fn set_linkup_env(state: LocalState) -> Result<(), CliError> {
    // Set env vars to linkup
    for service in &state.services {
        if let Some(d) = &service.directory {
            set_service_env(d.clone(), state.linkup.config_path.clone())?
        }
    }
    Ok(())
}

fn load_and_save_state(
    config_arg: &Option<String>,
    no_tunnel: bool,
    is_paid: bool,
) -> Result<LocalState, CliError> {
    let previous_state = LocalState::load();
    let config_path = config_path(config_arg)?;
    let input_config = get_config(&config_path)?;

    let mut state = config_to_state(input_config.clone(), config_path, no_tunnel, is_paid);

    // Reuse previous session name if possible
    if let Ok(ps) = previous_state {
        state.linkup.session_name = ps.linkup.session_name;
        state.linkup.session_token = ps.linkup.session_token;

        // Maintain tunnel state until it is rewritten
        if !no_tunnel && ps.linkup.tunnel.is_some() {
            state.linkup.tunnel = ps.linkup.tunnel;
        }
    }

    state.save()?;
    Ok(state)
}

fn set_service_env(directory: String, config_path: String) -> Result<(), CliError> {
    let config_dir = Path::new(&config_path).parent().ok_or_else(|| {
        CliError::SetServiceEnv(
            directory.clone(),
            "config_path does not have a parent directory".to_string(),
        )
    })?;

    let service_path = PathBuf::from(config_dir).join(&directory);

    let dev_env_files_result = fs::read_dir(service_path);
    let dev_env_files: Vec<_> = match dev_env_files_result {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_name().to_string_lossy().ends_with(".linkup")
                    && entry.file_name().to_string_lossy().starts_with(".env.")
            })
            .collect(),
        Err(e) => {
            return Err(CliError::SetServiceEnv(
                directory.clone(),
                format!("Failed to read directory: {}", e),
            ))
        }
    };

    if dev_env_files.is_empty() {
        return Err(CliError::NoDevEnv(directory));
    }

    for dev_env_file in dev_env_files {
        let dev_env_path = dev_env_file.path();
        let env_path =
            PathBuf::from(dev_env_path.parent().unwrap()).join(dev_env_path.file_stem().unwrap());

        write_to_env_file(&directory, &dev_env_path, &env_path)?;
    }

    Ok(())
}
