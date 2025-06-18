use std::fmt::Display;

use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug)]
pub enum VersionError {
    #[error("Failed to parse version '{0}'")]
    Parsing(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionChannel {
    Stable,
    Beta,
}

impl Display for VersionChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionChannel::Stable => write!(f, "stable"),
            VersionChannel::Beta => write!(f, "beta"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub pre_release: Option<String>,
}

impl Version {
    pub fn channel(&self) -> VersionChannel {
        match &self.pre_release {
            Some(_) => VersionChannel::Beta,
            None => VersionChannel::Stable,
        }
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major
            && self.minor == other.minor
            && self.patch == other.patch
            && self.pre_release == other.pre_release
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (&self.pre_release, &other.pre_release) {
            (Some(a), Some(b)) => a.cmp(b).into(),
            (Some(_), None) => {
                // pre-release is always lower than stable
                Some(std::cmp::Ordering::Less)
            }
            (None, Some(_)) => {
                // stable is always higher than pre-release
                Some(std::cmp::Ordering::Greater)
            }
            (None, None) => {
                match (
                    self.major.cmp(&other.major),
                    self.minor.cmp(&other.minor),
                    self.patch.cmp(&other.patch),
                ) {
                    (std::cmp::Ordering::Equal, std::cmp::Ordering::Equal, ord) => Some(ord),
                    (std::cmp::Ordering::Equal, ord, _) => Some(ord),
                    (ord, _, _) => Some(ord),
                }
            }
        }
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(pre) = &self.pre_release {
            write!(f, "{}.{}.{}-{}", self.major, self.minor, self.patch, pre)
        } else {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        }
    }
}

impl TryFrom<&str> for Version {
    type Error = VersionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let parts: Vec<&str> = value.split('-').collect();
        let version_part = parts[0];

        let (major, minor, patch) = match version_part.split('.').collect::<Vec<&str>>()[..] {
            [major, minor, patch] => (major, minor, patch),
            _ => return Err(VersionError::Parsing(value.to_string())),
        };

        let pre_release = if parts.len() > 1 {
            Some(parts[1..].join("-"))
        } else {
            None
        };

        Ok(Self {
            major: major
                .parse::<u16>()
                .map_err(|_| VersionError::Parsing(value.to_string()))?,
            minor: minor
                .parse::<u16>()
                .map_err(|_| VersionError::Parsing(value.to_string()))?,
            patch: patch
                .parse::<u16>()
                .map_err(|_| VersionError::Parsing(value.to_string()))?,
            pre_release,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    #[test]
    fn test_version_from_str_ok() {
        let version = Version::try_from("1.2.3").unwrap();

        assert_eq!(version.major, 1);
        assert_eq!(version.minor, 2);
        assert_eq!(version.patch, 3);
    }

    #[test]
    fn test_version_from_str_invalid() {
        let version = Version::try_from("1.2");

        assert!(version.is_err());

        let error = version.err().unwrap();
        assert!(matches!(&error, super::VersionError::Parsing(_)));
        assert_eq!(error.to_string(), "Failed to parse version '1.2'");
    }

    #[test]
    fn test_version_from_str_invalid_token() {
        let version = Version::try_from("1.2.blah");

        assert!(version.is_err());

        let error = version.err().unwrap();
        assert!(matches!(&error, super::VersionError::Parsing(_)));
        assert_eq!(error.to_string(), "Failed to parse version '1.2.blah'");
    }

    #[test]
    fn test_is_outdated_patch() {
        let version = Version::try_from("0.0.0").unwrap();
        let newer_version = Version::try_from("0.0.1").unwrap();

        assert!(newer_version > version);
        assert!(newer_version >= version);
    }

    #[test]
    fn test_is_outdated_minor() {
        let version = Version::try_from("0.0.1").unwrap();
        let newer_version = Version::try_from("0.1.0").unwrap();

        assert!(newer_version > version);
        assert!(newer_version >= version);
    }

    #[test]
    fn test_is_outdated_major() {
        let version = Version::try_from("0.1.2").unwrap();
        let newer_version = Version::try_from("1.0.0").unwrap();

        assert!(newer_version > version);
        assert!(newer_version >= version);
    }

    #[test]
    fn test_is_same() {
        let version = Version::try_from("1.2.3").unwrap();
        let newer_version = Version::try_from("1.2.3").unwrap();

        assert!(newer_version == version);
        assert!(newer_version >= version);
        assert!(newer_version <= version);
    }

    #[test]
    fn test_pre_release_vs_stable() {
        let pre_release_version = Version::try_from("0.0.0-next-20250317-abc123").unwrap();
        let stable_version = Version::try_from("1.2.3").unwrap();

        assert!(stable_version > pre_release_version);
        assert!(stable_version >= pre_release_version);
    }

    #[test]
    fn test_stable_vs_pre_release() {
        let stable_version = Version::try_from("1.2.3").unwrap();
        let pre_release_version = Version::try_from("0.0.0-next-20250317-abc123").unwrap();

        assert!(pre_release_version <= stable_version);
        assert!(pre_release_version < stable_version);
    }

    #[test]
    fn test_display() {
        let version = Version {
            major: 1,
            minor: 2,
            patch: 3,
            pre_release: None,
        };

        assert_eq!(version.to_string(), "1.2.3");
    }

    #[test]
    fn test_display_pre_release() {
        let version = Version {
            major: 1,
            minor: 2,
            patch: 3,
            pre_release: Some("next-20250317-abc123".into()),
        };

        assert_eq!(version.to_string(), "1.2.3-next-20250317-abc123");
    }
}
