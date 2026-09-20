//! Sundial package and user-facing version formatting.

use std::sync::OnceLock;

/// Full Cargo package version used by technical contracts and package metadata.
pub const PACKAGE: &str = env!("CARGO_PKG_VERSION");

/// Release version without a redundant zero patch component.
pub fn public() -> &'static str {
    PACKAGE.strip_suffix(".0").unwrap_or(PACKAGE)
}

/// User-facing release version with the conventional `v` prefix.
pub fn display() -> &'static str {
    static DISPLAY: OnceLock<String> = OnceLock::new();
    DISPLAY.get_or_init(|| format!("v{}", public())).as_str()
}
