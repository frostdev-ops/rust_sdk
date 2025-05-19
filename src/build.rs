//! Module exposing build information constants.
//!
//! These values are automatically populated at build time:
//! - `GIT_HASH`: Git commit hash
//! - `BUILD_TIME_UTC`: Build timestamp in RFC3339 format
//! - `RUSTC_VERSION`: Rustc version used to build the crate

/// Git commit hash of the build, or "unknown" if not available.
pub const GIT_HASH: &str = env!("PYWATT_GIT_HASH");

/// Build timestamp in RFC3339 format.
pub const BUILD_TIME_UTC: &str = env!("PYWATT_BUILD_TIME_UTC");

/// Rustc version used for the build.
pub const RUSTC_VERSION: &str = env!("PYWATT_RUSTC_VERSION");
