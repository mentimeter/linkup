use std::{
    env::{self},
    fmt::Display,
    fs,
    path::PathBuf,
    time::{self, Duration},
};

use flate2::read::GzDecoder;
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};
use tar::Archive;
use url::Url;

use crate::{linkup_file_path, CliError};

const CACHED_LATEST_RELEASE_FILE: &str = "latest_release.json";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("{0}")]
    InvalidVersion(String),
    #[error("File missing from dowloaded compressed archive")]
    MissingBinary,
    #[error("ReqwestError: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("IoError: {0}")]
    Io(#[from] std::io::Error),
}

pub async fn update() -> Result<(), CliError> {
    match available_update().await {
        Some(asset) => {
            let new_exe_path = asset.download_decompressed().await.unwrap();

            let current_exe = get_exe_path().expect("failed to get the current exe path");
            let bkp_exe = current_exe.with_extension("bkp");

            fs::rename(&current_exe, &bkp_exe).unwrap();
            fs::rename(&new_exe_path, &current_exe).unwrap();

            fs::remove_file(bkp_exe).unwrap();

            println!("Finished update!");
        }
        None => {
            println!("No new version available.");
        }
    }

    Ok(())
}

pub async fn new_version_available() -> bool {
    available_update().await.is_some()
}

// --------------------------------------------------------------------------------------------------------------------

struct Version {
    major: u16,
    minor: u16,
    patch: u16,
}

impl Version {
    // NOTE: This is a super simple and naïve implementation of this. For our case I think should be enough,
    //       but we could consider using something like https://docs.rs/semver/latest/semver/ if we want something
    //       more robust.
    fn is_outdated(&self, other: &Self) -> bool {
        let same_major = self.major == other.major;
        let same_minor = self.minor == other.minor;
        let same_patch = self.patch == other.patch;

        match (same_major, same_minor, same_patch) {
            (true, true, true) => false,
            (true, true, false) => self.patch < other.patch,
            (true, false, _) => self.minor < other.minor,
            (false, _, _) => self.major < other.major,
        }
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;

        Ok(())
    }
}

impl TryFrom<&str> for Version {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let (major, minor, patch) = match value.split('.').collect::<Vec<&str>>()[..] {
            [major, minor, patch] => (major, minor, patch),
            _ => return Err(Error::InvalidVersion(value.to_string())),
        };

        Ok(Self {
            major: major.parse::<u16>().unwrap(),
            minor: minor.parse::<u16>().unwrap(),
            patch: patch.parse::<u16>().unwrap(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Asset {
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

    pub async fn download_decompressed(&self) -> Result<PathBuf, Error> {
        let file_path = self.download().await?;
        let file = fs::File::open(&file_path)?;

        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);

        let new_exe_path =
            archive
                .entries()?
                .filter_map(|e| e.ok())
                .find_map(|mut entry| -> Option<PathBuf> {
                    if entry.path().unwrap().to_str().unwrap() == "linkup" {
                        let path = env::temp_dir().join("linkup");

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
struct Release {
    #[serde(rename = "name")]
    version: String,
    assets: Vec<Asset>,
}

impl Release {
    pub fn asset_for(&self, os: &str, arch: &str) -> Option<Asset> {
        let lookup_os = match os {
            "macos" => "apple-darwin",
            "linux" => "unknown-linux",
            _ => return None,
        };

        for asset in &self.assets {
            if asset.name.contains(lookup_os) && asset.name.contains(arch) {
                return Some(asset.clone());
            }
        }

        None
    }
}

#[derive(Serialize, Deserialize)]
struct CachedLatestRelease {
    time: u64,
    release: Release,
}

async fn available_update() -> Option<Asset> {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    let current_version = match Version::try_from(CURRENT_VERSION) {
        Ok(version) => version,
        Err(error) => {
            log::error!(
                "failed to parse current version '{}': {}",
                CURRENT_VERSION,
                error
            );

            return None;
        }
    };

    let release = latest_release().await;
    let latest_version = match Version::try_from(release.version.as_str()) {
        Ok(version) => version,
        Err(error) => {
            log::error!(
                "failed to parse latest version '{}': {}",
                release.version,
                error
            );

            return None;
        }
    };

    release
        .asset_for(os, arch)
        .filter(|_| current_version.is_outdated(&latest_version))
}

async fn latest_release() -> Release {
    match cached_latest_release().await {
        Some(cached_latest_release) => cached_latest_release.release,
        None => {
            let release = fetch_latest_release().await;
            match fs::File::create(linkup_file_path(CACHED_LATEST_RELEASE_FILE)) {
                Ok(new_file) => {
                    if let Err(error) = serde_json::to_writer_pretty(new_file, &release) {
                        log::error!("Failed to write the release data into cache: {}", error);
                    }

                    release
                }
                Err(error) => {
                    log::error!("Failed to create release cache file: {}", error);

                    release
                }
            }
        }
    }
}

async fn fetch_latest_release() -> Release {
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

    let client = reqwest::Client::new();
    client.execute(req).await.unwrap().json().await.unwrap()
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

// Get the current exe path. Using canonicalize ensure that we follow the symlink in case it is one.
// This is important in case the version is one installed with Homebrew.
fn get_exe_path() -> Result<PathBuf, Error> {
    Ok(fs::canonicalize(std::env::current_exe()?)?)
}

fn now() -> u64 {
    let start = time::SystemTime::now();

    let since_the_epoch = start.duration_since(time::UNIX_EPOCH).unwrap();

    since_the_epoch.as_secs()
}
