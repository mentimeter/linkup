use std::env;

fn main() {
    println!("cargo::rerun-if-env-changed=GITHUB_REF_NAME");

    if let Ok(tag) = env::var("GITHUB_REF_NAME") {
        let pkg_version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
        if tag.contains('.') && tag != pkg_version {
            println!("cargo::rustc-env=LINKUP_VERSION_OVERRIDE={tag}");
        }
    }
}
