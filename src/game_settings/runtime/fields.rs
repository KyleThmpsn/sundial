//! Known current Sunrise runtime controls, including effective defaults for omitted fields.

use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Bool(bool),
    Text(&'static str, usize, usize),
    Ipv4(&'static str),
    UInt(u64, u64, u64),
    Choice(&'static [&'static str]),
}

#[derive(Clone, Copy)]
pub(super) struct Field {
    pub path: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub kind: Kind,
    pub help: &'static str,
}

impl Field {
    pub fn default_value(self) -> Value {
        match self.kind {
            Kind::Bool(value) => Value::Bool(value),
            Kind::Text(value, _, _) | Kind::Ipv4(value) => Value::from(value),
            Kind::UInt(value, _, _) => Value::from(value),
            Kind::Choice(values) => Value::from(values[0]),
        }
    }

    pub fn validate(self, value: &Value) -> Result<(), String> {
        let valid = match self.kind {
            Kind::Bool(_) => value.is_boolean(),
            Kind::Text(_, min, max) => value.as_str().is_some_and(|s| {
                (min..=max).contains(&s.len())
                    && s.bytes()
                        .all(|b| (32..=126).contains(&b) && b != b'\\' && b != b'"')
            }),
            Kind::Ipv4(_) => value
                .as_str()
                .is_some_and(|s| s.parse::<std::net::Ipv4Addr>().is_ok()),
            Kind::UInt(_, min, max) => value.as_u64().is_some_and(|n| (min..=max).contains(&n)),
            Kind::Choice(values) => value.as_str().is_some_and(|s| values.contains(&s)),
        };
        if valid {
            Ok(())
        } else {
            Err(format!("{} has an invalid value", self.path))
        }
    }

    pub fn account_owned(self) -> bool {
        self.path.starts_with("/state/account/")
    }
}

pub(super) const FIELDS: &[Field] = &[
    Field {
        path: "/client/socket_menu_routing",
        label: "Socket menu routing",
        group: "Presentation",
        kind: Kind::Bool(false),
        help: "Enables Sunrise's socket menu routing hook.",
    },
    Field {
        path: "/client/reveal_lore_books",
        label: "Reveal lore books",
        group: "Presentation",
        kind: Kind::Bool(true),
        help: "Reveals lore books in the client. Saved lore unlock flags and objective values remain separate.",
    },
    Field {
        path: "/state/investment/complete_exotic_catalysts",
        label: "Complete Exotic Catalysts",
        group: "Profile & Catalysts",
        kind: Kind::Bool(true),
        help: "Completes released exotic weapon catalysts while Sunrise resolves item state; does not rewrite socket plugs.",
    },
    Field {
        path: "/state/account/profile_setup_completed",
        label: "Profile Setup Completed",
        group: "Profile & Catalysts",
        kind: Kind::Bool(false),
        help: "Saved completion state of the active JSON account. Turn off to revisit setup.",
    },
    Field {
        path: "/client/custom_bootflow_textures",
        label: "Custom bootflow textures",
        group: "Presentation",
        kind: Kind::Bool(true),
        help: "Uses Sunrise's installed bootflow textures. Does not install or modify textures.",
    },
    Field {
        path: "/client/ui/enabled",
        label: "In-game Sunrise menu",
        group: "Presentation",
        kind: Kind::Bool(true),
        help: "Enables Sunrise's in-game menu.",
    },
    Field {
        path: "/client/ui/toggle_key",
        label: "Menu toggle key",
        group: "Presentation",
        kind: Kind::Choice(&[
            "insert", "home", "end", "delete", "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8",
            "f9", "f10", "f11", "f12",
        ]),
        help: "Key used to open the in-game menu.",
    },
    Field {
        path: "/client/skip_orbit_cinematic_wait",
        label: "Skip orbit cinematic wait",
        group: "Presentation",
        kind: Kind::Bool(false),
        help: "Optional cinematic wait override.",
    },
    Field {
        path: "/server/gameplay/hold_launch_cinematic",
        label: "Hold launch cinematic",
        group: "Presentation",
        kind: Kind::Bool(false),
        help: "Keeps launch synchronization open through loading.",
    },
    Field {
        path: "/server/activation/mission_scripting",
        label: "Mission scripting",
        group: "Activities & Scripting",
        kind: Kind::Bool(false),
        help: "Runs Sunrise's mission scripting runtime. When this setting is omitted, scripting defaults to off.",
    },
    Field {
        path: "/server/activation/activity_public_membership",
        label: "Public activity membership",
        group: "Activities & Scripting",
        kind: Kind::Bool(true),
        help: "Publishes public activity membership.",
    },
    Field {
        path: "/server/activation/prevent_ownerless_channel_close",
        label: "Keep ownerless channels open",
        group: "Activities & Scripting",
        kind: Kind::Bool(false),
        help: "Diagnostic override for ownerless activity channels.",
    },
    Field {
        path: "/state/activity/roster_key_from_identity",
        label: "Roster key from identity",
        group: "Activities & Scripting",
        kind: Kind::Bool(false),
        help: "Uses the membership identity for the roster player key.",
    },
    Field {
        path: "/state/activity/roster_key_on_all_slots",
        label: "Roster key on all slots",
        group: "Activities & Scripting",
        kind: Kind::Bool(false),
        help: "Publishes participation on every relevant slot.",
    },
    Field {
        path: "/core/activity_sdk_generation/lua_declarations",
        label: "Generate Lua declarations",
        group: "Activities & Scripting",
        kind: Kind::Bool(true),
        help: "Writes declarations for Sunrise's Lua API when SDK generation runs. Native Bungie Script package records are not Lua bytecode.",
    },
    Field {
        path: "/client/region_private",
        label: "Private region",
        group: "Advanced Runtime",
        kind: Kind::Bool(false),
        help: "Uses private region routing.",
    },
    Field {
        path: "/client/pin_replicated_record",
        label: "Pin replicated record",
        group: "Advanced Runtime",
        kind: Kind::Bool(true),
        help: "Keeps the replicated account record pinned.",
    },
];
