use std::{
    env, fs,
    io::stdout,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::{self, sleep},
    time::Duration,
};

use colored::Colorize;
use crossterm::{cursor, ExecutableCommand};

use crate::{
    background::{self, BackgroudServiceStatus, BackgroundService},
    env_files::write_to_env_file,
    local_config::{config_path, config_to_state, get_config},
};
use crate::{local_config::LocalState, CliError};

const LOADING_CHARS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn start(config_arg: &Option<String>, no_tunnel: bool) -> Result<(), CliError> {
    env_logger::init();
    let is_paid = use_paid_tunnels();
    let state = load_and_save_state(config_arg, no_tunnel, is_paid)?;
    set_linkup_env(state.clone())?;

    let state = Arc::new(Mutex::new(state));
    let local_server = background::LocalServer::new(state.clone());
    let cloudflare_tunnel = background::CloudflareTunnel::new(state.clone());
    let caddy = background::Caddy::new(state.clone());
    let dnsmasq = background::Dnsmasq::new(state.clone());

    let services: Vec<&dyn BackgroundService> =
        vec![&local_server, &cloudflare_tunnel, &caddy, &dnsmasq];
    let services_names: Vec<String> = services.iter().map(|s| s.name()).collect();
    let rows = services_names.len();

    let statuses = Arc::new(Mutex::new(vec![
        background::BackgroudServiceStatus::Pending;
        rows
    ]));

    std::io::stdout().execute(cursor::Hide).unwrap();

    let mut printing_thread: Option<thread::JoinHandle<()>> = None;

    // If we are running with debug, we will look at the logs instead of relying on the "pretty printed"
    // status update, so we skip it.
    if !log::log_enabled!(log::Level::Debug) {
        printing_thread = Some(background_printing(services_names, statuses.clone()));
    }

    for i in 0..rows {
        let service = services[i];

        service.setup();

        let mut s = statuses.lock().unwrap();
        s[i] = background::BackgroudServiceStatus::Starting;
        drop(s);

        service.start();
        if !service.ready() {
            let mut s = statuses.lock().unwrap();
            s[i] = background::BackgroudServiceStatus::Timeout;
        }
        service.update_state();

        let mut s = statuses.lock().unwrap();
        s[i] = background::BackgroudServiceStatus::Started;
    }

    if let Some(printing_thread) = printing_thread {
        ctrlc::set_handler(move || {
            stdout().execute(cursor::Show).unwrap();
            std::process::exit(130);
        })
        .expect("Failed to set CTRL+C handler");

        printing_thread.join().unwrap();

        std::io::stdout().execute(cursor::Show).unwrap();
    }

    Ok(())
}

