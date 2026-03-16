use std::{fs, path::Path};

use anyhow::anyhow;

use crate::{
    default_linkup_dir_path,
    services::{self, service_id_for_home, stop_service, BackgroundService},
    state::State,
    worker_client::WorkerClient,
    Result,
};

use super::instance_use::ACTIVE_INSTANCE_FILE;

#[derive(clap::Args)]
pub struct Args {
    /// Instance number to remove (e.g., 1, 2, 3)
    instance: u32,
}

pub async fn instance_remove(args: &Args) -> Result<()> {
    let instance_dir = default_linkup_dir_path()
        .join("instances")
        .join(args.instance.to_string());

    if !instance_dir.exists() {
        return Err(anyhow!("Instance {} does not exist", args.instance));
    }

    try_delete_tunnel(&instance_dir).await;

    let home = instance_dir.to_string_lossy().to_string();
    stop_service(&service_id_for_home(services::LocalServer::ID, &home));
    stop_service(&service_id_for_home(services::CloudflareTunnel::ID, &home));
    stop_service(&service_id_for_home(services::LocalDnsServer::ID, &home));

    fs::remove_dir_all(&instance_dir)?;

    clear_active_instance_if_matches(&instance_dir, args.instance);

    println!("Removed instance {}", args.instance);

    Ok(())
}

/// Best-effort tunnel cleanup: loads state from the instance directory to find
/// the session name and worker URL, then asks the worker to delete the tunnel.
/// Logs a warning and returns normally on any failure so local cleanup can proceed.
pub(super) async fn try_delete_tunnel(instance_dir: &Path) {
    let state = match State::load_from_dir(instance_dir) {
        Ok(s) => s,
        Err(_) => return,
    };

    let session_name = &state.linkup.session_name;
    if session_name.is_empty() {
        return;
    }

    let client = WorkerClient::new(&state.linkup.worker_url, &state.linkup.worker_token);
    if let Err(e) = client.delete_tunnel(session_name).await {
        log::debug!(
            "Failed to delete tunnel for session '{}': {}",
            session_name,
            e
        );
    }
}

fn clear_active_instance_if_matches(removed_dir: &std::path::Path, instance_id: u32) {
    let active_path = default_linkup_dir_path().join(ACTIVE_INSTANCE_FILE);
    if let Ok(content) = fs::read_to_string(&active_path) {
        let active_dir = std::path::PathBuf::from(content.trim());
        if active_dir == removed_dir {
            let _ = fs::remove_file(&active_path);
            println!(
                "Instance {} was the active instance; switched back to default.",
                instance_id
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<F: FnOnce(std::path::PathBuf)>(f: F) {
        let _lock = crate::ENV_TEST_MUTEX.lock().unwrap();
        let prev_home = std::env::var("HOME").ok();

        let tmp = std::env::temp_dir().join(format!("linkup-test-remove-{}", std::process::id()));
        let linkup_dir = tmp.join(".linkup");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&linkup_dir).unwrap();
        unsafe { std::env::set_var("HOME", &tmp) };

        f(linkup_dir);

        let _ = fs::remove_dir_all(&tmp);
        if let Some(val) = prev_home {
            unsafe { std::env::set_var("HOME", val) };
        }
    }

    #[test]
    fn test_instance_remove_nonexistent() {
        with_temp_home(|_| {
            let args = Args { instance: 99 };
            let result = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(instance_remove(&args));
            assert!(result.is_err());
            assert!(
                result.unwrap_err().to_string().contains("does not exist"),
                "should report instance does not exist"
            );
        });
    }

    #[test]
    fn test_instance_remove_clears_active_instance() {
        with_temp_home(|linkup_dir| {
            let instance_dir = linkup_dir.join("instances").join("1");
            fs::create_dir_all(&instance_dir).unwrap();

            let active_path = linkup_dir.join(ACTIVE_INSTANCE_FILE);
            fs::write(&active_path, instance_dir.to_string_lossy().as_bytes()).unwrap();

            let args = Args { instance: 1 };
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(instance_remove(&args))
                .unwrap();

            assert!(
                !active_path.exists(),
                "active-instance file should be removed"
            );
            assert!(!instance_dir.exists(), "instance dir should be removed");
        });
    }

    #[test]
    fn test_instance_remove_preserves_other_active_instance() {
        with_temp_home(|linkup_dir| {
            let instance_dir = linkup_dir.join("instances").join("1");
            fs::create_dir_all(&instance_dir).unwrap();

            let other_dir = linkup_dir.join("instances").join("2");
            fs::create_dir_all(&other_dir).unwrap();

            let active_path = linkup_dir.join(ACTIVE_INSTANCE_FILE);
            fs::write(&active_path, other_dir.to_string_lossy().as_bytes()).unwrap();

            let args = Args { instance: 1 };
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(instance_remove(&args))
                .unwrap();

            assert!(
                active_path.exists(),
                "active-instance file should be preserved"
            );
            let content = fs::read_to_string(&active_path).unwrap();
            assert_eq!(content.trim(), other_dir.to_string_lossy());
        });
    }

    #[test]
    fn test_try_delete_tunnel_no_state_file() {
        let tmp =
            std::env::temp_dir().join(format!("linkup-test-tunnel-nostate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(try_delete_tunnel(&tmp));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_try_delete_tunnel_empty_session_name() {
        let tmp = std::env::temp_dir().join(format!(
            "linkup-test-tunnel-emptysession-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let state_yaml = r#"
linkup:
  session_name: ""
  session_token: abc123
  worker_url: https://remote-linkup.example.com
  worker_token: test_token_123
  config_path: ./config.yaml
  tunnel: null
  cache_routes: null
domains: []
services: []
"#;
        fs::write(tmp.join("state"), state_yaml).unwrap();

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(try_delete_tunnel(&tmp));

        let _ = fs::remove_dir_all(&tmp);
    }
}
