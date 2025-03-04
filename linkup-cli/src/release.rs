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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    name: String,
    #[serde(rename = "browser_download_url")]
    download_url: String,
}

#[derive(Debug)]
pub struct DownloadedAsset {
    pub path: PathBuf,
}

impl DownloadedAsset {
    pub fn linkup_path(&self) -> Option<PathBuf> {
        let linkup_path = self.path.join("linkup");
        if linkup_path.exists() {
            return Some(linkup_path);
        }

        None
    }

    pub fn caddy_path(&self) -> Option<PathBuf> {
        let caddy_path = self.path.join("linkup-caddy");
        if caddy_path.exists() {
            return Some(caddy_path);
        }

        None
    }
}

impl Asset {
    pub async fn download_decompressed(&self) -> Result<DownloadedAsset, Error> {
        let response = reqwest::get(&self.download_url).await?;

        let asset_path = env::temp_dir().join(&self.name);
        let mut file = fs::File::create(&asset_path)?;

        let mut content = std::io::Cursor::new(response.bytes().await?);
        std::io::copy(&mut content, &mut file)?;

        let compressed_release = fs::File::open(&asset_path)?;
        let decompressed_dir_path = env::temp_dir().join(self.name.replace(".tar.gz", ""));

        let decoder = GzDecoder::new(compressed_release);
        let mut archive = Archive::new(decoder);

        archive.unpack(&decompressed_dir_path)?;

        Ok(DownloadedAsset {
            path: decompressed_dir_path,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Release {
    #[serde(rename = "name")]
    version: String,
    assets: Vec<Asset>,
}

impl Release {
    /// Examples assets files:
    /// release-2.1.2-darwin-aarch64.tar.gz
    /// release-2.1.2-darwin-x86_64.tar.gz
    /// release-2.1.2-linux-aarch64.tar.gz
    /// release-2.1.2-linux-x86_64.tar.gz
    pub fn matching_asset(&self, os: &str, arch: &str) -> Option<Asset> {
        let lookup_os = match os {
            "macos" => "darwin",
            other => other,
        };

        let asset = self
            .assets
            .iter()
            .find(|asset| asset.name.ends_with(&format!("{lookup_os}-{arch}.tar.gz")))
            .cloned();

        asset
    }
}

#[derive(Serialize, Deserialize)]
struct CachedLatestRelease {
    time: u64,
    release: Release,
}

pub async fn available_update(current_version: &Version) -> Option<Asset> {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    let latest_release = match cached_latest_release().await {
        Some(cached_latest_release) => cached_latest_release.release,
        None => {
            let release = match fetch_latest_release().await {
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

    latest_release.matching_asset(os, arch)
}

async fn fetch_latest_release() -> Result<Release, reqwest::Error> {
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

pub async fn fetch_release(tag: &str) -> Result<Option<Release>, reqwest::Error> {
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

    let res = client.execute(req).await?;
    if !res.status().is_success() {
        let status = res.status();

        match res.text().await {
            Ok(body) => {
                log::error!(
                    "Failed to fetch release: HTTP Status {}; Body: {}",
                    status,
                    body
                );
            }
            Err(_) => {
                log::error!("Failed to fetch release: HTTP Status {}", status);
            }
        }

        return Ok(None);
    }

    res.json().await
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
