use std::fmt::Display;

#[derive(thiserror::Error, Debug)]
pub enum VersionError {
    #[error("Failed to parse version '{0}'")]
    Parsing(String),
}

#[derive(Debug, Clone)]
pub struct Version {
    major: u16,
    minor: u16,
    patch: u16,
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major && self.minor == other.minor && self.patch == other.patch
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
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

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;

        Ok(())
    }
}

impl TryFrom<&str> for Version {
    type Error = VersionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let (major, minor, patch) = match value.split('.').collect::<Vec<&str>>()[..] {
            [major, minor, patch] => (major, minor, patch),
            _ => return Err(VersionError::Parsing(value.to_string())),
        };

        Ok(Self {
            major: major.parse::<u16>().unwrap(),
            minor: minor.parse::<u16>().unwrap(),
            patch: patch.parse::<u16>().unwrap(),
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
    fn test_display() {
        let version = Version {
            major: 1,
            minor: 2,
            patch: 3,
        };

        assert_eq!(version.to_string(), "1.2.3");
    }
}
