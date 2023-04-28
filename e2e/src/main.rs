extern crate anyhow;

mod boot_cfworker;
mod boot_server;
mod run_cli;

use anyhow::Result;
use std::fs::File;
use std::io::Write;
use std::time::Duration;
use std::{env, thread};

use boot_cfworker::{boot_worker, kill_worker};
use boot_server::boot_background_web_server;
use run_cli::{build_cli_project, run_cli_binary};

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
      remote: http://localhost:8900
      local: http://localhost:8901
      path_modifiers:
        - source: /foo/(.*)
          target: /bar/$1
    - name: backend
      remote: http://localhost:8910
      local: http://localhost:8911
  domains:
    - domain: example.com
      default_service: frontend
      routes:
        - path: /api/v1/.*
          service: backend
    - domain: api.example.com
      default_service: backend
      "#;

    let mut cleanup = Cleanup::new();

    build_cli_project()?;

    let mut file = File::create("e2e_conf.yml")?;
    file.write_all(CONF_STR.as_bytes())?;
    cleanup.add(|| std::fs::remove_file("e2e_conf.yml").map_err(anyhow::Error::from));

    boot_worker()?;
    cleanup.add(move || kill_worker());

    thread::sleep(Duration::from_secs(5));

    let (out, err) = run_cli_binary(vec!["start", "-c", "e2e_conf.yml"])?;
    println!("out: {}", out);
    println!("err: {}", err);
    // cleanup.add(move || {
    //     std::fs::remove_dir_all(format!("{}/.linkup", env::var("HOME").unwrap()))
    //         .map_err(anyhow::Error::from)
    // });

    thread::sleep(Duration::from_secs(20));

    let mut front_remote = boot_background_web_server(8900, String::from("front_remote"))?;
    cleanup.add(move || front_remote.kill().map_err(anyhow::Error::from));

    let mut back_remote = boot_background_web_server(8910, String::from("back_remote"))?;
    cleanup.add(move || back_remote.kill().map_err(anyhow::Error::from));

    Ok(())
}

fn main() {
    if let Err(e) = run_with_cleanup() {
        println!("An error occurred: {}", e);
        // Perform any additional error handling or logging here
    }
}
