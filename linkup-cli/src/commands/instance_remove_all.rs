use std::fs;

use crate::{
    default_linkup_dir_path,
    services::{self, service_id_for_home, stop_service, BackgroundService},
    Result,
};

use super::{instance_remove::try_delete_tunnel, instance_use::ACTIVE_INSTANCE_FILE};

#[derive(clap::Args)]
pub struct Args {}

pub async fn instance_remove_all(_args: &Args) -> Result<()> {
    let instances_dir = default_linkup_dir_path().join("instances");

    if !instances_dir.exists() {
        println!("No instances to remove");
        return Ok(());
    }

    let entries: Vec<_> = fs::read_dir(&instances_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    if entries.is_empty() {
        println!("No instances to remove");
        return Ok(());
    }

    for entry in &entries {
        try_delete_tunnel(&entry.path()).await;

        let home = entry.path().to_string_lossy().to_string();
        stop_service(&service_id_for_home(services::LocalServer::ID, &home));
        stop_service(&service_id_for_home(services::CloudflareTunnel::ID, &home));
        stop_service(&service_id_for_home(services::LocalDnsServer::ID, &home));
    }

    fs::remove_dir_all(&instances_dir)?;

    let active_path = default_linkup_dir_path().join(ACTIVE_INSTANCE_FILE);
    if active_path.exists() {
        let _ = fs::remove_file(&active_path);
        println!("Cleared active instance; switched back to default.");
    }

    let counter_path = default_linkup_dir_path().join("next-instance");
    if counter_path.exists() {
        fs::remove_file(&counter_path)?;
    }

    println!("Removed {} instance(s)", entries.len());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<F: FnOnce(std::path::PathBuf)>(f: F) {
        let _lock = crate::ENV_TEST_MUTEX.lock().unwrap();
        let prev_home = std::env::var("HOME").ok();

        let tmp =
            std::env::temp_dir().join(format!("linkup-test-remove-all-{}", std::process::id()));
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
    fn test_instance_remove_all_no_instances_dir() {
        with_temp_home(|_| {
            let args = Args {};
            let result = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(instance_remove_all(&args));
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_instance_remove_all_empty_instances_dir() {
        with_temp_home(|linkup_dir| {
            fs::create_dir_all(linkup_dir.join("instances")).unwrap();

            let args = Args {};
            let result = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(instance_remove_all(&args));
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_instance_remove_all_clears_active_instance() {
        with_temp_home(|linkup_dir| {
            let instances_dir = linkup_dir.join("instances");
            let instance_1 = instances_dir.join("1");
            fs::create_dir_all(&instance_1).unwrap();

            let active_path = linkup_dir.join(ACTIVE_INSTANCE_FILE);
            fs::write(&active_path, instance_1.to_string_lossy().as_bytes()).unwrap();

            let args = Args {};
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(instance_remove_all(&args))
                .unwrap();

            assert!(
                !active_path.exists(),
                "active-instance file should be cleared"
            );
            assert!(!instances_dir.exists(), "instances dir should be removed");
        });
    }
}
