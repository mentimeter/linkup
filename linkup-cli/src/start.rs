use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{
    background_booting::boot_background_services,
    env_files::write_to_env_file,
    file_system::{FileSystem, RealFileSystem},
    linkup_file_path,
    local_config::{config_path, config_to_state, get_config},
    tunnel::{RealTunnelManager, TunnelManager},
    LINKUP_LOCALDNS_INSTALL,
};
use crate::{
    local_config::LocalState,
    status::{server_status, ServerStatus},
    CliError,
};

pub fn start(config_arg: &Option<String>, no_tunnel: bool) -> Result<(), CliError> {
    if env::var("PAID_TUNNELS").is_ok() {
        start_paid_tunnel(&RealTunnelManager, &RealFileSystem, "happy-lion")?;
    } else {
        start_free_tunnel(config_arg, no_tunnel)?;
    }
    Ok(())
}

fn start_paid_tunnel(
    manager: &dyn TunnelManager,
    fs: &dyn FileSystem,
    session_name: &str,
) -> Result<(), CliError> {
    println!("Starting paid tunnel with session name: {}", session_name);
    let tunnel_name = session_name.to_string();
    let tunnel_id = match manager.get_tunnel_id(&tunnel_name) {
        Ok(Some(id)) => id,
        Ok(None) => "".to_string(),
        Err(e) => return Err(e),
    };

    // If there exists a /$ENV_HOME/.cloudflared/<Tunnel-UUID>.json file, skip creating a tunnel
    if tunnel_id.is_empty() {
        println!("Tunnel ID is empty");
    } else {
        println!("Tunnel ID: {}", tunnel_id);
        let file_path = format!(
            "{}/.cloudflared/{}.json",
            env::var("HOME").expect("HOME is not set"),
            tunnel_id
        );
        if fs.file_exists(&file_path) {
            println!("File exists: {}", file_path);
            return Ok(());
        }
    }

    println!("Creating tunnel");
    let tunnel_id = manager.create_tunnel(&tunnel_name)?;
    manager.create_dns_record(&tunnel_id, &tunnel_name)?;
    Ok(())
}

fn start_free_tunnel(config_arg: &Option<String>, no_tunnel: bool) -> Result<(), CliError> {
    println!("Starting free tunnel");
    let previous_state = LocalState::load();
    let config_path = config_path(config_arg)?;
    let input_config = get_config(&config_path)?;

    let mut state = config_to_state(input_config.clone(), config_path, no_tunnel);

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

    boot_background_services()?;

    check_local_not_started()?;
    Ok(())
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
    use crate::{file_system::MockFileSystem, tunnel::MockTunnelManager};

    use super::*;

    #[test]
    fn test_start_paid_tunnel_tunnel_exists() {
        let mut mock_manager = MockTunnelManager::new();
        let mut mock_fs = MockFileSystem::new();
        mock_manager
            .expect_get_tunnel_id()
            .returning(|_| Ok(Some("test_tunnel_id".to_string())));
        mock_fs.expect_file_exists().returning(|_| true);
        mock_manager.expect_create_tunnel().never();
        mock_manager.expect_create_dns_record().never();
        let _result = start_paid_tunnel(&mock_manager, &mock_fs, "test_session");
    }

    #[test]
    fn test_start_paid_tunnel_no_tunnel_exists() {
        let mut mock_manager = MockTunnelManager::new();
        let mut mock_fs = MockFileSystem::new();
        mock_manager.expect_get_tunnel_id().returning(|_| Ok(None));
        mock_fs.expect_file_exists().never();
        mock_manager
            .expect_create_tunnel()
            .times(1)
            .returning(|_| Ok("tunnel-id".to_string()));
        mock_manager
            .expect_create_dns_record()
            .times(1)
            .returning(|_, _| Ok(()));
        let _result = start_paid_tunnel(&mock_manager, &mock_fs, "test_session");
    }
}
