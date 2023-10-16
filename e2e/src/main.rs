extern crate anyhow;
extern crate nix;
extern crate reqwest;

mod boot_cfworker;
mod boot_server;
mod run_cli;

use anyhow::{anyhow, Result};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use reqwest::blocking::Client;
use reqwest::header::REFERER;
use std::fs::{self, read_to_string, File};
use std::io::Write;
use std::str::FromStr;
use std::time::Duration;
use std::{env, thread};

use boot_cfworker::{boot_worker, kill_worker};
use boot_server::boot_background_web_server;
use run_cli::{build_cli_project, run_cli_binary};

use crate::boot_cfworker::wait_worker_started;

type CleanupFunc = Box<dyn FnOnce() -> Result<()> + 'static + Send>;

struct Cleanup {
    funcs: Vec<CleanupFunc>,
}

impl Cleanup {
    fn new() -> Self {
        Cleanup { funcs: Vec::new() }
    }

    fn add<F: FnOnce() -> Result<()> + 'static + Send>(&mut self, f: F) {
        self.funcs.push(Box::new(f));
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for func in self.funcs.drain(..) {
            if let Err(e) = func() {
                eprintln!("Error during cleanup: {}", e);
            }
        }
    }
}

fn run_with_cleanup() -> Result<()> {
    const CONF_STR: &str = r#"
  linkup:
    remote: http://localhost:8787
  services:
    - name: frontend
      remote: https://example.com
      local: http://localhost:8901
      rewrites:
        - source: /foo/(.*)
          target: /bar/$1
    - name: backend
      remote: http://menti.com
      local: http://localhost:8911
      # Set dir to this to test env var copies
      directory: ./
  domains:
    - domain: somedomain.com
      default_service: frontend
      routes:
        - path: /api/v1/.*
          service: backend
    - domain: api.somedomain.com
      default_service: backend
      "#;

    let mut cleanup = Cleanup::new();

    build_cli_project()?;

    let mut file = File::create("e2e_conf.yml")?;
    file.write_all(CONF_STR.as_bytes())?;
    cleanup.add(|| std::fs::remove_file("e2e_conf.yml").map_err(anyhow::Error::from));

    boot_worker()?;
    cleanup.add(kill_worker);
    wait_worker_started()?;

    let (out, err) = run_cli_binary(vec!["start", "-c", "e2e_conf.yml"])?;
    println!("out: {}", out);
    println!("err: {}", err);
    cleanup.add(move || {
        // print_linkup_files().expect("print_linkup_files failed");
        std::fs::remove_dir_all(format!("{}/.linkup", env::var("HOME").unwrap()))
            .map_err(anyhow::Error::from)
    });

    let env_file = read_to_string(".env")?;
    if !env_file.contains("SOME_API_VAR=foobar.com") {
        return Err(anyhow::Error::msg(
            "env file does not contain SOME_API_VAR=foobar.com after start",
        ));
    }

    let referer_to = format!("http://{}.somedomain.com", out.trim());
    println!("referer_to: {}", referer_to);
    println!("referer_to: {}", referer_to);

    let mut front_remote = boot_background_web_server(8901, String::from("front_remote"))?;
    cleanup.add(move || front_remote.kill().map_err(anyhow::Error::from));

    let mut back_remote = boot_background_web_server(8911, String::from("back_remote"))?;
    cleanup.add(move || back_remote.kill().map_err(anyhow::Error::from));

    let client = Client::new();

    let resp = client
        .get("http://localhost:8787")
        .header(REFERER, &referer_to)
        .send()?;

    if resp.status().as_u16() != 200 {
        return Err(anyhow::Error::msg("status code is not 200"));
    }
    if !String::from_utf8(resp.bytes().unwrap().to_vec())
        .unwrap()
        .contains("Example Domain")
    {
        return Err(anyhow::Error::msg("body does not contain Example Domain"));
    }

    let (out, err) = run_cli_binary(vec!["local", "frontend"])?;
    println!("out: {}", out);
    println!("err: {}", err);

    thread::sleep(Duration::from_secs(1));

    let resp = client
        .get("http://localhost:8787")
        .header(REFERER, referer_to)
        .send()?;

    if resp.status().as_u16() != 200 {
        return Err(anyhow::Error::msg("status code is not 200"));
    }
    if !String::from_utf8(resp.bytes().unwrap().to_vec())
        .unwrap()
        .contains("front_remote")
    {
        return Err(anyhow::Error::msg(
            "session did not route to local domain after local switch",
        ));
    }

    let localserver_pid_file = fs::read_to_string(format!(
        "{}/.linkup/localserver-pid",
        env::var("HOME").unwrap()
    ))
    .unwrap();
    let cloudflared_pid_file = fs::read_to_string(format!(
        "{}/.linkup/cloudflared-pid",
        env::var("HOME").unwrap()
    ))
    .unwrap();
    let localserver_pid = localserver_pid_file.trim();
    let cloudflared_pid = cloudflared_pid_file.trim();

    run_cli_binary(vec!["stop"])?;

    thread::sleep(Duration::from_secs(2));

    check_process_dead(localserver_pid)?;
    check_process_dead(cloudflared_pid)?;

    let env_file = read_to_string(".env")?;
    if env_file.contains("SOME_API_VAR=foobar.com") {
        return Err(anyhow::Error::msg(
            "env file should not contain SOME_API_VAR=foobar.com after stop",
        ));
    }

    Ok(())
}

fn check_process_dead(pid_str: &str) -> Result<()> {
    // Parse the PID string to a i32
    let pid_num = i32::from_str(pid_str).map_err(|e| anyhow!("Failed to parse PID: {}", e))?;

    // Create a Pid from the i32
    let pid = Pid::from_raw(pid_num);

    // Use the kill function with a signal of 0 to check if the process is alive
    match kill(pid, Some(Signal::SIGCHLD)) {
        Ok(_) => Err(anyhow!("Process is still alive")),
        Err(_) => Ok(()),
    }
}

fn main() {
    if let Err(e) = run_with_cleanup() {
        println!("An error occurred: {}", e);
        // Perform any additional error handling or logging here
    }
}
