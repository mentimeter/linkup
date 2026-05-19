use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=../worker/src");
    println!("cargo::rerun-if-changed=../linkup/src");
    println!("cargo::rerun-if-env-changed=GITHUB_REF_NAME");

    // When building in CI from a git tag (e.g. "4.1.0-rc.1"), override
    // CARGO_PKG_VERSION so the binary reports the prerelease version correctly.
    // GITHUB_REF_NAME is set by GitHub Actions to the tag name on tag pushes.
    if let Ok(tag) = env::var("GITHUB_REF_NAME") {
        // Only override if the tag looks like a semver version (contains a dot)
        // and differs from what Cargo.toml already provides.
        let pkg_version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
        if tag.contains('.') && tag != pkg_version {
            println!("cargo::rustc-env=LINKUP_VERSION_OVERRIDE={tag}");
        }
    }

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR to be set");

    let worker_target_dir = Path::new(&out_dir).join("worker-target");

    let install_status = Command::new("cargo")
        // TODO(augustoccesar)[2026-02-25]: From 0.7.0, the worker-build version is aligned
        //   with the version of the worker crate. When bumping the worker, it needs to also
        //   bump the worker-build.
        .args(["install", "-q", "worker-build@0.1.4"])
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
