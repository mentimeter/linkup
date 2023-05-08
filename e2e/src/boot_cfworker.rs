use std::env;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::Result;

pub fn boot_worker() -> Result<Child> {
    let original_cwd = env::current_dir()?;
    env::set_current_dir(Path::new("../worker"))?;

    Command::new("npm")
        .arg("install")
        .arg("wrangler@latest")
        .status()?;

    let cmd = Command::new("npx")
        .arg("wrangler@latest")
        .arg("dev")
        .stdout(Stdio::null())
        // .stdout(Stdio::inherit())
        // DEBUG POINT, use inherit stderr to see wrangler output
        .stderr(Stdio::null())
        // .stderr(Stdio::inherit())
        .spawn()?;

    thread::sleep(Duration::from_secs(5));

    env::set_current_dir(original_cwd)?;

    Ok(cmd)
}

pub fn kill_worker() -> Result<()> {
    // Run pgrep to find the process ID of the wrangler process
    let pgrep_output = Command::new("pgrep").arg("wrangler").output()?;

    // Check if pgrep was successful and the output is not empty
    if pgrep_output.status.success() && !pgrep_output.stdout.is_empty() {
        // Parse the process ID from the output
        let pid_str = String::from_utf8_lossy(&pgrep_output.stdout);
        let pid: i32 = pid_str.trim().parse()?;

        // Run the kill command with the process ID as an argument
        Command::new("kill").arg(pid.to_string()).status()?;
    } else {
        println!("No wrangler process found.");
    }

    Ok(())
}