fn background_printing(
    services_names: Vec<String>,
    statuses: Arc<Mutex<Vec<BackgroudServiceStatus>>>,
) -> thread::JoinHandle<()> {
    let rows = services_names.len();

    println!("Background services:");
    println!("{:<20} {:<10}", "NAME", "STATUS");

    thread::spawn(move || {
        let mut loop_iter = 0;
        loop {
            if loop_iter > 0 {
                crossterm::execute!(std::io::stdout(), cursor::MoveUp(rows as u16)).unwrap();
            }

            match statuses.lock() {
                Ok(statuses) => {
                    for i in 0..rows {
                        let formatted_status = match statuses[i] {
                            background::BackgroudServiceStatus::Starting => LOADING_CHARS
                                [loop_iter % LOADING_CHARS.len()]
                            .to_string()
                            .normal(),
                            background::BackgroudServiceStatus::Started => {
                                statuses[i].to_string().blue()
                            }
                            background::BackgroudServiceStatus::Timeout => {
                                statuses[i].to_string().yellow()
                            }
                            _ => statuses[i].to_string().normal(),
                        };

                        println!("{:<20} {:<10}", services_names[i], formatted_status)
                    }
                }
                Err(_) => continue,
            }

            if statuses
                .lock()
                .unwrap()
                .iter()
                .all(|s| *s == background::BackgroudServiceStatus::Started)
            {
                break;
            }

            loop_iter += 1;
            sleep(Duration::from_millis(50));
        }
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

fn use_paid_tunnels() -> bool {
    env::var("LINKUP_CLOUDFLARE_ACCOUNT_ID").is_ok()
        && env::var("LINKUP_CLOUDFLARE_ZONE_ID").is_ok()
        && env::var("LINKUP_CF_API_TOKEN").is_ok()
}

// fn start_paid_tunnel(
//     sys: &dyn System,
//     paid_manager: &dyn PaidTunnelManager,
//     boot: &dyn BackgroundServices,
//     tunnel_manager: &dyn TunnelManager,
//     mut state: LocalState,
// ) -> Result<(), CliError> {
//     state = boot.boot_linkup_server(state.clone())?;

//     log::info!(
//         "Starting paid tunnel with session name: {}",
//         state.linkup.session_name
//     );
//     let tunnel_name = format!("tunnel-{}", state.linkup.session_name);
//     let mut tunnel_id = match paid_manager.get_tunnel_id(&tunnel_name) {
//         Ok(Some(id)) => id,
//         Ok(None) => "".to_string(),
//         Err(e) => return Err(e),
//     };

//     let mut create_tunnel = false;

//     if tunnel_id.is_empty() {
//         log::info!("Tunnel ID is empty");
//         create_tunnel = true;
//     } else {
//         log::info!("Tunnel ID: {}", tunnel_id);
//         let file_path = format!("{}/.cloudflared/{}.json", sys.get_env("HOME")?, tunnel_id);
//         if sys.file_exists(Path::new(&file_path)) {
//             log::info!("Tunnel config file for {}: {}", tunnel_id, file_path);
//         } else {
//             log::info!("Tunnel config file for {} does not exist", tunnel_id);
//             create_tunnel = true;
//         }
//     }

//     if create_tunnel {
//         println!("Creating tunnel...");
//         tunnel_id = paid_manager.create_tunnel(&tunnel_name)?;
//         paid_manager.create_dns_record(&tunnel_id, &tunnel_name)?;
//     }

//     if tunnel_manager.is_tunnel_running().is_err() {
//         println!("Starting paid tunnel...");
//         state.linkup.tunnel = Some(tunnel_manager.run_tunnel(&state)?);
//     } else {
//         println!("Cloudflare tunnel was already running.. Try stopping linkup first if you have problems.");
//     }
//     state.save()?;

//     if sys.file_exists(&linkup_file_path(LINKUP_LOCALDNS_INSTALL)) {
//         boot.boot_local_dns(state.domain_strings(), state.linkup.session_name.clone())?;
//     }

//     print_session_names(&state);
//     check_local_not_started(&state)?;

//     Ok(())
// }

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

// fn check_local_not_started(state: &LocalState) -> Result<(), CliError> {
//     for service in &state.services {
//         if service.local == service.remote {
//             continue;
//         }

//         if server_status(service.local.to_string(), None) == ServerStatus::Ok {
//             let warning = format!(
//                 "⚠️  Service {} is already running locally!! You need to restart it for linkup's environment variables to be loaded.",
//                 service.name
//             ).yellow();

//             println!("{}", warning);
//         }
//     }
//     Ok(())
// }

// #[cfg(test)]
// mod tests {
//     use mockall::{mock, predicate};

//     use crate::{
//         background_booting::MockBackgroundServices, local_config::LinkupState,
//         paid_tunnel::MockPaidTunnelManager, services::tunnel::MockTunnelManager,
//         system::MockSystem, CheckErr,
//     };

//     use url::Url;

//     use super::*;

//     mock! {
//         pub LocalState {
//             pub fn save(&self) -> Result<(), CliError>;
//         }
//     }

//     fn make_state(session_name: &str) -> LocalState {
//         LocalState {
//             linkup: {
//                 LinkupState {
//                     session_name: session_name.to_string(),
//                     session_token: "test_token".to_string(),
//                     config_path: "/tmp/home/.linkup/config".to_string(),
//                     remote: Url::parse("http://localhost:9066").unwrap(),
//                     is_paid: Some(true),
//                     tunnel: Some(Url::parse("http://localhost:9066").unwrap()),
//                     cache_routes: None,
//                 }
//             },
//             services: vec![],
//             domains: vec![],
//         }
//     }

//     #[test]
//     fn test_start_paid_tunnel_tunnel_exists() {
//         let mut mock_boot_bg_services = MockBackgroundServices::new();
//         let mut mock_paid_manager = MockPaidTunnelManager::new();
//         let mut mock_sys = MockSystem::new();
//         let mut mock_tunnel_manager = MockTunnelManager::new();

//         // Start background services
//         mock_boot_bg_services
//             .expect_boot_linkup_server()
//             .once()
//             .returning(|_| Ok(make_state("test_session")));

//         // Check if tunnel exists -> Yes
//         mock_paid_manager
//             .expect_get_tunnel_id()
//             .with(predicate::eq("tunnel-test_session"))
//             .returning(|_| Ok(Some("test_tunnel_id".to_string())));

//         // Mock HOME env var
//         mock_sys
//             .expect_get_env()
//             .with(predicate::eq("HOME"))
//             .returning(|_| Ok("/tmp/home".to_string()));

//         // Check if config file exists -> Yes
//         mock_sys
//             .expect_file_exists()
//             .with(predicate::eq(Path::new(
//                 "/tmp/home/.cloudflared/test_tunnel_id.json",
//             )))
//             .returning(|_| true);

//         mock_tunnel_manager
//             .expect_is_tunnel_running()
//             .once()
//             .returning(|| Err(CheckErr::TunnelNotRunning));

//         // Run tunnel
//         mock_tunnel_manager
//             .expect_run_tunnel()
//             .once()
//             .returning(|_| Ok(Url::parse("http://localhost:9066").unwrap()));

//         mock_sys
//             .expect_file_exists()
//             .with(predicate::eq(linkup_file_path(LINKUP_LOCALDNS_INSTALL)))
//             .returning(|_| true);

//         mock_boot_bg_services
//             .expect_boot_local_dns()
//             .once()
//             .returning(|_, _| Ok(()));

//         // Don't create tunnel or DNS record
//         mock_paid_manager.expect_create_tunnel().never();
//         mock_paid_manager.expect_create_dns_record().never();

//         let result = start_paid_tunnel(
//             &mock_sys,
//             &mock_paid_manager,
//             &mock_boot_bg_services,
//             &mock_tunnel_manager,
//             make_state("test_session"),
//         );
//         assert!(result.is_ok());
//     }

//     #[test]
//     fn test_start_paid_tunnel_no_tunnel_exists() {
//         let mut mock_boot_bg_services = MockBackgroundServices::new();
//         let mut mock_paid_manager = MockPaidTunnelManager::new();
//         let mut mock_sys = MockSystem::new();
//         let mut mock_tunnel_manager = MockTunnelManager::new();

//         // Start background services
//         mock_boot_bg_services
//             .expect_boot_linkup_server()
//             .once()
//             .returning(|_| Ok(make_state("test_session")));

//         // Check if tunnel exists -> No
//         mock_paid_manager
//             .expect_get_tunnel_id()
//             .returning(|_| Ok(None));

//         // Don't read config file
//         mock_sys
//             .expect_file_exists()
//             .with(predicate::eq(Path::new("/tmp/home/.cloudflared/.json")))
//             .never();

//         // Create tunnel
//         mock_paid_manager
//             .expect_create_tunnel()
//             .once()
//             .with(predicate::eq("tunnel-test_session"))
//             .returning(|_| Ok("tunnel-id".to_string()));

//         // Create DNS record
//         mock_paid_manager
//             .expect_create_dns_record()
//             .once()
//             .with(
//                 predicate::eq("tunnel-id"),
//                 predicate::eq("tunnel-test_session"),
//             )
//             .returning(|_, _| Ok(()));

//         mock_tunnel_manager
//             .expect_is_tunnel_running()
//             .once()
//             .returning(|| Err(CheckErr::TunnelNotRunning));

//         // Run tunnel
//         mock_tunnel_manager
//             .expect_run_tunnel()
//             .once()
//             .with(predicate::eq(make_state("test_session")))
//             .returning(|_| Ok(Url::parse("http://localhost:9066").unwrap()));

//         mock_sys
//             .expect_file_exists()
//             .with(predicate::eq(linkup_file_path(LINKUP_LOCALDNS_INSTALL)))
//             .returning(|_| true);

//         mock_boot_bg_services
//             .expect_boot_local_dns()
//             .once()
//             .returning(|_, _| Ok(()));

//         let result = start_paid_tunnel(
//             &mock_sys,
//             &mock_paid_manager,
//             &mock_boot_bg_services,
//             &mock_tunnel_manager,
//             make_state("test_session"),
//         );
//         assert!(result.is_ok());
//     }

//     #[test]
//     fn test_start_paid_tunnel_tunnel_exists_but_not_config() {
//         let mut mock_boot_bg_services = MockBackgroundServices::new();
//         let mut mock_paid_manager = MockPaidTunnelManager::new();
//         let mut mock_sys = MockSystem::new();
//         let mut mock_tunnel_manager = MockTunnelManager::new();

//         // Start background services
//         mock_boot_bg_services
//             .expect_boot_linkup_server()
//             .once()
//             .returning(|_| Ok(make_state("test_session")));

//         // Check if tunnel exists -> Yes
//         mock_paid_manager
//             .expect_get_tunnel_id()
//             .with(predicate::eq("tunnel-test_session"))
//             .returning(|_| Ok(Some("tunnel_id".to_string())));

//         // Mock HOME env var
//         mock_sys
//             .expect_get_env()
//             .with(predicate::eq("HOME"))
//             .returning(|_| Ok("/tmp/home".to_string()));

//         // Check if config file exists -> No
//         mock_sys
//             .expect_file_exists()
//             .with(predicate::eq(Path::new(
//                 "/tmp/home/.cloudflared/tunnel_id.json",
//             )))
//             .returning(|_| false);

//         // Tunnel without config is no good, so create a new one
//         mock_paid_manager
//             .expect_create_tunnel()
//             .once()
//             .with(predicate::eq("tunnel-test_session"))
//             .returning(|_| Ok("tunnel_id".to_string()));

//         // Create DNS record
//         mock_paid_manager
//             .expect_create_dns_record()
//             .once()
//             .with(
//                 predicate::eq("tunnel_id"),
//                 predicate::eq("tunnel-test_session"),
//             )
//             .returning(|_, _| Ok(()));

//         mock_tunnel_manager
//             .expect_is_tunnel_running()
//             .once()
//             .returning(|| Err(CheckErr::TunnelNotRunning));

//         // Run tunnel
//         mock_tunnel_manager
//             .expect_run_tunnel()
//             .once()
//             .with(predicate::eq(make_state("test_session")))
//             .returning(|_| Ok(Url::parse("http://localhost:9066").unwrap()));

//         mock_sys
//             .expect_file_exists()
//             .with(predicate::eq(linkup_file_path(LINKUP_LOCALDNS_INSTALL)))
//             .returning(|_| true);

//         mock_boot_bg_services
//             .expect_boot_local_dns()
//             .once()
//             .returning(|_, _| Ok(()));

//         let result = start_paid_tunnel(
//             &mock_sys,
//             &mock_paid_manager,
//             &mock_boot_bg_services,
//             &mock_tunnel_manager,
//             make_state("test_session"),
//         );
//         assert!(result.is_ok());
//     }

//     #[test]
//     fn test_start_paid_tunnel_cannot_get_tunnel_id() {
//         let mut mock_boot_bg_services = MockBackgroundServices::new();
//         let mut mock_paid_manager = MockPaidTunnelManager::new();
//         let mock_sys = MockSystem::new();
//         let mock_tunnel_manager = MockTunnelManager::new();

//         // Start background services
//         mock_boot_bg_services
//             .expect_boot_linkup_server()
//             .once()
//             .returning(|_| Ok(make_state("test_session")));

//         // Check if tunnel exists -> Error
//         mock_paid_manager
//             .expect_get_tunnel_id()
//             .with(predicate::eq("tunnel-test_session"))
//             .returning(|_| Err(CliError::StatusErr("test error".to_string())));

//         let result = start_paid_tunnel(
//             &mock_sys,
//             &mock_paid_manager,
//             &mock_boot_bg_services,
//             &mock_tunnel_manager,
//             make_state("test_session"),
//         );
//         assert!(result.is_err());
//     }
// }
