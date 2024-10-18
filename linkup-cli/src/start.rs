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
    env_logger::init();
    let is_paid = use_paid_tunnels();
    let state = load_and_save_state(config_arg, no_tunnel, is_paid)?;
    set_linkup_env(state.clone())?;
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

fn start_paid_tunnel(
    sys: &dyn System,
    paid_manager: &dyn PaidTunnelManager,
    boot: &dyn BackgroundServices,
    tunnel_manager: &dyn TunnelManager,
    mut state: LocalState,
) -> Result<(), CliError> {
    state = boot.boot_background_services(state.clone())?;

    log::info!(
        "Starting paid tunnel with session name: {}",
        state.linkup.session_name
    );
    let tunnel_name = format!("tunnel-{}", state.linkup.session_name);
    let mut tunnel_id = match paid_manager.get_tunnel_id(&tunnel_name) {
        Ok(Some(id)) => id,
        Ok(None) => "".to_string(),
        Err(e) => return Err(e),
    };

    let mut create_tunnel = false;

    if tunnel_id.is_empty() {
        log::info!("Tunnel ID is empty");
        create_tunnel = true;
    } else {
        log::info!("Tunnel ID: {}", tunnel_id);
        let file_path = format!("{}/.cloudflared/{}.json", sys.get_env("HOME")?, tunnel_id);
        if sys.file_exists(Path::new(&file_path)) {
            log::info!("Tunnel config file for {}: {}", tunnel_id, file_path);
        } else {
            log::info!("Tunnel config file for {} does not exist", tunnel_id);
            create_tunnel = true;
        }
    }

    if create_tunnel {
        println!("Creating tunnel...");
        tunnel_id = paid_manager.create_tunnel(&tunnel_name)?;
        paid_manager.create_dns_record(&tunnel_id, &tunnel_name)?;
    }

    if tunnel_manager.is_tunnel_running().is_err() {
        println!("Starting paid tunnel...");
        state.linkup.tunnel = Some(tunnel_manager.run_tunnel(&state)?);
    } else {
        println!("Cloudflare tunnel was already running.. Try stopping linkup first if you have problems.");
    }
    state.save()?;

    if sys.file_exists(&linkup_file_path(LINKUP_LOCALDNS_INSTALL)) {
        boot.boot_local_dns(state.domain_strings(), state.linkup.session_name.clone())?;
    }

    Ok(())
}

