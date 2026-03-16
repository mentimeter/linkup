use std::fs;

use anyhow::anyhow;

use crate::{default_linkup_dir_path, Result};

pub const ACTIVE_INSTANCE_FILE: &str = "active-instance";

#[derive(clap::Args)]
pub struct Args {
    /// Instance to switch to (e.g., 1, 2, 3) or "default"
    instance: String,
}

pub fn instance_use(args: &Args) -> Result<()> {
    let active_instance_path = default_linkup_dir_path().join(ACTIVE_INSTANCE_FILE);

    if args.instance == "default" {
        if active_instance_path.exists() {
            fs::remove_file(&active_instance_path)?;
        }
        println!("Switched to default instance");
        return Ok(());
    }

    let instance_dir = default_linkup_dir_path()
        .join("instances")
        .join(&args.instance);

    if !instance_dir.exists() {
        return Err(anyhow!("Instance {} does not exist", args.instance));
    }

    fs::write(
        &active_instance_path,
        instance_dir.to_string_lossy().as_bytes(),
    )?;

    println!("Switched to instance {}", args.instance);

    Ok(())
}

pub fn active_instance_dir() -> Option<std::path::PathBuf> {
    let path = default_linkup_dir_path().join(ACTIVE_INSTANCE_FILE);
    let content = fs::read_to_string(path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let dir = std::path::PathBuf::from(trimmed);
    if dir.exists() {
        Some(dir)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<F: FnOnce(std::path::PathBuf)>(f: F) {
        let _lock = crate::ENV_TEST_MUTEX.lock().unwrap();
        let prev_home = std::env::var("HOME").ok();

        let tmp =
            std::env::temp_dir().join(format!("linkup-test-instance-use-{}", std::process::id()));
        let linkup_dir = tmp.join(".linkup");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&linkup_dir).unwrap();
        unsafe { std::env::set_var("HOME", &tmp) };

        f(linkup_dir);

        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(val) = prev_home {
            unsafe { std::env::set_var("HOME", val) };
        }
    }

    #[test]
    fn test_active_instance_dir_no_file() {
        with_temp_home(|_| {
            assert_eq!(active_instance_dir(), None);
        });
    }

    #[test]
    fn test_active_instance_dir_empty_file() {
        with_temp_home(|linkup_dir| {
            fs::write(linkup_dir.join(ACTIVE_INSTANCE_FILE), "").unwrap();
            assert_eq!(active_instance_dir(), None);
        });
    }

    #[test]
    fn test_active_instance_dir_stale_path() {
        with_temp_home(|linkup_dir| {
            fs::write(
                linkup_dir.join(ACTIVE_INSTANCE_FILE),
                "/nonexistent/path/that/doesnt/exist",
            )
            .unwrap();
            assert_eq!(active_instance_dir(), None);
        });
    }

    #[test]
    fn test_active_instance_dir_valid() {
        with_temp_home(|linkup_dir| {
            let instance_dir = linkup_dir.join("instances").join("1");
            fs::create_dir_all(&instance_dir).unwrap();
            fs::write(
                linkup_dir.join(ACTIVE_INSTANCE_FILE),
                instance_dir.to_string_lossy().as_bytes(),
            )
            .unwrap();
            assert_eq!(active_instance_dir(), Some(instance_dir));
        });
    }
}
