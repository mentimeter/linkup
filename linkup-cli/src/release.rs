mod github {
    use std::{env, fs, path::PathBuf, time::Duration};

    use flate2::read::GzDecoder;
    use linkup::VersionError;
    use reqwest::header::HeaderValue;
    use serde::{de::DeserializeOwned, Deserialize, Serialize};
    use tar::Archive;
    use url::Url;

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("ReqwestError: {0}")]
        Reqwest(#[from] reqwest::Error),
        #[error("IoError: {0}")]
        Io(#[from] std::io::Error),
        #[error("File missing from downloaded compressed archive")]
        MissingBinary,
        #[error("Release have an invalid tag")]
        InvalidVersionTag(#[from] VersionError),
        #[error("Hit a rate limit while checking for updates")]
        RateLimit(u64),
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Release {
        #[serde(rename = "name")]
        pub version: String,
        pub assets: Vec<Asset>,
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

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Asset {
        name: String,
        #[serde(rename = "browser_download_url")]
        download_url: String,
    }

    impl Asset {
        async fn inner_download(&self) -> Result<PathBuf, Error> {
            let response = reqwest::get(&self.download_url).await?;

            let file_path = env::temp_dir().join(&self.name);
            let mut file = fs::File::create(&file_path)?;

            let mut content = std::io::Cursor::new(response.bytes().await?);
            std::io::copy(&mut content, &mut file)?;

            Ok(file_path)
        }

        pub async fn download(&self) -> Result<PathBuf, Error> {
            let filename = "linkup";
            let file_path = self.inner_download().await?;
            let file = fs::File::open(&file_path)?;

            let decoder = GzDecoder::new(file);
            let mut archive = Archive::new(decoder);

            let new_exe_path = archive.entries()?.filter_map(|e| e.ok()).find_map(
                |mut entry| -> Option<PathBuf> {
                    let entry_path = entry.path().unwrap();

                    if entry_path.to_str().unwrap().contains(filename) {
                        let path = env::temp_dir().join(filename);

                        entry.unpack(&path).unwrap();

                        Some(path)
                    } else {
                        None
                    }
                },
            );

            match new_exe_path {
                Some(new_exe_path) => Ok(new_exe_path),
                None => Err(Error::MissingBinary),
            }
        }
    }

    pub(super) async fn fetch_stable_release() -> Result<Option<Release>, Error> {
        let url: Url = "https://api.github.com/repos/mentimeter/linkup/releases/latest"
            .parse()
            .expect("GitHub URL to be correct");

        let release = fetch(url).await?;

        Ok(Some(release))
    }

    pub(super) async fn fetch_beta_release() -> Result<Option<Release>, Error> {
        let url: Url = "https://api.github.com/repos/mentimeter/linkup/releases"
            .parse()
            .expect("GitHub URL to be correct");

        let releases: Vec<Release> = fetch(url).await?;

        let beta_release = releases
            .into_iter()
            .find(|release| release.version.starts_with("0.0.0-next-"));

        Ok(beta_release)
    }

    async fn fetch<T>(url: Url) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        let mut req = reqwest::Request::new(reqwest::Method::GET, url);
        let headers = req.headers_mut();
        headers.append("User-Agent", HeaderValue::from_static("linkup-cli"));
        headers.append(
            "Accept",
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.append(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
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
                .unwrap_or_else(super::next_morning_utc_seconds);

            return Err(Error::RateLimit(retry_at));
        }

        Ok(response.json::<T>().await?)
    }
}

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::linkup_file_path;
use github::Asset;
use linkup::{Version, VersionChannel};

const CACHE_FILE_NAME: &str = "releases_cache.json";

#[derive(Clone, Serialize, Deserialize)]
pub struct Release {
    pub channel: VersionChannel,
    pub version: Version,
    pub binary: Asset,
}

impl Release {
    fn from_github_release(gh_release: &github::Release, os: &str, arch: &str) -> Option<Release> {
        let version = Version::try_from(gh_release.version.as_str());
        let asset = gh_release.linkup_asset(os, arch);

        match (version, asset) {
            (Ok(version), Some(asset)) => Some(Release {
                channel: version.channel(),
                version,
                binary: asset,
            }),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct CachedReleases {
    fetched_at: u64,
    next_fetch_at: u64,
    releases: Vec<Release>,
}

impl CachedReleases {
    fn empty_with_retry(retry_at: u64) -> Self {
        Self {
            fetched_at: now(),
            next_fetch_at: retry_at,
            releases: Vec::default(),
        }
    }

    fn cache_path() -> PathBuf {
        linkup_file_path(CACHE_FILE_NAME)
    }

    /// Always return the cache only if is "fresh". If the cache is expired, this will delete the
    /// cache file and return None.
    fn load() -> Option<Self> {
        let path = linkup_file_path(CACHE_FILE_NAME);
        if !path.exists() {
            return None;
        }

        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(error) => {
                log::debug!("failed to open cached latest release file: {}", error);

                return None;
            }
        };

        let cache: Self = match serde_json::from_reader(file) {
            Ok(cache) => cache,
            Err(error) => {
                log::debug!("failed to parse cached latest release: {}", error);

                if std::fs::remove_file(&path).is_err() {
                    log::debug!("failed to delete latest release cache file");
                }

                return None;
            }
        };

        if now() > cache.next_fetch_at {
            Self::clear();

            return None;
        }

        Some(cache)
    }

    fn save(&self) {
        match std::fs::File::create(linkup_file_path(CACHE_FILE_NAME)) {
            Ok(new_file) => {
                if let Err(error) = serde_json::to_writer_pretty(new_file, self) {
                    log::debug!("failed to write the release data into cache: {}", error);
                }
            }
            Err(error) => {
                log::debug!("Failed to create release cache file: {}", error);
            }
        }
    }

    pub fn clear() {
        let path = Self::cache_path();
        if !path.exists() {
            return;
        }

        if let Err(error) = std::fs::remove_file(&path) {
            log::debug!("failed to delete cached latest release file: {}", error);
        }
    }

    fn get_release(&self, channel: VersionChannel) -> Option<&Release> {
        self.releases
            .iter()
            .find(|update| update.channel == channel)
    }
}

async fn fetch_releases(os: &str, arch: &str) -> Result<Vec<Release>, github::Error> {
    // TODO: Could we maybe do a single request to GH to list the releases and do the filtering
    //       locally?
    let mut releases = Vec::<Release>::with_capacity(2);

    if let Some(stable) = github::fetch_stable_release()
        .await?
        .and_then(|gh_release| Release::from_github_release(&gh_release, os, arch))
    {
        releases.push(stable);
    }

    if let Some(beta) = github::fetch_beta_release()
        .await?
        .and_then(|gh_release| Release::from_github_release(&gh_release, os, arch))
    {
        releases.push(beta);
    }

    Ok(releases)
}

pub async fn check_for_update(
    current_version: &Version,
    channel: Option<VersionChannel>,
) -> Option<Release> {
    let channel = channel.unwrap_or_else(|| current_version.channel());
    log::debug!("Looking for available update on '{channel}' channel.");

    let cached_releases = CachedReleases::load();
    match cached_releases {
        Some(cached_releases) => cached_releases.get_release(channel).cloned(),
        None => {
            let os = std::env::consts::OS;
            let arch = std::env::consts::ARCH;

            let new_cache = match fetch_releases(os, arch).await {
                Ok(releases) => {
                    let cache = CachedReleases {
                        fetched_at: now(),
                        next_fetch_at: next_morning_utc_seconds(),
                        releases,
                    };

                    cache.save();

                    cache
                }
                Err(error) => {
                    let cache = match error {
                        github::Error::RateLimit(retry_at) => {
                            CachedReleases::empty_with_retry(retry_at)
                        }
                        _ => CachedReleases::empty_with_retry(next_morning_utc_seconds()),
                    };

                    cache.save();

                    cache
                }
            };

            new_cache.get_release(channel).cloned()
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_secs()
}

fn next_morning_utc_seconds() -> u64 {
    let seconds_in_day = 60 * 60 * 24;
    let now_in_seconds = now();

    let seconds_since_midnight = now_in_seconds % seconds_in_day;

    now_in_seconds + (seconds_in_day - seconds_since_midnight)
}
