use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{
    background_booting::{BackgroundServices, RealBackgroundServices},
    env_files::write_to_env_file,
    linkup_file_path,
    local_config::{config_path, config_to_state, get_config},
    paid_tunnel::{PaidTunnelManager, RealPaidTunnelManager},
    services::tunnel::{RealTunnelManager, TunnelManager},
    system::{RealSystem, System},
    LINKUP_LOCALDNS_INSTALL,
};
use crate::{
    local_config::LocalState,
    status::{server_status, ServerStatus},
    CliError,
};

pub fn start(config_arg: &Option<String>, no_tunnel: bool) -> Result<(), CliError> {
    let is_paid = use_paid_tunnels();
    let state = load_and_save_state(config_arg, no_tunnel, is_paid)?;
    if is_paid {
        start_paid_tunnel(
            &RealSystem,
            &RealPaidTunnelManager,
            &RealBackgroundServices,
            &RealTunnelManager,
            state,
        )?;
    } else {
        start_free_tunnel(state, no_tunnel)?;
    }
    Ok(())
}

fn use_paid_tunnels() -> bool {
    env::var("LINKUP_CLOUDFLARE_ACCOUNT_ID").is_ok()
        && env::var("LINKUP_CLOUDFLARE_ZONE_ID").is_ok()
        && env::var("LINKUP_CF_API_TOKEN").is_ok()
}

fn start_paid_tunnel(
    sys: &dyn System,
    paid_manager: &dyn PaidTunnelManager,
    boot: &dyn BackgroundServices,
    tunnel_manager: &dyn TunnelManager,
    mut state: LocalState,
) -> Result<(), CliError> {
    state = boot.boot_background_services(state.clone())?;

    println!(
        "Starting paid tunnel with session name: {}",
        state.linkup.session_name
    );
    let tunnel_name = state.linkup.session_name.to_string();
    let tunnel_id = match paid_manager.get_tunnel_id(&tunnel_name) {
        Ok(Some(id)) => id,
        Ok(None) => "".to_string(),
        Err(e) => return Err(e),
    };

    // If there exists a /$ENV_HOME/.cloudflared/<Tunnel-UUID>.json file, skip creating a tunnel
    if tunnel_id.is_empty() {
        println!("Tunnel ID is empty");
    } else {
        println!("Tunnel ID: {}", tunnel_id);
        let file_path = format!("{}/.cloudflared/{}.json", sys.get_env("HOME")?, tunnel_id);
        if sys.file_exists(Path::new(&file_path)) {
            println!("File exists: {}", file_path);
            let tunnel = tunnel_manager.run_tunnel(&state)?;
            state.linkup.tunnel = Some(tunnel);
            state.save()?;
            return Ok(());
        }
    }

    println!("Creating tunnel");
    let tunnel_id = paid_manager.create_tunnel(&tunnel_name)?;
    paid_manager.create_dns_record(&tunnel_id, &tunnel_name)?;

    tunnel_manager.run_tunnel(&state)?;

    Ok(())
}

