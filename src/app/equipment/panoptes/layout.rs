//! Panoptes' semantic socket ordering, derived from the installed packages.

use crate::catalog::{Catalog, ItemDef};

pub(super) const MAX_ROW_WIDTH: usize = 5;
const MAX_PINS: usize = 3;

const INTRINSIC_SOCKET_TYPES: &[u16] = &[176, 14, 377, 677];
const ARMOR_INTRINSIC_SOCKET_TYPES: &[u16] = &[14, 377, 547];
const ARMOR_GLOW_SOCKET_TYPES: &[u16] = &[465, 466, 467, 468, 469, 605, 606, 607, 608, 609];
const DAMAGE_MOD_SOCKET_TYPES: &[u16] = &[68, 69];
const MOD_SOCKET_TYPES: &[u16] = &[
    643, 644, 645, 646, 647, 648, 649, 724, 734, 735, 687, 688, 689, 690, 691, 692, 694, 695, 696,
    697, 698, 699, 700, 701, 702, 703,
];
const STAT_SOCKET_TYPES: &[u16] = &[676, 760, 761, 762, 763];
const ENERGY_SOCKET_TYPES: &[u16] = &[678, 679];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RowGroup {
    Perk,
    Mod,
    Stat,
    Cosmetic,
}

impl RowGroup {
    fn weight(self, cosmetic: Option<CosmeticKind>) -> u16 {
        match self {
            Self::Perk => 10,
            Self::Mod => 20,
            Self::Stat => 30,
            Self::Cosmetic => 50 + cosmetic.map_or(0, CosmeticKind::order),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CosmeticKind {
    Tracker,
    Ornament,
    Radiance,
    Projection,
    Other,
    Shader,
}

impl CosmeticKind {
    const fn order(self) -> u16 {
        self as u16
    }
}

pub(super) struct SocketLine {
    pub(super) pinned: Option<usize>,
    pub(super) row: Vec<usize>,
}

pub(super) fn socket_lines(
    catalog: &Catalog,
    item: &ItemDef,
    slot: &str,
    socket_count: usize,
    grouped: bool,
    is_visible: impl Fn(usize) -> bool,
) -> Vec<SocketLine> {
    if !grouped {
        return combine_lines(
            Vec::new(),
            wrapped_rows((0..socket_count).filter(|socket| is_visible(*socket))),
        );
    }

    let mut pinned = pinned_sockets(catalog, item, slot, socket_count, &is_visible);
    pinned.sort_by_key(|&socket| (pin_weight(catalog, item, slot, socket), socket));
    pinned.dedup();

    let mut loose = (0..socket_count)
        .filter(|socket| is_visible(*socket) && !pinned.contains(socket))
        .map(|socket| {
            let group = socket_group(catalog, item, slot, socket);
            (
                group.weight(cosmetic_kind(catalog, item, socket)),
                group,
                socket,
            )
        })
        .collect::<Vec<_>>();
    loose.sort_unstable();

    let mut rows = Vec::<Vec<usize>>::new();
    let mut open = None;
    for (_, group, socket) in loose {
        let full = rows.last().is_some_and(|row| row.len() >= MAX_ROW_WIDTH);
        if open != Some(group) || full {
            rows.push(Vec::new());
        }
        rows.last_mut()
            .expect("a semantic socket row was just opened")
            .push(socket);
        open = Some(group);
    }
    combine_lines(pinned, rows)
}

fn pinned_sockets(
    catalog: &Catalog,
    item: &ItemDef,
    slot: &str,
    socket_count: usize,
    is_visible: &impl Fn(usize) -> bool,
) -> Vec<usize> {
    let find = |test: &dyn Fn(usize) -> bool| {
        (0..socket_count).find(|socket| is_visible(*socket) && test(*socket))
    };
    let mut pinned = Vec::new();
    if crate::app::WEAPON_SLOTS.contains(&slot) {
        pinned.extend(find(&|socket| {
            socket_type_is(item, socket, INTRINSIC_SOCKET_TYPES)
        }));
        pinned.extend(find(&|socket| is_masterwork_socket(catalog, item, socket)));
        pinned.extend(find(&|socket| {
            socket_type_is(item, socket, DAMAGE_MOD_SOCKET_TYPES)
        }));
    } else if crate::app::ARMOR_SLOTS.contains(&slot) {
        if item.type_name == "Mask" {
            pinned.extend(find(&|_| true));
        } else if item.sockets.iter().any(|socket| socket.socket_type == 643) {
            pinned.extend(find(&|socket| {
                socket_type_is(item, socket, ENERGY_SOCKET_TYPES)
            }));
        } else {
            pinned.extend(find(&|socket| {
                socket_type_is(item, socket, ARMOR_INTRINSIC_SOCKET_TYPES)
            }));
            pinned.extend(find(&|socket| is_masterwork_socket(catalog, item, socket)));
        }
        pinned.extend(find(&|socket| {
            socket_type_is(item, socket, ARMOR_GLOW_SOCKET_TYPES)
        }));
    } else {
        let pin_type = match slot {
            "vehicle" => Some(61),
            "ship" => Some(58),
            "ghost" => Some(519),
            "clan_banner" => Some(746),
            _ => None,
        };
        if let Some(socket_type) = pin_type {
            pinned.extend(find(&|socket| socket_type_is(item, socket, &[socket_type])));
        }
    }
    pinned
}

fn pin_weight(catalog: &Catalog, item: &ItemDef, slot: &str, socket: usize) -> u16 {
    if socket_type_is(item, socket, DAMAGE_MOD_SOCKET_TYPES) {
        u16::MAX
    } else {
        let group = socket_group(catalog, item, slot, socket);
        group.weight(cosmetic_kind(catalog, item, socket))
    }
}

fn socket_group(catalog: &Catalog, item: &ItemDef, slot: &str, socket: usize) -> RowGroup {
    if cosmetic_kind(catalog, item, socket).is_some() {
        RowGroup::Cosmetic
    } else if socket_type_is(item, socket, STAT_SOCKET_TYPES) {
        RowGroup::Stat
    } else if is_secondary_socket(catalog, item, socket)
        || crate::app::ARMOR_SLOTS.contains(&slot)
        || socket_type_is(item, socket, MOD_SOCKET_TYPES)
    {
        // Panoptes main intentionally places secondary sockets with mods. Its
        // Experimental branch used a separate spare row, creating the extra
        // line that made Sundial's first port visibly diverge.
        RowGroup::Mod
    } else {
        RowGroup::Perk
    }
}

fn cosmetic_kind(catalog: &Catalog, item: &ItemDef, socket: usize) -> Option<CosmeticKind> {
    let socket = item.sockets.get(socket)?;
    match socket.socket_type {
        535 => return Some(CosmeticKind::Radiance),
        519 => return Some(CosmeticKind::Projection),
        746 => return Some(CosmeticKind::Other),
        _ => {}
    }
    catalog.socket_options(socket).iter().find_map(|hash| {
        let name = catalog.names.get(hash)?.as_str();
        match name {
            "Default Shader" => Some(CosmeticKind::Shader),
            "Default Ornament" => Some(CosmeticKind::Ornament),
            "Tracker Disabled" => Some(CosmeticKind::Tracker),
            _ if name.contains("Kill Tracker") => Some(CosmeticKind::Tracker),
            _ => None,
        }
    })
}

fn is_secondary_socket(catalog: &Catalog, item: &ItemDef, socket: usize) -> bool {
    let Some(socket) = item.sockets.get(socket) else {
        return false;
    };
    DAMAGE_MOD_SOCKET_TYPES.contains(&socket.socket_type)
        || catalog.socket_options(socket).is_empty()
}

pub(super) fn is_mod_socket(item: &ItemDef, socket: usize) -> bool {
    item.sockets.get(socket).is_some_and(|socket| {
        MOD_SOCKET_TYPES.contains(&socket.socket_type)
            || DAMAGE_MOD_SOCKET_TYPES.contains(&socket.socket_type)
    })
}

fn is_masterwork_socket(catalog: &Catalog, item: &ItemDef, socket: usize) -> bool {
    item.sockets.get(socket).is_some_and(|socket| {
        catalog.socket_options(socket).iter().any(|hash| {
            catalog.names.get(hash).is_some_and(|name| {
                name.contains("Masterwork")
                    || name == "Empty Catalyst Socket"
                    || name.ends_with(" Catalyst")
            })
        })
    })
}

fn socket_type_is(item: &ItemDef, socket: usize, types: &[u16]) -> bool {
    item.sockets
        .get(socket)
        .is_some_and(|socket| types.contains(&socket.socket_type))
}

fn combine_lines(pinned: Vec<usize>, rows: Vec<Vec<usize>>) -> Vec<SocketLine> {
    let pinned = pinned.into_iter().take(MAX_PINS).collect::<Vec<_>>();
    (0..pinned.len().max(rows.len()))
        .map(|line| SocketLine {
            pinned: pinned.get(line).copied(),
            row: rows.get(line).cloned().unwrap_or_default(),
        })
        .collect()
}

fn wrapped_rows(sockets: impl Iterator<Item = usize>) -> Vec<Vec<usize>> {
    sockets
        .collect::<Vec<_>>()
        .chunks(MAX_ROW_WIDTH)
        .map(<[_]>::to_vec)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_rows_keep_native_order_and_wrap_at_five() {
        assert_eq!(wrapped_rows(0..7), vec![vec![0, 1, 2, 3, 4], vec![5, 6]]);
    }

    #[test]
    fn pin_column_is_independently_capped() {
        let lines = combine_lines(vec![0, 1, 2, 3], vec![vec![4], vec![5], vec![6], vec![7]]);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[2].pinned, Some(2));
        assert_eq!(lines[3].pinned, None);
    }
}
