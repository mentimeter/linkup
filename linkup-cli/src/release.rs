use std::{
    env, fs,
    path::PathBuf,
    time::{self, Duration},
};

use flate2::read::GzDecoder;
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};
use tar::Archive;
use url::Url;

use crate::{linkup_file_path, Version};

const CACHED_LATEST_RELEASE_FILE: &str = "latest_release.json";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ReqwestError: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("IoError: {0}")]
    Io(#[from] std::io::Error),
    #[error("File missing from downloaded compressed archive")]
    MissingBinary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    name: String,
    #[serde(rename = "browser_download_url")]
    download_url: String,
}

impl Asset {
    pub async fn download(&self) -> Result<PathBuf, Error> {
        let response = reqwest::get(&self.download_url).await?;

        let file_path = env::temp_dir().join(&self.name);
        let mut file = fs::File::create(&file_path)?;

        let mut content = std::io::Cursor::new(response.bytes().await?);
        std::io::copy(&mut content, &mut file)?;

        Ok(file_path)
    }

    pub async fn download_decompressed(&self, lookup_name: &str) -> Result<PathBuf, Error> {
        let file_path = self.download().await?;
        let file = fs::File::open(&file_path)?;

        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);

        let new_exe_path =
            archive
                .entries()?
                .filter_map(|e| e.ok())
                .find_map(|mut entry| -> Option<PathBuf> {
                    let entry_path = entry.path().unwrap();

                    if entry_path.to_str().unwrap().contains(lookup_name) {
                        let path = env::temp_dir().join(lookup_name);

                        entry.unpack(&path).unwrap();

                        Some(path)
                    } else {
                        None
                    }
                });