fn start_free_tunnel(state: LocalState, no_tunnel: bool) -> Result<(), CliError> {
    println!("Starting free tunnel");

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
        if server_status(service.local.to_string(), None) == ServerStatus::Ok {
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
        system::MockSystem, CheckErr,
    };

    use url::Url;

    use super::*;

    mock! {
        pub LocalState {
            pub fn save(&self) -> Result<(), CliError>;
        }
    }

    fn make_state(session_name: &str) -> LocalState {
        return LocalState {
            linkup: {
                LinkupState {
                    session_name: session_name.to_string(),
                    session_token: "test_token".to_string(),
                    config_path: "/tmp/home/.linkup/config".to_string(),
                    remote: Url::parse("http://localhost:9066").unwrap(),
                    is_paid: Some(true),
                    tunnel: Some(Url::parse("http://localhost:9066").unwrap()),
                    cache_routes: None,
                }
            },
            services: vec![],
            domains: vec![],
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
            .returning(|_| Ok(make_state("test_session")));

        // Check if tunnel exists -> Yes
        mock_paid_manager
            .expect_get_tunnel_id()
            .with(predicate::eq("tunnel-test_session"))
            .returning(|_| Ok(Some("test_tunnel_id".to_string())));

        // Mock HOME env var
        mock_sys
            .expect_get_env()
            .with(predicate::eq("HOME"))
            .returning(|_| Ok("/tmp/home".to_string()));

        // Check if config file exists -> Yes
        mock_sys
            .expect_file_exists()
            .with(predicate::eq(Path::new(
                "/tmp/home/.cloudflared/test_tunnel_id.json",
            )))
            .returning(|_| true);

        mock_tunnel_manager
            .expect_is_tunnel_running()
            .once()
            .returning(|| Err(CheckErr::TunnelNotRunning));

        // Run tunnel
        mock_tunnel_manager
            .expect_run_tunnel()
            .once()
            .returning(|_| Ok(Url::parse("http://localhost:9066").unwrap()));

        mock_sys
            .expect_file_exists()
            .with(predicate::eq(linkup_file_path(LINKUP_LOCALDNS_INSTALL)))
            .returning(|_| true);

        mock_boot_bg_services
            .expect_boot_local_dns()
            .once()
            .returning(|_, _| Ok(()));

        // Don't create tunnel or DNS record
        mock_paid_manager.expect_create_tunnel().never();
        mock_paid_manager.expect_create_dns_record().never();

        let result = start_paid_tunnel(
            &mock_sys,
            &mock_paid_manager,
            &mock_boot_bg_services,
            &mock_tunnel_manager,
            make_state("test_session"),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_start_paid_tunnel_no_tunnel_exists() {
        let mut mock_boot_bg_services = MockBackgroundServices::new();
        let mut mock_paid_manager = MockPaidTunnelManager::new();
        let mut mock_sys = MockSystem::new();
        let mut mock_tunnel_manager = MockTunnelManager::new();

        // Start background services
        mock_boot_bg_services
            .expect_boot_background_services()
            .once()
            .returning(|_| Ok(make_state("test_session")));

        // Check if tunnel exists -> No
        mock_paid_manager
            .expect_get_tunnel_id()
            .returning(|_| Ok(None));

        // Don't read config file
        mock_sys
            .expect_file_exists()
            .with(predicate::eq(Path::new("/tmp/home/.cloudflared/.json")))
            .never();

        // Create tunnel
        mock_paid_manager
            .expect_create_tunnel()
            .once()
            .with(predicate::eq("tunnel-test_session"))
            .returning(|_| Ok("tunnel-id".to_string()));

        // Create DNS record
        mock_paid_manager
            .expect_create_dns_record()
            .once()
            .with(
                predicate::eq("tunnel-id"),
                predicate::eq("tunnel-test_session"),
            )
            .returning(|_, _| Ok(()));

        mock_tunnel_manager
            .expect_is_tunnel_running()
            .once()
            .returning(|| Err(CheckErr::TunnelNotRunning));

        // Run tunnel
        mock_tunnel_manager
            .expect_run_tunnel()
            .once()
            .with(predicate::eq(make_state("test_session")))
            .returning(|_| Ok(Url::parse("http://localhost:9066").unwrap()));

        mock_sys
            .expect_file_exists()
            .with(predicate::eq(linkup_file_path(LINKUP_LOCALDNS_INSTALL)))
            .returning(|_| true);

        mock_boot_bg_services
            .expect_boot_local_dns()
            .once()
            .returning(|_, _| Ok(()));

        let result = start_paid_tunnel(
            &mock_sys,
            &mock_paid_manager,
            &mock_boot_bg_services,
            &mock_tunnel_manager,
            make_state("test_session"),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_start_paid_tunnel_tunnel_exists_but_not_config() {
        let mut mock_boot_bg_services = MockBackgroundServices::new();
        let mut mock_paid_manager = MockPaidTunnelManager::new();
        let mut mock_sys = MockSystem::new();
        let mut mock_tunnel_manager = MockTunnelManager::new();

        // Start background services
        mock_boot_bg_services
            .expect_boot_background_services()
            .once()
            .returning(|_| Ok(make_state("test_session")));

        // Check if tunnel exists -> Yes
        mock_paid_manager
            .expect_get_tunnel_id()
            .with(predicate::eq("tunnel-test_session"))
            .returning(|_| Ok(Some("tunnel_id".to_string())));

        // Mock HOME env var
        mock_sys
            .expect_get_env()
            .with(predicate::eq("HOME"))
            .returning(|_| Ok("/tmp/home".to_string()));

        // Check if config file exists -> No
        mock_sys
            .expect_file_exists()
            .with(predicate::eq(Path::new(
                "/tmp/home/.cloudflared/tunnel_id.json",
            )))
            .returning(|_| false);

        // Tunnel without config is no good, so create a new one
        mock_paid_manager
            .expect_create_tunnel()
            .once()
            .with(predicate::eq("tunnel-test_session"))
            .returning(|_| Ok("tunnel_id".to_string()));

        // Create DNS record
        mock_paid_manager
            .expect_create_dns_record()
            .once()
            .with(
                predicate::eq("tunnel_id"),
                predicate::eq("tunnel-test_session"),
            )
            .returning(|_, _| Ok(()));

        mock_tunnel_manager
            .expect_is_tunnel_running()
            .once()
            .returning(|| Err(CheckErr::TunnelNotRunning));

        // Run tunnel
        mock_tunnel_manager
            .expect_run_tunnel()
            .once()
            .with(predicate::eq(make_state("test_session")))
            .returning(|_| Ok(Url::parse("http://localhost:9066").unwrap()));

        mock_sys
            .expect_file_exists()
            .with(predicate::eq(linkup_file_path(LINKUP_LOCALDNS_INSTALL)))
            .returning(|_| true);

        mock_boot_bg_services
            .expect_boot_local_dns()
            .once()
            .returning(|_, _| Ok(()));

        let result = start_paid_tunnel(
            &mock_sys,
            &mock_paid_manager,
            &mock_boot_bg_services,
            &mock_tunnel_manager,
            make_state("test_session"),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_start_paid_tunnel_cannot_get_tunnel_id() {
        let mut mock_boot_bg_services = MockBackgroundServices::new();
        let mut mock_paid_manager = MockPaidTunnelManager::new();
        let mock_sys = MockSystem::new();
        let mock_tunnel_manager = MockTunnelManager::new();

        // Start background services
        mock_boot_bg_services
            .expect_boot_background_services()
            .once()
            .returning(|_| Ok(make_state("test_session")));

        // Check if tunnel exists -> Error
        mock_paid_manager
            .expect_get_tunnel_id()
            .with(predicate::eq("tunnel-test_session"))
            .returning(|_| Err(CliError::StatusErr("test error".to_string())));

        let result = start_paid_tunnel(
            &mock_sys,
            &mock_paid_manager,
            &mock_boot_bg_services,
            &mock_tunnel_manager,
            make_state("test_session"),
        );
        assert!(result.is_err());
    }
}
