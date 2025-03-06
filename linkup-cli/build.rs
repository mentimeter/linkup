use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os == "macos" {
        println!("cargo:rustc-cfg=feature=\"localdns\"");
    }

    if target_os == "linux" && env::var("CARGO_FEATURE_LOCALDNS").is_ok() {
        panic!("The `localdns` feature is not supported on Linux");
    }

    println!("cargo::rerun-if-changed=../worker/src");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR to be set");

    let worker_target_dir = Path::new(&out_dir).join("worker-target");

    let install_status = Command::new("cargo")
        .args(["install", "-q", "worker-build"])
        .current_dir("../worker")
        .env("CARGO_TARGET_DIR", &worker_target_dir)
        .status()
        .expect("failed to execute worker-build install process");

    if !install_status.success() {
        panic!("Failed to install worker-build");
    }

    let build_status = Command::new("worker-build")
        .args(["--release"])
        .current_dir("../worker")
        .env("CARGO_TARGET_DIR", &worker_target_dir)
        .status()
        .expect("failed to execute worker-build process");

    if !build_status.success() {
        panic!("Failed to build worker");
    }

    let shim_src = Path::new("../worker/build/worker/shim.mjs");
    let wasm_src = Path::new("../worker/build/worker/index.wasm");
    let shim_dest = Path::new(&out_dir).join("shim.mjs");
    let wasm_dest = Path::new(&out_dir).join("index.wasm");

    fs::create_dir_all(&out_dir).expect("failed to create output directories");

    fs::copy(shim_src, &shim_dest).expect("failed to copy shim.mjs");
    fs::copy(wasm_src, &wasm_dest).expect("failed to copy index.wasm");
}
