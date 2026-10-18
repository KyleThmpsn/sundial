//! Runtime controls that only some Sunrise releases read.

use crate::package_runtime::{normalize_sunrise_version, runtime_version_at_least};

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

/// Settings Sunrise 0.6 added, again without a schema version change.
///
/// 0.6 added `cheats` and `dump_package_keys` to `core/settings/client/definition.h` and
/// `studio_attach` to `core/settings/settings.h`. Earlier parsers skip them, and Dawn reads none.
const ADDED_IN_SUNRISE_0_6: &[&str] = &[
    "/client/cheats",
    "/client/dump_package_keys",
    "/core/studio_attach",
];

/// Which optional runtime controls the installed module reads.
///
/// The default supports everything, so an undetected runtime hides nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Capabilities {
    retired_sunrise_0_5: bool,
    before_sunrise_0_6: bool,
}

impl Capabilities {
    /// Reads the capabilities of the installed launch copy. Dawn carries its own version line, so
    /// only a Sunrise module retires Sunrise settings, and Dawn never reads the 0.6 additions. An
    /// unreadable Sunrise version hides nothing.
    pub(crate) fn detect(version: Option<&str>, dawn: bool) -> Self {
        let sunrise_at_least = |minimum: &[u64]| {
            version
                .filter(|version| !dawn && normalize_sunrise_version(version).is_some())
                .map(|version| runtime_version_at_least(version, minimum))
        };
        Self {
            retired_sunrise_0_5: sunrise_at_least(&[0, 5]) == Some(true),
            before_sunrise_0_6: dawn || sunrise_at_least(&[0, 6]) == Some(false),
        }
    }

    /// True while the installed runtime reads the setting at `path`.
    pub(crate) fn supports(self, path: &str) -> bool {
        let retired = self.retired_sunrise_0_5 && RETIRED_IN_SUNRISE_0_5.contains(&path);
        let not_yet_added = self.before_sunrise_0_6 && ADDED_IN_SUNRISE_0_6.contains(&path);
        !retired && !not_yet_added
    }
}
