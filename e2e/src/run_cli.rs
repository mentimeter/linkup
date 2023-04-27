use anyhow::{anyhow, Result};
use std::env;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

pub fn build_cli_project() -> Result<()> {
    // Store the original current working directory
    let original_cwd = env::current_dir()?;

    // Change the current working directory to the '../cli' folder
    env::set_current_dir(Path::new("../cli"))?;

    // Run the 'cargo build --release' command to build the CLI project
    let mut cmd = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

    // Wait for the child process to finish
    let status = cmd.wait()?;

    // Restore the original current working directory
    env::set_current_dir(original_cwd)?;

    if !status.success() {
        Err(anyhow!("Command failed: {}", status,))?;
    }

    Ok(())
}

pub fn run_cli_binary(args: Vec<&str>) -> Result<(String, String)> {
    // Build the path to the compiled binary
    let binary_path = Path::new("../target/release/cli");

    // Run the compiled binary with the provided arguments and capture stdout and stderr
    let mut cmd = Command::new(binary_path)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Separate variables for stdout and stderr before spawning threads
    let stdout_pipe = cmd.stdout.take();
    let stderr_pipe = cmd.stderr.take();

    // Spawn a thread to read stdout
    let stdout_handle = thread::spawn(move || {
        let mut stdout = String::new();
        if let Some(mut out) = stdout_pipe {
            let mut reader = BufReader::new(&mut out);
            reader.read_to_string(&mut stdout).unwrap();
        }
        stdout
    });

    // Spawn a thread to read stderr
    let stderr_handle = thread::spawn(move || {
        let mut stderr = String::new();
        if let Some(mut err) = stderr_pipe {
            let mut reader = BufReader::new(&mut err);
            reader.read_to_string(&mut stderr).unwrap();
        }
        stderr
    });

    // Wait for the child process to finish
    let status = cmd.wait()?;

    // Join the stdout and stderr reading threads
    let stdout = stdout_handle.join().unwrap();
    let stderr = stderr_handle.join().unwrap();

    if !status.success() {
        Err(anyhow!(
            "Command failed: {}, {}, {}",
            status,
            stdout,
            stderr
        ))?;
    }

    Ok((stdout, stderr))
}
