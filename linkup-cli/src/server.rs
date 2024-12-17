use std::fs;

use crate::CliError;

pub async fn server(pidfile: &str) -> Result<(), CliError> {
    let pid = std::process::id();
    fs::write(pidfile, pid.to_string())?;

    let res = linkup_local_server::start_server().await;

    if let Err(pid_file_err) = fs::remove_file(pidfile) {
        eprintln!("Failed to remove pidfile: {}", pid_file_err);
    }

    res.map_err(|e| e.into())
}
