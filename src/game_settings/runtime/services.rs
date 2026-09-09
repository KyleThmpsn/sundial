//! Sunrise logging, networking, activation, and account configuration.
use super::{
    fields::{Field, Kind},
    optional_value,
};
use eframe::egui;
use serde_json::Value;

pub(super) const FIELDS: &[Field] = &[
    Field {
        path: "/core/logging/debugger_sink",
        label: "Debugger Output",
        group: "Logging",
        kind: Kind::Bool(true),
        help: "Send Sunrise logs to the debugger.",
    },
    Field {
        path: "/core/logging/file_sink",
        label: "File Output",
        group: "Logging",
        kind: Kind::Bool(false),
        help: "Write Sunrise logs to disk.",
    },
    Field {
        path: "/core/logging/levels/core",
        label: "Core",
        group: "Logging",
        kind: Kind::Choice(&["warn", "error", "info", "debug"]),
        help: "Minimum severity for this Sunrise channel.",
    },
    Field {
        path: "/core/logging/levels/client",
        label: "Client",
        group: "Logging",
        kind: Kind::Choice(&["warn", "error", "info", "debug"]),
        help: "Minimum severity for this Sunrise channel.",
    },
    Field {
        path: "/core/logging/levels/state",
        label: "State",
        group: "Logging",
        kind: Kind::Choice(&["warn", "error", "info", "debug"]),
        help: "Minimum severity for this Sunrise channel.",
    },
    Field {
        path: "/core/logging/levels/server",
        label: "Server",
        group: "Logging",
        kind: Kind::Choice(&["warn", "error", "info", "debug"]),
        help: "Minimum severity for this Sunrise channel.",
    },
    Field {
        path: "/core/logging/levels/middleware",
        label: "Middleware",
        group: "Logging",
        kind: Kind::Choice(&["warn", "error", "info", "debug"]),
        help: "Minimum severity for this Sunrise channel.",
    },
    Field {
        path: "/client/external_server/enabled",
        label: "Use External Server",
        group: "External Server",
        kind: Kind::Bool(false),
        help: "Connect to the configured external server.",
    },
    Field {
        path: "/client/external_server/host",
        label: "Host IPv4 Address",
        group: "External Server",
        kind: Kind::Ipv4("127.0.0.1"),
        help: "Numeric IPv4 address used for server connections.",
    },
    Field {
        path: "/client/external_server/config_url",
        label: "Configuration URL",
        group: "External Server",
        kind: Kind::Text("https://127.0.0.1/config/", 1, 127),
        help: "Configuration route served by the external server. Use 1–127 printable ASCII characters.",
    },
    Field {
        path: "/client/external_server/config_guid",
        label: "Configuration GUID",
        group: "External Server",
        kind: Kind::Text("d2legacy-0000-0000-0000-000000000001", 36, 36),
        help: "Exactly 36 printable ASCII characters matching the external manifest.",
    },
    Field {
        path: "/server/bap_port",
        label: "BAP Port",
        group: "Server Networking",
        kind: Kind::UInt(30974, 1, 65535),
        help: "BAP service port.",
    },
    Field {
        path: "/server/gameplay/topology",
        label: "Gameplay Topology",
        group: "Server Networking",
        kind: Kind::Choice(&["embedded", "disabled", "external"]),
        help: "Embedded hosts gameplay here. External uses another process. Disabled publishes no endpoint.",
    },
    Field {
        path: "/server/gameplay/bind_address",
        label: "Bind Address",
        group: "Server Networking",
        kind: Kind::Ipv4("127.0.0.1"),
        help: "Numeric IPv4 address for the gameplay endpoint.",
    },
    Field {
        path: "/server/gameplay/advertised_address",
        label: "Advertised Address",
        group: "Server Networking",
        kind: Kind::Ipv4("127.0.0.1"),
        help: "Numeric IPv4 address for the gameplay endpoint.",
    },
    Field {
        path: "/server/gameplay/transport_address",
        label: "Transport Address",
        group: "Server Networking",
        kind: Kind::Ipv4("127.0.0.1"),
        help: "Numeric IPv4 address for the gameplay endpoint.",
    },
    Field {
        path: "/server/gameplay/port",
        label: "Gameplay Port",
        group: "Server Networking",
        kind: Kind::UInt(30976, 0, 65535),
        help: "An even starting port with room for 16 ports, stepping by two. The pool must avoid port 3074.",
    },
    Field {
        path: "/server/gameplay/server_reserve_count",
        label: "Server Reserved Slots",
        group: "Server Networking",
        kind: Kind::UInt(256, 0, 65535),
        help: "Active gameplay requires 8–4096 reserved entity slots.",
    },
    Field {
        path: "/server/gameplay/client_join_grant_count",
        label: "Client Join Slots",
        group: "Server Networking",
        kind: Kind::UInt(8192, 0, 65535),
        help: "Active gameplay requires 400–8192 requested slots. Sunrise caps the grant to the free slots.",
    },
    Field {
        path: "/server/activation/default_client_activation",
        label: "Default Client Activation",
        group: "Server Activation",
        kind: Kind::Bool(true),
        help: "Use Sunrise default client activation.",
    },
];

pub(super) fn draw(ui: &mut egui::Ui, document: &mut Value, json_account: bool) -> bool {
    if !super::available(document) {
        return false;
    }
    let mut changed = false;
    for group in [
        "Logging",
        "External Server",
        "Server Networking",
        "Server Activation",
    ] {
        egui::CollapsingHeader::new(group).show(ui, |ui| {
            for &field in FIELDS.iter().filter(|field| field.group == group) {
                ui.push_id(field.path, |ui| {
                    changed |= super::page::draw_field(ui, document, field, json_account);
                });
            }
        });
    }
    if json_account {
        changed |= super::entitlements::draw(ui, document);
    }
    changed |= super::character_page::draw(ui, document, json_account);
    if let Err(error) = validate(document, json_account) {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    changed
}

pub(super) fn validate(document: &Value, json_account: bool) -> Result<(), String> {
    let topology = optional_value(document, "/server/gameplay/topology")?
        .and_then(Value::as_str)
        .unwrap_or("embedded");
    if topology != "disabled" {
        let number = |key: &str, default| -> Result<u64, String> {
            Ok(
                optional_value(document, &format!("/server/gameplay/{key}"))?
                    .and_then(Value::as_u64)
                    .unwrap_or(default),
            )
        };
        let port = number("port", 30976)?;
        if port == 0 || port % 2 != 0 || port > 65504 || (port <= 3074 && port + 30 >= 3074) {
            return Err("Gameplay port must be even, between 2 and 65504, with its 16-port pool clear of discovery port 3074".into());
        }
        if !(8..=4096).contains(&number("server_reserve_count", 256)?) {
            return Err("Server reserved slots must be 8–4096".into());
        }
        if !(400..=8192).contains(&number("client_join_grant_count", 8192)?) {
            return Err("Client join slots must be 400–8192".into());
        }
    }
    if json_account {
        super::entitlements::validate(document)?;
        super::character_page::validate(document)?;
    }
    Ok(())
}
