//! Authored socket roles and the lanes a grafted behavior pins its own plugs into.
use super::*;

/// Socket type of a weapon's intrinsic frame.
pub(crate) const INTRINSIC_SOCKET_TYPE: u16 = 176;
/// Socket type of a weapon's trait columns.
pub(crate) const TRAIT_SOCKET_TYPE: u16 = 92;

/// The role every lane actually has, which is not always the role the donor shipped.
///
/// An author can turn one of the donor's sockets into a trait column, and can append columns the
/// donor never had. The socket list draws both as trait sockets, so anything choosing a lane has
/// to read them the same way or it lands somewhere the author was never shown.
pub(crate) fn effective_socket_types(
    authored_roles: &[Option<u16>],
    donor_socket_types: &[u16],
) -> Vec<u16> {
    (0..donor_socket_types.len().max(authored_roles.len()))
        .map(|lane| {
            authored_roles
                .get(lane)
                .copied()
                .flatten()
                .or_else(|| donor_socket_types.get(lane).copied())
                .unwrap_or(u16::MAX)
        })
        .collect()
}

/// Every plug the chosen behaviors own, whether or not a pin was needed to place it.
///
/// A lane is only cleared of a perk no behavior wants any more, so this has to name the perks
/// that are still wanted even when they already sit where the author put them.
pub(crate) fn claimed_plugs<'a>(
    behaviors: impl IntoIterator<Item = &'a str>,
    skip_behavior_perks: bool,
) -> BTreeSet<u32> {
    if skip_behavior_perks {
        return BTreeSet::new();
    }
    behaviors
        .into_iter()
        .filter_map(behavior)
        .flat_map(|entry| [entry.intrinsic_plug, entry.trait_plug])
        .flatten()
        .collect()
}

/// The socket lanes each grafted behavior claims, and the plug it puts first in them.
///
/// The editor writes these as soon as a behavior is chosen and the build writes them again, so
/// both read the lanes from here rather than each deciding for itself which socket a behavior
/// takes. A behavior whose lane the weapon does not have is skipped.
///
/// `socket_types` is every lane's effective role, so a column the author turned into a trait
/// socket counts as one of them. `placed` is what each lane already holds: a perk the author put
/// in a socket of their own already satisfies the behavior, and pinning a second copy into the
/// donor's own trait lane would show the same perk twice and reshuffle a list they arranged.
pub(crate) fn socket_pins<'a>(
    behaviors: impl IntoIterator<Item = &'a str>,
    skip_behavior_perks: bool,
    socket_types: &[u16],
    placed: &[Vec<u32>],
) -> Vec<(usize, u32)> {
    if skip_behavior_perks {
        return Vec::new();
    }
    let lanes = |kind: u16| {
        socket_types
            .iter()
            .enumerate()
            .filter(move |(_, socket_type)| **socket_type == kind)
            .map(|(lane, _)| lane)
    };
    let holds = |lane: usize, plug: u32| {
        placed
            .get(lane)
            .is_some_and(|choices| choices.contains(&plug))
    };
    // Each behavior takes a fresh trait lane, so two of them do not land on one socket. A lane
    // already holding one of their perks counts as spoken for.
    let mut taken = BTreeSet::new();
    let mut pins = Vec::new();
    for entry in behaviors.into_iter().filter_map(behavior) {
        for (plug, kind) in [
            (entry.intrinsic_plug, INTRINSIC_SOCKET_TYPE),
            (entry.trait_plug, TRAIT_SOCKET_TYPE),
        ] {
            let Some(plug) = plug else { continue };
            if let Some(lane) = lanes(kind).find(|lane| holds(*lane, plug)) {
                taken.insert(lane);
                continue;
            }
            // The intrinsic column is single, so behaviors share it as they always have.
            let free = if kind == INTRINSIC_SOCKET_TYPE {
                lanes(kind).next()
            } else {
                lanes(kind).find(|lane| !taken.contains(lane))
            };
            let Some(lane) = free else { continue };
            taken.insert(lane);
            pins.push((lane, plug));
        }
    }
    pins
}