        match new_exe_path {
            Some(new_exe_path) => Ok(new_exe_path),
            None => Err(Error::MissingBinary),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Release {
    #[serde(rename = "name")]
    version: String,
    assets: Vec<Asset>,
}

impl Release {
    /// Examples of Linkup asset files:
    /// - linkup-1.7.1-x86_64-apple-darwin.tar.gz
    /// - linkup-1.7.1-aarch64-apple-darwin.tar.gz
    /// - linkup-1.7.1-x86_64-unknown-linux-gnu.tar.gz
    /// - linkup-1.7.1-aarch64-unknown-linux-gnu.tar.gz
    pub fn linkup_asset(&self, os: &str, arch: &str) -> Option<Asset> {
        let lookup_os = match os {
            "macos" => "apple-darwin",
            "linux" => "unknown-linux",
            _ => return None,
        };

        let asset = self
            .assets
            .iter()
            .find(|asset| asset.name.contains(lookup_os) && asset.name.contains(arch))
            .cloned();

        if asset.is_none() {
            log::debug!(
                "Linkup release for OS '{}' and ARCH '{}' not found on version {}",
                lookup_os,
                arch,
                &self.version
            );
        }

        asset
    }

    /// Examples of Caddy asset files:
    /// - caddy-darwin-amd64.tar.gz
    /// - caddy-darwin-arm64.tar.gz
    /// - caddy-linux-amd64.tar.gz
    /// - caddy-linux-arm64.tar.gz
    pub fn caddy_asset(&self, os: &str, arch: &str) -> Option<Asset> {
        let lookup_os = match os {
            "macos" => "darwin",
            "linux" => "linux",
            lookup_os => lookup_os,
        };

        let lookup_arch = match arch {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            lookup_arch => lookup_arch,
        };

        let asset = self
            .assets
            .iter()
            .find(|asset| asset.name == format!("caddy-{}-{}.tar.gz", lookup_os, lookup_arch))
            .cloned();

        if asset.is_none() {
            log::debug!(
                "Caddy release for OS '{}' and ARCH '{}' not found on version {}",
                lookup_os,
                lookup_arch,
                &self.version
            );
        }

        asset
    }
}

#[derive(Serialize, Deserialize)]
struct CachedLatestRelease {
    time: u64,
    release: Release,
}

pub struct Update {
    pub linkup: Asset,
    pub caddy: Asset,
}

pub async fn available_update(current_version: &Version) -> Option<Update> {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    let is_beta = current_version.to_string().contains("-next");

    let latest_release = match cached_latest_release().await {
        Some(cached_latest_release) => cached_latest_release.release,
        None => {
            let release = if is_beta {
                fetch_next_release().await
            } else {
                fetch_stable_release().await
            };

            let release = match release {
                Ok(release) => release,
                Err(error) => {
                    log::error!("Failed to fetch the latest release: {}", error);
                    return None;
                }
            };

            match fs::File::create(linkup_file_path(CACHED_LATEST_RELEASE_FILE)) {
                Ok(new_file) => {
                    let release_cache = CachedLatestRelease {
                        time: now(),
                        release,
                    };

                    if let Err(error) = serde_json::to_writer_pretty(new_file, &release_cache) {
                        log::error!("Failed to write the release data into cache: {}", error);
                    }

                    release_cache.release
                }
                Err(error) => {
                    log::error!("Failed to create release cache file: {}", error);

                    release
                }
            }
        }
    };

    let latest_version = match Version::try_from(latest_release.version.as_str()) {
        Ok(version) => version,
        Err(error) => {
            log::error!(
                "Failed to parse latest version '{}': {}",
                latest_release.version,
                error
            );

            return None;
        }
    };

    if current_version >= &latest_version {
        return None;
    }

    let caddy = latest_release
        .caddy_asset(os, arch)
        .expect("Caddy asset to be present on a release");
    let linkup = latest_release
        .linkup_asset(os, arch)
        .expect("Linkup asset to be present on a release");

    Some(Update { linkup, caddy })
}

async fn fetch_stable_release() -> Result<Release, reqwest::Error> {
    let url: Url = "https://api.github.com/repos/mentimeter/linkup/releases/latest"
        .parse()
        .unwrap();

    let mut req = reqwest::Request::new(reqwest::Method::GET, url);
    let headers = req.headers_mut();
    headers.append("User-Agent", HeaderValue::from_str("linkup-cli").unwrap());
    headers.append(
        "Accept",
        HeaderValue::from_str("application/vnd.github+json").unwrap(),
    );
    headers.append(
        "X-GitHub-Api-Version",
        HeaderValue::from_str("2022-11-28").unwrap(),
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();

    client.execute(req).await?.json().await
}

pub async fn fetch_next_release() -> Result<Release, reqwest::Error> {
    let url: Url = "https://api.github.com/repos/mentimeter/linkup/releases/tags/next"
        .parse()
        .unwrap();

    let mut req = reqwest::Request::new(reqwest::Method::GET, url);
    let headers = req.headers_mut();
    headers.append("User-Agent", HeaderValue::from_str("linkup-cli").unwrap());
    headers.append(
        "Accept",
        HeaderValue::from_str("application/vnd.github+json").unwrap(),
    );
    headers.append(
        "X-GitHub-Api-Version",
        HeaderValue::from_str("2022-11-28").unwrap(),
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();

    client.execute(req).await?.json().await
}

pub async fn fetch_release(version: &Version) -> Result<Option<Release>, reqwest::Error> {
    let tag = version.to_string();

    let url: Url = format!(
        "https://api.github.com/repos/mentimeter/linkup/releases/tags/{}",
        &tag
    )
    .parse()
    .unwrap();

    let mut req = reqwest::Request::new(reqwest::Method::GET, url);
    let headers = req.headers_mut();
    headers.append("User-Agent", HeaderValue::from_str("linkup-cli").unwrap());
    headers.append(
        "Accept",
        HeaderValue::from_str("application/vnd.github+json").unwrap(),
    );
    headers.append(
        "X-GitHub-Api-Version",
        HeaderValue::from_str("2022-11-28").unwrap(),
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();

    client.execute(req).await?.json().await
}

async fn cached_latest_release() -> Option<CachedLatestRelease> {
    let path = linkup_file_path(CACHED_LATEST_RELEASE_FILE);
    if !path.exists() {
        return None;
    }

    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) => {
            log::error!("Failed to open cached latest release file: {}", error);

            return None;
        }
    };

    let cached_latest_release: CachedLatestRelease = match serde_json::from_reader(file) {
        Ok(cached_latest_release) => cached_latest_release,
        Err(error) => {
            log::error!("Failed to parse cached latest release: {}", error);

            if fs::remove_file(&path).is_err() {
                log::error!("Failed to delete latest release cache file");
            }

            return None;
        }
    };

    let cache_time = Duration::from_secs(cached_latest_release.time);
    let time_now = Duration::from_secs(now());

    if time_now - cache_time > Duration::from_secs(60 * 60 * 24) {
        if let Err(error) = fs::remove_file(&path) {
            log::error!("Failed to delete cached latest release file: {}", error);
        }

        return None;
    }

    Some(cached_latest_release)
}

pub fn clear_cache() {
    let path = linkup_file_path(CACHED_LATEST_RELEASE_FILE);

    if path.exists() {
        if let Err(error) = fs::remove_file(path) {
            log::error!("Failed to delete latest release cache file: {}", error);
        }
    }
}

fn now() -> u64 {
    let start = time::SystemTime::now();

    let since_the_epoch = start.duration_since(time::UNIX_EPOCH).unwrap();

    since_the_epoch.as_secs()
}
