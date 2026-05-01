use crate::rural_modeler::dsp::error::NamError;

/// Latest fully-supported NAM file version.
pub const LATEST_FULLY_SUPPORTED_NAM_FILE_VERSION: &str = "0.7.0";

/// Earliest supported NAM file version.
pub const EARLIEST_SUPPORTED_NAM_FILE_VERSION: &str = "0.5.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
}

impl Version {
    pub const fn new(major: i32, minor: i32, patch: i32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

pub fn parse_version(s: &str) -> Result<Version, NamError> {
    let mut it = s.split('.');
    let major = it
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .ok_or_else(|| NamError::UnsupportedVersion(s.to_string()))?;
    let minor = it
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .ok_or_else(|| NamError::UnsupportedVersion(s.to_string()))?;
    let patch = it.next().and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    if it.next().is_some() || major < 0 || minor < 0 || patch < 0 {
        return Err(NamError::UnsupportedVersion(s.to_string()));
    }
    Ok(Version::new(major, minor, patch))
}

/// Verify that a model file version is supported, matching NAM C++ behavior.
pub fn verify_config_version(version_str: &str) -> Result<(), NamError> {
    let version = parse_version(version_str)?;
    let current = parse_version(LATEST_FULLY_SUPPORTED_NAM_FILE_VERSION)?;
    let earliest = parse_version(EARLIEST_SUPPORTED_NAM_FILE_VERSION)?;

    if version < earliest {
        return Err(NamError::UnsupportedVersion(format!(
            "{}. The earliest supported version is {}. Try either converting the model to a more recent version, or update your version of the NAM plugin.",
            version_str, earliest
        )));
    }
    if version.major > current.major
        || (version.major == current.major && version.minor > current.minor)
    {
        return Err(NamError::UnsupportedVersion(format!(
            "{}. The latest fully-supported version is {}.",
            version_str, current
        )));
    }
    Ok(())
}