fn start_free_tunnel(state: LocalState, no_tunnel: bool) -> Result<(), CliError> {
    println!("Starting free tunnel");
    // Set env vars to linkup
    for service in &state.services {
        match &service.directory {
            Some(d) => set_service_env(d.clone(), state.linkup.config_path.clone())?,
            None => {}
        }
    }

    if no_tunnel && !linkup_file_path(LINKUP_LOCALDNS_INSTALL).exists() {
        println!("Run `linkup local-dns install` before running without a tunnel");

        return Err(CliError::NoTunnelWithoutLocalDns);
    }

    let background_service = RealBackgroundServices {};
    background_service.boot_background_services(state)?;

    check_local_not_started()?;
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
        //println!("Previous session name: {}", ps.linkup.session_name);
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

fn check_local_not_started() -> Result<(), CliError> {
    let state = LocalState::load()?;
    for service in state.services {
        if service.local == service.remote {
            continue;
        }
        if server_status(service.local.to_string()) == ServerStatus::Ok {
            println!("⚠️  Service {} is already running locally!! You need to restart it for linkup's environment variables to be loaded.", service.name);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mockall::{mock, predicate};

    use crate::{
        background_booting::MockBackgroundServices, local_config::LinkupState,
        paid_tunnel::MockPaidTunnelManager, services::tunnel::MockTunnelManager,
        system::MockSystem,
    };

    use url::Url;

    use super::*;

    mock! {
        pub LocalState {
            pub fn save(&self) -> Result<(), CliError>;
        }
    }

    fn make_state() -> LocalState {
        return LocalState {
            linkup: {
                LinkupState {
                    session_name: "test_session".to_string(),
                    session_token: "test_token".to_string(),
                    config_path: "/tmp/home/.linkup/config".to_string(),
                    remote: Url::parse("http://localhost:9066").unwrap(),
                    tunnel: None,
                    cache_routes: None,
                }
            },
            services: vec![],
            domains: vec![],
            is_paid: true,
        };
    }

    #[test]
    fn test_start_paid_tunnel_tunnel_exists() {
        let mut mock_boot_bg_services = MockBackgroundServices::new();
        let mut mock_paid_manager = MockPaidTunnelManager::new();
        let mut mock_sys = MockSystem::new();
        let mut mock_tunnel_manager = MockTunnelManager::new();

        // Start background services
        mock_boot_bg_services
            .expect_boot_background_services()
            .once()
            .returning(|_| Ok(make_state()));

        // Check if tunnel exists
        mock_paid_manager
            .expect_get_tunnel_id()
            .with(predicate::eq("test_session"))
            .returning(|_| Ok(Some("test_tunnel_id".to_string())));

        // Mock HOME env var
        mock_sys
            .expect_get_env()
            .with(predicate::eq("HOME"))
            .returning(|_| Ok("/tmp/home".to_string()));

        // If tunnel exists, check if config file exists
        mock_sys
            .expect_file_exists()
            .with(predicate::eq(Path::new(
                "/tmp/home/.cloudflared/test_tunnel_id.json",
            )))
            .returning(|_| true);

        // If tunnel exists and config file exists, run tunnel
        mock_tunnel_manager
            .expect_run_tunnel()
            .once()
            .returning(|_| Ok(Url::parse("http://localhost:9066").unwrap()));

        // If tunnel exists and config file exists, don't create tunnel or DNS record
        mock_paid_manager.expect_create_tunnel().never();
        mock_paid_manager.expect_create_dns_record().never();

        let _result = start_paid_tunnel(
            &mock_sys,
            &mock_paid_manager,
            &mock_boot_bg_services,
            &mock_tunnel_manager,
            make_state(),
        );
    }

    #[test]
    fn test_start_paid_tunnel_no_tunnel_exists() {
        let mut mock_boot_bg_services = MockBackgroundServices::new();
        let mut mock_manager = MockPaidTunnelManager::new();
        let mut mock_sys = MockSystem::new();
        let mut mock_tunnel_manager = MockTunnelManager::new();

        mock_boot_bg_services
            .expect_boot_background_services()
            .times(1)
            .returning(|_| Ok(make_state()));
        mock_manager.expect_get_tunnel_id().returning(|_| Ok(None));
        mock_sys.expect_file_exists().never();
        mock_manager
            .expect_create_tunnel()
            .times(1)
            .returning(|_| Ok("tunnel-id".to_string()));
        mock_manager
            .expect_create_dns_record()
            .times(1)
            .returning(|_, _| Ok(()));
        mock_tunnel_manager
            .expect_run_tunnel()
            .times(1)
            .returning(|_| Ok(Url::parse("http://localhost:9066").unwrap()));
        let _result = start_paid_tunnel(
            &mock_sys,
            &mock_manager,
            &mock_boot_bg_services,
            &mock_tunnel_manager,
            make_state(),
        );
    }
}
