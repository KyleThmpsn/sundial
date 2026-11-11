//! Runtime controls that only some Sunrise releases read.

use crate::package_runtime::{normalize_sunrise_version, runtime_version_at_least};

/// Keys ignored by Sunrise 0.5 and later. Settings schema 18 spans both parser shapes,
/// so these controls are gated by the installed module version.
const RETIRED_IN_SUNRISE_0_5: &[&str] = &[
    "/client/region_private",
    "/client/pin_replicated_record",
    "/client/skip_orbit_cinematic_wait",
    "/state/activity/arrival_overrides",
];

/// Keys read by Sunrise 0.6 and later. Earlier Sunrise parsers and Dawn ignore them.
const ADDED_IN_SUNRISE_0_6: &[&str] = &[
    "/client/cheats",
    "/client/dump_package_keys",
    "/client/exports/package_keys",
    "/client/exports/tag_definitions",
    "/core/studio_attach",
    "/multiplayer_enabled",
];

/// Settings schema 19 moved `client.dump_package_keys` to `client.exports.package_keys`, beside
/// the new `client.exports.tag_definitions`, and added the `multiplayer_enabled` opt-in. Sunrise's
/// upgrade carries the moved value across, and each schema's parser reads only its own key.
const REPLACED_IN_SCHEMA_19: &[&str] = &["/client/dump_package_keys"];
const ADDED_IN_SCHEMA_19: &[&str] = &[
    "/client/exports/package_keys",
    "/client/exports/tag_definitions",
    "/multiplayer_enabled",
];

/// Which optional runtime controls the installed module and the settings schema read.
///
/// An undetected runtime leaves version-gated controls visible. Trace logging requires
/// evidence from the bundled account schema because the module version is ambiguous. Without a
/// settings schema the controls follow schema 18.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Capabilities {
    retired_sunrise_0_5: bool,
    before_sunrise_0_6: bool,
    trace_logging: bool,
    settings_schema: Option<u64>,
}

impl Capabilities {
    /// Selects version-gated controls for the installed launch copy. Dawn has a separate
    /// version line and does not read Sunrise's 0.6 controls.
    pub(crate) fn detect(version: Option<&str>, dawn: bool) -> Self {
        let sunrise_at_least = |minimum: &[u64]| {
            version
                .filter(|version| !dawn && normalize_sunrise_version(version).is_some())
                .map(|version| runtime_version_at_least(version, minimum))
        };
        Self {
            retired_sunrise_0_5: sunrise_at_least(&[0, 5]) == Some(true),
            before_sunrise_0_6: dawn || sunrise_at_least(&[0, 6]) == Some(false),
            trace_logging: false,
            settings_schema: None,
        }
    }

    /// Records the edited document's settings schema, which decides the shape of keys a schema
    /// moved.
    pub(crate) fn with_settings_schema(mut self, schema: Option<u64>) -> Self {
        self.settings_schema = schema;
        self
    }

    /// Records the bundled-schema capability separately from the ambiguous DLL version.
    pub(crate) fn with_trace_logging(mut self, supported: bool) -> Self {
        self.trace_logging = supported;
        self
    }

    pub(super) fn trace_logging(self) -> bool {
        self.trace_logging
    }

    /// True while the installed runtime reads the setting at `path`.
    pub(crate) fn supports(self, path: &str) -> bool {
        let retired = self.retired_sunrise_0_5 && RETIRED_IN_SUNRISE_0_5.contains(&path);
        let not_yet_added = self.before_sunrise_0_6 && ADDED_IN_SUNRISE_0_6.contains(&path);
        let schema_19 = self.settings_schema.is_some_and(|schema| schema >= 19);
        let moved = if schema_19 {
            REPLACED_IN_SCHEMA_19.contains(&path)
        } else {
            ADDED_IN_SCHEMA_19.contains(&path)
        };
        !retired && !not_yet_added && !moved
    }
}