/// The role the recipe gives each lane, empty where it leaves the donor's own.
pub(crate) fn authored_socket_roles(
    overrides: &crate::item::WeaponCloneOverrides,
) -> Vec<Option<u16>> {
    overrides
        .socket_columns
        .iter()
        .map(|column| column.as_ref().and_then(|column| column.socket_type))
        .collect()
}

/// The plugs the recipe puts in each lane, empty where it inherits the donor's own.
pub(crate) fn authored_socket_choices(
    overrides: &crate::item::WeaponCloneOverrides,
) -> Vec<Vec<u32>> {
    overrides
        .socket_columns
        .iter()
        .map(|column| {
            column
                .as_ref()
                .map(|column| column.choices.clone())
                .unwrap_or_default()
        })
        .collect()
}

/// Pins each grafted behavior's own plugs into the weapon's intrinsic and trait sockets.
///
/// A graph moves the firing and projectile side, but several exotics keep half of the behavior in
/// their perk, so the plugs travel with the graft unless the author turned them off. The plug
/// leads the socket rather than emptying it, so an author's own choices stay behind it.
pub(crate) fn expand_socket_columns(
    overrides: &crate::item::WeaponCloneOverrides,
    socket_types: &[u16],
) -> crate::AuthoringResult<crate::item::WeaponCloneOverrides> {
    use crate::item::WeaponSocketColumnOverride;
    let pins = socket_pins(
        overrides.additional_behaviors.iter().map(String::as_str),
        overrides.skip_behavior_perks,
        &effective_socket_types(&authored_socket_roles(overrides), socket_types),
        &authored_socket_choices(overrides),
    );
    let mut expanded = overrides.clone();
    if !pins.is_empty() && expanded.socket_columns.is_empty() {
        expanded.socket_columns = vec![None; socket_types.len()];
    }
    if !pins.is_empty() && expanded.socket_columns.len() < socket_types.len() {
        return Err(invalid(format!(
            "The recipe lists {} socket columns but the base weapon has {} sockets.",
            expanded.socket_columns.len(),
            socket_types.len()
        )));
    }
    for (lane, plug) in pins {
        let column =
            expanded.socket_columns[lane].get_or_insert_with(|| WeaponSocketColumnOverride {
                choices: Vec::new(),
                socket_type: None,
                choice_weight_bits: Vec::new(),
                choice_conditions: Vec::new(),
                reusable_plug_set_index: None,
                randomized_plug_set_index: None,
                randomized_selection_program: Vec::new(),
            });
        column.choices.retain(|choice| *choice != plug);
        column.choices.insert(0, plug);
    }
    // A weapon has one frame. A borrowed frame replaces the host's rather than sitting ahead of
    // it, so the intrinsic lane holds the borrowed frames alone. A plug carrying a custom perk
    // the author wrote is their own frame, not the donor's, so it stays and its perk is carried
    // to wherever the trim leaves it: a variant addresses its plug by position, and a stale
    // position is refused by the compiler rather than silently landing on another plug.
    let claimed = claimed_plugs(
        overrides.additional_behaviors.iter().map(String::as_str),
        overrides.skip_behavior_perks,
    );
    let types = effective_socket_types(&authored_socket_roles(overrides), socket_types);
    for lane in 0..expanded.socket_columns.len() {
        if types.get(lane) != Some(&INTRINSIC_SOCKET_TYPE) {
            continue;
        }
        let authored = expanded
            .socket_plug_variants
            .iter()
            .filter(|variant| usize::from(variant.socket_index) == lane)
            .map(|variant| variant.source_plug_hash)
            .collect::<BTreeSet<_>>();
        let Some(column) = expanded.socket_columns[lane].as_mut() else {
            continue;
        };
        if !column.choices.iter().any(|choice| claimed.contains(choice)) {
            continue;
        }
        column
            .choices
            .retain(|choice| claimed.contains(choice) || authored.contains(choice));
        let choices = column.choices.clone();
        for variant in &mut expanded.socket_plug_variants {
            if usize::from(variant.socket_index) != lane {
                continue;
            }
            if let Some(index) = choices
                .iter()
                .position(|choice| *choice == variant.source_plug_hash)
                && let Ok(index) = u16::try_from(index)
            {
                variant.choice_index = index;
            }
        }
    }
    Ok(expanded)
}
