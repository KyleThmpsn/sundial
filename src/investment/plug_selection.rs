use crate::catalog::{Catalog, ItemDef};
use eframe::egui;
use serde::{Deserialize, Serialize};

const SOCKET_AND_GEAR_TYPE_WARNING: &str = "Shows only plugs that match the socket and gear type. A few plugs may not work, or cause the item to behave unexpectedly.";
const MATCHING_SOCKET_WARNING: &str = "Use caution: these plugs match the socket type but are not known to be supported by this item. Incompatible choices may prevent the item or loadout from working correctly.";
const GEAR_TYPE_WARNING: &str = "High risk: this exposes plugs used anywhere on the same broad gear type, not just this socket. Incompatible choices may prevent the item or loadout from working correctly.";
const ANY_PLUG_WARNING: &str = "High risk: this exposes every discovered plug for every socket. Incompatible choices may prevent Sunrise/Destiny 2 from loading or cause instability.";

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlugSelectionMode {
    Supported,
    #[default]
    SocketAndGearType,
    MatchingSocketType,
    GearType,
    AnyPlug,
}

impl PlugSelectionMode {
    pub const ALL: [Self; 5] = [
        Self::Supported,
        Self::SocketAndGearType,
        Self::MatchingSocketType,
        Self::GearType,
        Self::AnyPlug,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Supported => "Compatible",
            Self::SocketAndGearType => "Socket + Gear Type",
            Self::MatchingSocketType => "Socket Type",
            Self::GearType => "Gear Type",
            Self::AnyPlug => "All",
        }
    }
}

/// The shared candidate policy for an existing native socket. Callers may narrow
/// these candidates for their task or retain the equipped plug as a comparison;
/// neither exception should broaden the user's selected compatibility scope.
pub(crate) fn candidates_for_socket(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    mode: PlugSelectionMode,
) -> Vec<u64> {
    candidates_for_socket_type(catalog, item, socket_index, None, mode).into_owned()
}

/// An override only retains native support when it names the original socket type.
/// Broader modes use the requested type without claiming native compatibility.
pub(crate) fn candidates_for_socket_type<'a>(
    catalog: &'a Catalog,
    item: &'a ItemDef,
    socket_index: usize,
    socket_type_override: Option<u16>,
    mode: PlugSelectionMode,
) -> std::borrow::Cow<'a, [u64]> {
    use std::borrow::Cow;
    let socket = item.sockets.get(socket_index);
    let Some(socket_type) =
        socket_type_override.or_else(|| socket.map(|socket| socket.socket_type))
    else {
        return Cow::Borrowed(&[]);
    };
    match mode {
        PlugSelectionMode::Supported => Cow::Borrowed(
            socket
                .filter(|socket| socket.socket_type == socket_type)
                .map_or(&[][..], |socket| catalog.socket_options(socket)),
        ),
        PlugSelectionMode::SocketAndGearType => {
            Cow::Borrowed(catalog.socket_and_gear_type_options_for_type(item, socket_type))
        }
        PlugSelectionMode::MatchingSocketType => {
            Cow::Borrowed(catalog.socket_type_options(socket_type))
        }
        PlugSelectionMode::GearType => Cow::Owned(if socket_type_override.is_some() {
            catalog.gear_type_options_for_type(item, socket_type)
        } else {
            catalog.gear_type_options(item, socket_index)
        }),
        PlugSelectionMode::AnyPlug => Cow::Borrowed(catalog.all_plug_options()),
    }
}

pub(crate) fn draw_plug_selection_warning(ui: &mut egui::Ui, mode: PlugSelectionMode) {
    match mode {
        PlugSelectionMode::Supported => {}
        PlugSelectionMode::SocketAndGearType => {
            ui.colored_label(ui.visuals().warn_fg_color, SOCKET_AND_GEAR_TYPE_WARNING);
        }
        PlugSelectionMode::MatchingSocketType => {
            ui.colored_label(ui.visuals().warn_fg_color, MATCHING_SOCKET_WARNING);
        }
        PlugSelectionMode::GearType => {
            ui.colored_label(ui.visuals().error_fg_color, GEAR_TYPE_WARNING);
        }
        PlugSelectionMode::AnyPlug => {
            ui.colored_label(ui.visuals().error_fg_color, ANY_PLUG_WARNING);
        }
    }
}
