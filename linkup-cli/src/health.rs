use std::{
    env,
    fs::{self, File},
    io::Read,
};

use colored::Colorize;
use sysinfo::System;

use crate::{linkup_dir_path, local_config::LocalState, CliError};

pub fn health() -> Result<(), CliError> {
    system_info()?;
    println!();
    session_info()?;
    println!();
    backgroud_services()?;
    println!();
    env_variables()?;
    println!();
    linkup_folder_content()?;

    Ok(())
}

fn env_variables() -> Result<(), CliError> {
    println!("{}", "Environment variables:".bold().italic());

    let expected_vars = [
        "LINKUP_CF_API_TOKEN",
        "LINKUP_CLOUDFLARE_ZONE_ID",
        "LINKUP_CLOUDFLARE_ACCOUNT_ID",
    ];

    for var in expected_vars {
        print!("  {:30}", var);
        match env::var(var) {
            Ok(_) => println!("{}", "OK".blue()),
            Err(_) => println!("{}", "MISSING".yellow()),
        }
    }

    Ok(())
}

fn session_info() -> Result<(), CliError> {
    let state = LocalState::load()?;
    println!("{}", "Session info:".bold().italic());
    println!("  Name: {}", state.linkup.session_name);

    Ok(())
}

fn linkup_folder_content() -> Result<(), CliError> {
    println!("{}", "Linkup Dir:".bold().italic());

    let dir_path = linkup_dir_path();

    println!("  Location: {}", dir_path.to_str().unwrap());

    println!("  Content:");
    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let file_name = entry.file_name().to_str().unwrap().to_string();
        let file_path = entry.path();

        print!("    {}", &file_name);

        if file_name.ends_with("-pid") {
            let mut file = File::open(file_path).unwrap();
            let mut pid = String::new();
            file.read_to_string(&mut pid)?;

            print!(" ({})", pid.trim());
        }

        println!();
    }

    Ok(())
}

fn system_info() -> Result<(), CliError> {
    println!("{}", "System info:".bold().italic());

    println!(
        "  OS: {} ({})",
        System::name().unwrap(),
        System::os_version().unwrap()
    );

    Ok(())
}

fn backgroud_services() -> Result<(), CliError> {
    println!("{}", "Background sevices:".bold().italic());

    let mut sys = System::new_all();
    sys.refresh_all();

    let mut local_dns_pids: Vec<String> = vec![];
    let mut caddy_pids: Vec<String> = vec![];
    let mut cloudflared_pids: Vec<String> = vec![];

    for (pid, process) in sys.processes() {
        let process_name = process.name();

        if process_name == "dnsmasq" {
            local_dns_pids.push(pid.to_string());
        } else if process_name == "caddy" {
            caddy_pids.push(pid.to_string());
        } else if process_name == "cloudflared" {
            cloudflared_pids.push(pid.to_string());
        }
    }

    print!("  Caddy        ");
    if !caddy_pids.is_empty() {
        println!("{} ({})", "RUNNING".blue(), caddy_pids.join(","))
    } else {
        println!("{}", "NOT RUNNING".yellow())
    }

    print!("  Local DNS    ");
    if !local_dns_pids.is_empty() {
        println!("{} ({})", "RUNNING".blue(), local_dns_pids.join(","));
    } else {
        println!("{}", "NOT RUNNING".yellow());
    }

    print!("  Cloudflared  ");
    if !cloudflared_pids.is_empty() {
        println!("{} ({})", "RUNNING".blue(), cloudflared_pids.join(","));
    } else {
        println!("{}", "NOT RUNNING".yellow());
    }

    Ok(())
}
