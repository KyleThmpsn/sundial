//! Runtime controls that later Sunrise releases stopped reading.

use crate::package_runtime::runtime_version_at_least;

/// Settings Sunrise 0.5 deleted along with the code that read them.
///
/// 0.5 dropped the three client flags from `core/settings/client/definition.h` and deleted
/// `activity_arrival_override_parser.cpp` outright. Its parsers fall through to `skip_value` on an
/// unrecognized key, so writing these stays harmless, but nothing consumes them. The settings
/// schema version stayed at 18 across the removal, so the installed module version is the only
/// thing that separates the two shapes.
const RETIRED_IN_SUNRISE_0_5: &[&str] = &[
    "/client/region_private",
    "/client/pin_replicated_record",
    "/client/skip_orbit_cinematic_wait",
    "/state/activity/arrival_overrides",
];

/// Which optional runtime controls the installed module still reads.
///
/// The default supports everything, so an undetected runtime hides nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Capabilities {
    retired_sunrise_0_5: bool,
}

impl Capabilities {
    /// Reads the capabilities of the installed launch copy. Dawn carries its own version line, so
    /// only a Sunrise module retires Sunrise settings.
    pub(crate) fn detect(version: Option<&str>, dawn: bool) -> Self {
        Self {
            retired_sunrise_0_5: !dawn
                && version.is_some_and(|version| runtime_version_at_least(version, &[0, 5])),
        }
    }

    /// True while the installed runtime still reads the setting at `path`.
    pub(crate) fn supports(self, path: &str) -> bool {
        !self.retired_sunrise_0_5 || !RETIRED_IN_SUNRISE_0_5.contains(&path)
    }
}
