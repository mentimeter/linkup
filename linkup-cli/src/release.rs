use std::{
    env, fs,
    path::PathBuf,
    time::{self, Duration},
};

use flate2::read::GzDecoder;
use linkup::VersionChannel;
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};
use tar::Archive;
use url::Url;

use crate::{linkup_file_path, Version};

const CACHED_LATEST_STABLE_RELEASE_FILE: &str = "latest_release_stable.json";
const CACHED_LATEST_BETA_RELEASE_FILE: &str = "latest_release_beta.json";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ReqwestError: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("IoError: {0}")]
    Io(#[from] std::io::Error),
    #[error("File missing from downloaded compressed archive")]
    MissingBinary,
    #[error("Hit a rate limit while checking for updates")]
    RateLimit(u64),
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
}

#[derive(Serialize, Deserialize)]
struct CachedLatestRelease {
    time: u64,
    next_check: u64,
    release: Option<Release>,
}

pub struct Update {
    pub version: Version,
    pub linkup: Asset,
}

pub async fn available_update(
    current_version: &Version,
    desired_channel: Option<linkup::VersionChannel>,
) -> Option<Update> {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    let channel = desired_channel.unwrap_or_else(|| current_version.channel());
    log::debug!("Looking for available update on '{channel}' channel.");

    let latest_release = match cached_latest_release(&channel).await {
        Some(cached_latest_release) if cached_latest_release.release.is_some() => {
            let release = cached_latest_release
                .release
                .expect("release should have been checked for is_some before reaching here");

            log::debug!("Found cached release: {}", release.version);

            release
        }
        _ => {
            log::debug!("No cached release found. Fetching from remote...");

            let release = match channel {
                linkup::VersionChannel::Stable => fetch_stable_release().await,
                linkup::VersionChannel::Beta => fetch_beta_release().await,
            };

            let cache_file = match channel {
                VersionChannel::Stable => CACHED_LATEST_STABLE_RELEASE_FILE,
                VersionChannel::Beta => CACHED_LATEST_BETA_RELEASE_FILE,
            };

            let release = match release {
                Ok(Some(release)) => {
                    log::debug!("Found release {} on channel '{channel}'.", release.version);

                    release
                }
                Ok(None) => {
                    log::debug!("No release found on remote for channel '{channel}'");

                    return None;
                }
                Err(Error::RateLimit(retry_at)) => {
                    log::error!("Hit rate limit while fetching latest release");

                    write_rate_limited_cache_file(cache_file, retry_at);

                    return None;
                }
                Err(error) => {
                    log::error!("Failed to fetch the latest release: {}", error);

                    return None;
                }
            };

            match fs::File::create(linkup_file_path(cache_file)) {
                Ok(new_file) => {
                    let release_cache = CachedLatestRelease {
                        time: now(),
                        next_check: next_morning_utc_seconds(),
                        release: Some(release),
                    };

                    if let Err(error) = serde_json::to_writer_pretty(new_file, &release_cache) {
                        log::error!("Failed to write the release data into cache: {}", error);
                    }

                    release_cache
                        .release
                        .expect("release should have been set when creating the cache")
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

    // Only check the version if the channel is the same.
    if current_version.channel() == latest_version.channel() && current_version >= &latest_version {
        log::debug!("Current version ({current_version}) is newer than latest ({latest_version}).");

        return None;
    }

    let linkup = latest_release
        .linkup_asset(os, arch)
        .expect("Linkup asset to be present on a release");

    Some(Update {
        version: latest_version,
        linkup,
    })
}

async fn fetch_stable_release() -> Result<Option<Release>, Error> {
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

    let response = client.execute(req).await?;

    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        // https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api?apiVersion=2022-11-28#checking-the-status-of-your-rate-limit
        let retry_at = response
            .headers()
            .get("x-ratelimit-reset")
            .and_then(|value| value.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or_else(next_morning_utc_seconds);

        return Err(Error::RateLimit(retry_at));
    }

    let release = response.json().await?;

    Ok(Some(release))
}

pub async fn fetch_beta_release() -> Result<Option<Release>, Error> {
    let url: Url = "https://api.github.com/repos/mentimeter/linkup/releases"
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

    let response = client.execute(req).await?;

    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        // https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api?apiVersion=2022-11-28#checking-the-status-of-your-rate-limit
        let retry_at = response
            .headers()
            .get("x-ratelimit-reset")
            .and_then(|value| value.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or_else(next_morning_utc_seconds);

        return Err(Error::RateLimit(retry_at));
    }

    let releases: Vec<Release> = response.json().await?;

    let beta_release = releases
        .into_iter()
        .find(|release| release.version.starts_with("0.0.0-next-"));

    Ok(beta_release)
}

async fn cached_latest_release(channel: &VersionChannel) -> Option<CachedLatestRelease> {
    let file = match channel {
        VersionChannel::Stable => CACHED_LATEST_STABLE_RELEASE_FILE,
        VersionChannel::Beta => CACHED_LATEST_STABLE_RELEASE_FILE,
    };

    let path = linkup_file_path(file);
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

    if now() > cached_latest_release.next_check {
        if let Err(error) = fs::remove_file(&path) {
            log::error!("Failed to delete cached latest release file: {}", error);
        }

        return None;
    }

    Some(cached_latest_release)
}

pub fn write_rate_limited_cache_file(cache_file: &str, retry_at: u64) {
    if let Ok(file) = fs::File::create(linkup_file_path(cache_file)) {
        let release_cache = CachedLatestRelease {
            time: now(),
            next_check: retry_at,
            release: None,
        };

        if let Err(error) = serde_json::to_writer_pretty(file, &release_cache) {
            log::error!("Failed to write rate-limited data into cache: {}", error);
        }
    }
}

pub fn clear_cache() {
    for path in [
        linkup_file_path(CACHED_LATEST_STABLE_RELEASE_FILE),
        linkup_file_path(CACHED_LATEST_BETA_RELEASE_FILE),
    ] {
        if path.exists() {
            if let Err(error) = fs::remove_file(&path) {
                log::error!("Failed to delete release cache file {path:?}: {error}");
            }
        }
    }
}

fn now() -> u64 {
    let now = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .expect("Time went backwards");

    now.as_secs()
}

fn next_morning_utc_seconds() -> u64 {
    let seconds_in_day = 60 * 60 * 24;
    let current_secs = now();

    let secs_since_midnight = current_secs % seconds_in_day;

    current_secs + (seconds_in_day - secs_since_midnight)
}
