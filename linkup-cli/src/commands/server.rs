use std::fs;

use crate::CliError;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    pidfile: String,
}

pub async fn server(args: &Args) -> Result<(), CliError> {
    let pid = std::process::id();
    fs::write(&args.pidfile, pid.to_string())?;

    let res = linkup_local_server::start_server().await;

    if let Err(pid_file_err) = fs::remove_file(&args.pidfile) {
        eprintln!("Failed to remove pidfile: {}", pid_file_err);
    }

    res.map_err(|e| e.into())
}
