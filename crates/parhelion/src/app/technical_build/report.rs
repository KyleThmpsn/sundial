//! Technical report construction from saved recipes and staged native readback.

use super::*;
use std::fmt::Write as _;

/// Renders the report. Pure, so its content is tested without drawing a frame. Without a staged
/// build it reports the identities the next build will assign. With a donor from the loaded
/// catalog, inherited fields show the donor's value instead of "donor".
pub(crate) fn technical_build_report(
    build: Option<&BuildReport>,
    recipe: &WeaponRecipe,
    donor: Option<&WeaponDonor>,
    art: &str,
    registry: &str,
) -> String {
    let mut out = String::new();
    match build {
        Some(build) => {
            append_build(&mut out, build);
            let current = build
                .weapons
                .iter()
                .find(|weapon| weapon.namespace == recipe.namespace);
            let matches = current.is_some_and(|weapon| {
                crate::manifest::recipe_selection_fingerprint(std::slice::from_ref(recipe))
                    .is_ok_and(|fingerprint| fingerprint == weapon.recipe_fingerprint)
            });
            let _ = writeln!(
                out,
                "\n{}",
                match current {
                    None => "Current recipe is not in this staged build.",
                    Some(_) if matches => "Current recipe matches this staged build.",
                    Some(_) =>
                        "Current recipe differs from this staged build. Build again to check these edits.",
                }
            );
            // Sunrise groups subclasses in item order, so each one's group depends on the others.
            let mut subclasses = build
                .weapons
                .iter()
                .filter(|weapon| weapon.kind == ItemKind::Subclass)
                .collect::<Vec<_>>();
            subclasses.sort_by_key(|weapon| weapon.item_index);
            for (index, weapon) in build.weapons.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "\nStaged {} {}/{}  {}",
                    weapon.kind.label(),
                    index + 1,
                    build.weapons.len(),
                    weapon.name
                );
                append_weapon(&mut out, weapon, &subclasses);
            }
        }
        None => append_planned(&mut out, recipe),
    }
    if build.is_some() {
        let _ = writeln!(out, "\nCurrent Recipe");
    }
    let weapon = recipe.kind.is_weapon();
    append_donors(&mut out, recipe, donor);
    append_item(&mut out, recipe, donor);
    append_install(&mut out, recipe);
    append_kind(&mut out, recipe, donor);
    // Other kinds show stats and perks only where they have some.
    let stats = donor.is_some_and(|donor| !donor.investment_stats.is_empty())
        || !recipe.overrides.investment_stats.is_empty()
        || !recipe.overrides.removed_investment_stats.is_empty();
    if weapon || stats {
        append_stats(&mut out, recipe, donor);
    }
    let perks = donor.is_some_and(|donor| {
        !donor.base_sandbox_perks.is_empty() || !donor.trait_indices.is_empty()
    }) || recipe.overrides.base_sandbox_perks.is_some()
        || recipe.overrides.trait_indices.is_some();
    if weapon || perks {
        append_perks(&mut out, recipe, donor);
    }
    append_text(&mut out, recipe);
    append_appearance(&mut out, recipe, donor);
    out.push_str(art);
    if weapon {
        append_recipe(&mut out, recipe);
    }
    let sockets = donor.is_some_and(|donor| !donor.sockets.is_empty())
        || !recipe.overrides.socket_columns.is_empty()
        || !recipe.overrides.socket_plug_variants.is_empty();
    if weapon || sockets {
        append_sockets(&mut out, recipe, donor);
    }
    if weapon {
        append_runtime(&mut out, recipe);
        out.push_str(registry);
    }
    append_document(&mut out, recipe);
    out
}

/// Whether the kind wears gear art, which carries art arrangements, dye rows and markers.
pub(super) const fn wears_gear_art(kind: ItemKind) -> bool {
    !matches!(
        kind,
        ItemKind::Subclass | ItemKind::Emblem | ItemKind::Shader | ItemKind::Mod
    )
}

pub(super) fn hex(value: u32) -> String {
    format!("0x{value:08X}")
}

pub(in super::super) fn field(out: &mut String, name: &str, value: impl std::fmt::Display) {
    let _ = writeln!(out, "  {name:<26}{value}");
}

/// Compact JSON for a value whose type has no display of its own. Everything in a recipe
/// serializes, so this never leaves a field out.
pub(super) fn json(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| format!("<unserializable: {error}>"))
}

/// Strings longer than this are summarized by length and digest.
const LONG_STRING_BYTES: usize = 96;

/// Compact JSON with every long string summarized as the recipe document summarizes it, so an
/// embedded image stays one short line.
pub(super) fn json_summary(value: &impl serde::Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(mut value) => {
            summarize_long_strings(&mut value);
            value.to_string()
        }
        Err(error) => format!("<unserializable: {error}>"),
    }
}

pub(super) fn summarize_long_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) if text.len() > LONG_STRING_BYTES => {
            let summary = long_string_summary(text);
            *text = summary;
        }
        serde_json::Value::Array(items) => {
            for item in items {
                summarize_long_strings(item);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                summarize_long_strings(item);
            }
        }
        _ => {}
    }
}

pub(super) fn long_string_summary(text: &str) -> String {
    let prefix = text
        .char_indices()
        .nth(32)
        .map_or(text.len(), |(index, _)| index);
    format!(
        "<{} bytes, sha256 {}> {}…",
        text.len(),
        sha256_prefix(text.as_bytes()),
        &text[..prefix]
    )
}

pub(super) fn list<T: std::fmt::Display>(values: impl IntoIterator<Item = T>) -> String {
    let joined = values
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        "[]".to_owned()
    } else {
        format!("[{joined}]")
    }
}

pub(super) fn hex_list(values: impl IntoIterator<Item = u32>) -> String {
    list(values.into_iter().map(hex))
}

/// One resolved field: the recipe's override when it has one, else the donor's value, else a
/// note that the catalog is not loaded. The origin is always named.
pub(super) fn resolved<T: std::fmt::Display>(
    out: &mut String,
    name: &str,
    override_value: Option<T>,
    donor_value: Option<Option<T>>,
) {
    let value = match (override_value, donor_value) {
        (Some(value), _) => format!("{value}  (recipe)"),
        (None, Some(Some(value))) => format!("{value}  (donor)"),
        (None, Some(None)) => "none  (donor)".to_owned(),
        (None, None) => "inherits donor  (catalog not loaded)".to_owned(),
    };
    field(out, name, value);
}

pub(super) fn append_build(out: &mut String, build: &BuildReport) {
    let _ = writeln!(out, "BUILD");
    let _ = writeln!(
        out,
        "Package checks do not establish firing cadence or perk behavior."
    );
    field(out, "selection fingerprint", &build.selection_fingerprint);
    field(out, "run directory", build.run_directory.display());
    field(out, "manifest", build.manifest_path.display());
    field(out, "weapons", build.weapons.len());
    field(out, "artifacts", build.artifacts.len());
    if !build.staged_recipe_paths.is_empty() {
        let _ = writeln!(
            out,
            "\nSTAGED RECIPES ({})",
            build.staged_recipe_paths.len()
        );
        for path in &build.staged_recipe_paths {
            let _ = writeln!(out, "  {}", path.display());
        }
    }
    if !build.artifacts.is_empty() {
        let _ = writeln!(out, "\nARTIFACTS ({})", build.artifacts.len());
        for artifact in &build.artifacts {
            let _ = writeln!(
                out,
                "  {:<40}{:>12} bytes  {}",
                artifact.file_name, artifact.byte_length, artifact.sha256
            );
        }
    }
}

pub(super) fn append_weapon(
    out: &mut String,
    weapon: &WeaponBuildReport,
    subclasses: &[&WeaponBuildReport],
) {
    field(out, "namespace", &weapon.namespace);
    field(out, "item hash", hex(weapon.item_hash));
    field(out, "item definition", hex(weapon.item_definition_hash));
    field(out, "item string", hex(weapon.item_string_hash));
    field(out, "icon definition", hex(weapon.icon_definition_hash));
    field(out, "item index", weapon.item_index);
    if let Some(details) = &weapon.details {
        let _ = writeln!(out, "\n  Build Sources");
        field(out, "Runtime Pattern", opt(details.runtime_source));
        if let Some(donor) = details.rig_donor {
            field(out, "Rig Moved From", hex(donor));
        }
        if let Some(pattern) = details.pinned_appearance {
            field(out, "Pinned Appearance Pattern", pattern);
        }
        if let Some(pattern) = details.animation_donor {
            field(out, "Animation Pattern", pattern);
        }
        use crate::item::WeaponDamageCarrier;
        let carrier = match details.damage_carrier {
            WeaponDamageCarrier::Empty => "No elemental carrier".into(),
            WeaponDamageCarrier::Fixed { damage_type, .. } => {
                format!("{damage_type:?} on the weapon")
            }
            WeaponDamageCarrier::PlugDriven { damage_type, lane } => {
                format!("{damage_type:?} from Socket {} by default", lane + 1)
            }
        };
        field(out, "Damage Carrier", carrier);
    }
    if let Some(collection) = &weapon.collection {
        field(out, "collectible hash", hex(collection.collectible_hash));
        field(out, "collectible index", collection.collectible_index);
        field(out, "unlock hash", hex(collection.unlock_hash));
        field(out, "unlock definition", collection.unlock_definition_index);
        field(
            out,
            "unlock bank / slot",
            format!("{} / {}", collection.unlock_bank, collection.unlock_slot),
        );
    }
    if let Some(subclass) = &weapon.subclass {
        append_staged_subclass(out, weapon, subclass, subclasses);
    }
    if weapon.custom_plugs.is_empty() {
        return;
    }
    let _ = writeln!(out, "  custom plugs ({})", weapon.custom_plugs.len());
    for plug in &weapon.custom_plugs {
        let _ = writeln!(
            out,
            "    socket {} choice {}  {}",
            plug.socket_index,
            plug.choice_index,
            plug.name.as_deref().unwrap_or("<unnamed>")
        );
        let _ = writeln!(
            out,
            "      item {} index {}  definition {}  string {}",
            hex(plug.item_hash),
            plug.item_index,
            hex(plug.definition_hash),
            hex(plug.string_hash)
        );
        for (name, value) in [
            ("icon definition", plug.icon_definition_hash),
            ("name", plug.name_hash),
            ("description", plug.description_hash),
        ] {
            if let Some(value) = value {
                let _ = writeln!(out, "      {name:<16}{}", hex(value));
            }
        }
        if !plug.offered_sets.is_empty() {
            let sets = plug
                .offered_sets
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "      {:<16}plug sets {sets}", "offered in");
        }
        for perk in &plug.perks {
            let _ = writeln!(
                out,
                "      perk source {}  hash {}  runtime key {}",
                perk.source_perk_index,
                hex(perk.perk_hash),
                hex(perk.runtime_key)
            );
        }
    }
}

/// The lists Sunrise keeps Super lane selection state for, which the build refuses to exceed.
const SUPER_LANE_LISTS: usize = 32;

/// What the build wrote for a subclass: the class its equip condition names, its list and
/// records, who the install gives it to, the group of three Sunrise gives with it and the Super
/// lane lists the build uses.
pub(super) fn append_staged_subclass(
    out: &mut String,
    weapon: &WeaponBuildReport,
    subclass: &crate::item::SubclassBuildDetails,
    subclasses: &[&WeaponBuildReport],
) {
    let class = |class: u8| crate::app::gear_view::class_label(class).unwrap_or("Unknown");
    let _ = writeln!(out, "\n  Subclass");
    field(out, "Base Class", subclass.base_class.map_or("none", class));
    let every_class = subclass.class_condition.is_none();
    field(
        out,
        "Class Condition",
        match subclass.class_condition {
            None => "removed  (Any Class)".to_owned(),
            Some(written) if Some(written) == subclass.base_class => {
                format!("{}  (base)", class(written))
            }
            Some(written) => format!("{}  (recipe)", class(written)),
        },
    );
    let receives = if every_class {
        subclass.base_class
    } else {
        subclass.class_condition
    };
    field(
        out,
        "Goes To",
        install_text(ItemKind::Subclass, receives, every_class),
    );
    field(out, "List Index", subclass.list_index);
    field(
        out,
        "Own List",
        subclass.own_list.map_or_else(
            || "none  (base's list)".to_owned(),
            |(list, display)| format!("list {}  display {}", hex(list), hex(display)),
        ),
    );
    field(out, "Named Paths", subclass.path_names);
    // Sunrise counts every subclass in item order, the stock nine forming three groups first.
    if let Some(position) = subclasses
        .iter()
        .position(|each| each.namespace == weapon.namespace)
    {
        let start = position - position % 3;
        field(
            out,
            "Sunrise Group",
            match subclasses.get(start..start + 3) {
                Some(group) => format!(
                    "{} of {}, with {}",
                    start / 3 + 1,
                    subclasses.len().div_ceil(3),
                    group
                        .iter()
                        .filter(|each| each.namespace != weapon.namespace)
                        .map(|each| each.name.as_str())
                        .collect::<Vec<_>>()
                        .join(" and ")
                ),
                None => format!(
                    "none  (a last {} of {} subclasses groups nothing)",
                    subclasses.len() - start,
                    subclasses.len()
                ),
            },
        );
    }
    let authored = subclasses
        .iter()
        .filter(|each| {
            each.subclass
                .as_ref()
                .is_some_and(|details| details.own_list.is_some())
        })
        .count();
    field(
        out,
        "Super Lane Lists",
        format!(
            "{} of {SUPER_LANE_LISTS}  ({} stock, {authored} authored)",
            subclass.stock_super_lane_lists + authored,
            subclass.stock_super_lane_lists
        ),
    );
    let _ = writeln!(out, "  Authored Entries ({})", subclass.entries.len());
    for entry in &subclass.entries {
        let _ = writeln!(
            out,
            "    {}  pool {}  record {}",
            entry.label,
            hex(entry.pool_tag),
            hex(entry.record_tag)
        );
        let mut parts = Vec::new();
        for (name, count) in [
            ("custom perks", entry.custom_perks),
            ("retargeted perks", entry.retargeted_perks),
            ("modifiers", entry.modifiers),
            ("values", entry.values),
            ("palettes", entry.palettes),
            ("tints", entry.tints),
            ("swaps", entry.swaps),
            ("bank values", entry.bank_values),
        ] {
            if count > 0 {
                parts.push(format!("{name} {count}"));
            }
        }
        if entry.grade {
            parts.push("grade".to_owned());
        }
        if let Some(row) = entry.icon_row {
            parts.push(format!("icon row {row}"));
        }
        if let Some(row) = entry.ability_row {
            parts.push(format!("ability row {row}"));
        }
        if !parts.is_empty() {
            let _ = writeln!(out, "      {}", parts.join("  "));
        }
    }
}

/// Where an install puts the item. An item with a Collections entry is unlocked there. A
/// subclass has none, so its class's characters receive it and each class's first is equipped.
/// A shader also arrives in the profile as a stack. A mod has no entry, since every socket of its
/// type offers it.
pub(super) fn install_text(kind: ItemKind, class: Option<u8>, every_class: bool) -> String {
    match kind {
        ItemKind::Subclass => {
            let whose = class
                .and_then(crate::app::gear_view::class_label)
                .map_or_else(
                    || "the base class's characters".to_owned(),
                    |class| format!("{class} characters"),
                );
            let equipped = "where it is the project's first subclass for the class";
            if every_class {
                format!("every character, equipped on {whose} {equipped}")
            } else {
                format!("{whose}, equipped {equipped}")
            }
        }
        ItemKind::Shader => format!(
            "Collections, unlocked, and the profile as a stack of up to {}",
            crate::install::SHADER_STACK
        ),
        ItemKind::Mod => "every socket of its type, offered without an unlock".to_owned(),
        _ => "Collections, unlocked".to_owned(),
    }
}

/// Where the recipe's item goes when installed.
pub(super) fn append_install(out: &mut String, recipe: &WeaponRecipe) {
    let overrides = &recipe.overrides;
    let _ = writeln!(out, "\nINSTALL");
    let class = overrides
        .subclass_class
        .and_then(crate::ArmorClass::native_class);
    field(
        out,
        "goes to",
        install_text(recipe.kind, class, overrides.subclass_every_class),
    );
}

/// What only one kind of item carries: a subclass's class and authored abilities, armor's class
/// and Collections pages, an emblem's nameplate and trackers, a shader's dyes.
pub(super) fn append_kind(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
    let overrides = &recipe.overrides;
    match recipe.kind {
        ItemKind::Subclass => append_subclass(out, recipe),
        ItemKind::Sparrow => {
            let _ = writeln!(out, "\nSparrow");
            let settings = overrides.sparrow.clone().unwrap_or_default();
            field(out, "Summon Vehicle", settings.summon.label());
            if let Ok(Some(entity)) = settings.summon.entity() {
                field(out, "Vehicle Entity Tag", format!("0x{entity:08X}"));
            }
            field(
                out,
                "Driving Speed",
                format!(
                    "{}% of the complete forward and reverse motion programs",
                    settings.speed_percent
                ),
            );
            field(
                out,
                "Gameplay Verification",
                "Speed, boost and alternate summoning require an in-game test",
            );
        }
        ItemKind::Armor => {
            use crate::ArmorClass;
            let _ = writeln!(out, "\nARMOR");
            field(
                out,
                "class",
                overrides.armor_class.map_or_else(
                    || "follows the base armor".to_owned(),
                    |class| format!("{}  (recipe)", class.label()),
                ),
            );
            let classes = match overrides.armor_class {
                Some(ArmorClass::Any) => "Titan, Hunter and Warlock",
                Some(class) => class.label(),
                None => "the base armor's class",
            };
            let exotic = match overrides.rarity {
                Some(rarity) => rarity == crate::recipe::RecipeRarity::Exotic,
                None => donor.is_some_and(|donor| {
                    donor.summary.rarity == sundial::investment::WeaponRarity::Exotic
                }),
            };
            field(
                out,
                "Collections",
                if exotic {
                    format!("Items / Exotic / Armor / {classes}")
                } else {
                    format!("Items / Armor / {classes} / the project's category / a numbered set")
                },
            );
        }
        ItemKind::Emblem => {
            use crate::emblem::NameplatePart;
            let _ = writeln!(out, "\nEMBLEM");
            let nameplate = overrides.nameplate.as_ref();
            for part in NameplatePart::ALL {
                field(
                    out,
                    &format!("nameplate {}", part.label().to_lowercase()),
                    match nameplate.and_then(|nameplate| nameplate.part(part)) {
                        None => "base emblem's".to_owned(),
                        Some(crate::emblem::NameplateImage::Emblem { item_hash }) => {
                            format!("emblem {item_hash}")
                        }
                        Some(image) => json_summary(image),
                    },
                );
            }
            field(
                out,
                "nameplate colors",
                nameplate
                    .and_then(|nameplate| nameplate.colors.as_ref())
                    .map_or_else(|| "base emblem's".to_owned(), json),
            );
            field(
                out,
                "stat trackers",
                match &overrides.stat_trackers {
                    None => "base emblem's".to_owned(),
                    Some(crate::emblem::StatTrackers::All) => "all".to_owned(),
                    Some(crate::emblem::StatTrackers::Selected { categories }) => {
                        list(categories.iter())
                    }
                },
            );
        }
        ItemKind::Shader => {
            let _ = writeln!(out, "\nSHADER");
            field(out, "icon from dyes", overrides.icon_from_dyes);
            let _ = writeln!(out, "  dye edits ({})", overrides.dye_edits.len());
            for edit in &overrides.dye_edits {
                let _ = writeln!(out, "    {}", json(edit));
            }
            let _ = writeln!(
                out,
                "  texture edits ({})",
                overrides.dye_texture_edits.len()
            );
            for edit in &overrides.dye_texture_edits {
                let _ = writeln!(out, "    {}", json(edit));
            }
        }
        _ => {}
    }
}

/// A subclass's class and what each of its abilities and attunement nodes authors.
pub(super) fn append_subclass(out: &mut String, recipe: &WeaponRecipe) {
    use crate::subclass::Place;
    let overrides = &recipe.overrides;
    let _ = writeln!(out, "\nSUBCLASS");
    field(
        out,
        "class",
        if overrides.subclass_every_class {
            "Any Class  (recipe, equip condition removed)".to_owned()
        } else {
            overrides.subclass_class.map_or_else(
                || "base subclass's".to_owned(),
                |class| format!("{}  (recipe)", class.label()),
            )
        },
    );
    field(
        out,
        "damage type icon",
        overrides.subclass_damage_type.map_or_else(
            || "base subclass's".to_owned(),
            |damage| format!("{damage:?}  (recipe, item strings)"),
        ),
    );
    field(
        out,
        "screen art",
        overrides
            .screen_art
            .as_ref()
            .map_or_else(|| "base subclass's".to_owned(), json_summary),
    );
    let Some(abilities) = overrides.subclass_abilities.as_ref() else {
        field(out, "abilities", "base subclass's");
        return;
    };
    let _ = writeln!(out, "  abilities ({})", abilities.choices.len());
    for choice in &abilities.choices {
        let _ = writeln!(
            out,
            "    {}  based on {} entry {}",
            Place::Ability(choice.entry).label(),
            hex(choice.source),
            choice.source_entry
        );
        append_entry_edits(out, &choice.edits, "      ");
    }
    let _ = writeln!(out, "  attunements ({})", abilities.attunements.len());
    for attunement in &abilities.attunements {
        let _ = writeln!(
            out,
            "    {} Path  from {} {} path{}",
            attunement.path.label(),
            hex(attunement.source),
            attunement.source_path.label().to_lowercase(),
            attunement
                .name
                .as_ref()
                .map_or_else(String::new, |name| format!("  named {name:?}"))
        );
        for node in &attunement.nodes {
            let _ = writeln!(
                out,
                "      Node {}  based on {} {} path node {}",
                node.position + 1,
                hex(node.source),
                node.source_path.label().to_lowercase(),
                node.source_position + 1
            );
            append_entry_edits(out, &node.edits, "        ");
        }
    }
}

/// One line for each change an ability or node makes.
pub(super) fn append_entry_edits(
    out: &mut String,
    edits: &crate::subclass::EntryEdits,
    indent: &str,
) {
    let mut lines = entry_text_lines(edits);
    if edits.extra_charges > 0 {
        lines.push(format!("extra charges +{}", edits.extra_charges));
    }
    if edits.recharge_bits != 0 {
        lines.push(format!("recharge ×{}", f32::from_bits(edits.recharge_bits)));
    }
    for modifier in &edits.modifiers {
        lines.push(format!("modifier {}", json(modifier)));
    }
    for modifier in &edits.removed_modifiers {
        lines.push(format!(
            "removed modifier key {} row {}",
            hex(modifier.key),
            modifier.row
        ));
    }
    for swap in &edits.spawn_swaps {
        lines.push(format!(
            "swap in {}  {} -> {}",
            hex(swap.graph),
            hex(swap.replaced),
            hex(swap.replacement)
        ));
    }
    for (name, count) in [
        ("parameters", edits.parameters.len()),
        ("values", edits.ability_values.len()),
        ("palettes", edits.palettes.len()),
        ("tints", edits.tints.len()),
        ("bank values", edits.bank_values.len()),
    ] {
        if count > 0 {
            lines.push(format!("{name} {count}"));
        }
    }
    if let Some(grade) = &edits.grade {
        lines.push(format!("grade {}", json(grade)));
    }
    if let (Some(damage), Some(mode)) = (edits.damage_type, edits.damage_mode()) {
        lines.push(format!("damage type {damage:?}  (profile mode {mode})"));
    }
    if lines.is_empty() {
        lines.push("no edits".to_owned());
    }
    for line in lines {
        let _ = writeln!(out, "{indent}{line}");
    }
}

/// The lines for an ability's or node's text, icon and perks.
pub(super) fn entry_text_lines(edits: &crate::subclass::EntryEdits) -> Vec<String> {
    use crate::subclass::EntryIcon;
    let mut lines = Vec::new();
    if let Some(name) = &edits.name {
        lines.push(format!("name {name:?}"));
    }
    if let Some(description) = &edits.description {
        lines.push(format!("description {} chars", description.chars().count()));
    }
    match &edits.icon {
        Some(EntryIcon::Ability { subclass, entry }) => {
            lines.push(format!("icon of {} entry {entry}", hex(*subclass)));
        }
        Some(EntryIcon::Artwork { .. }) => lines.push("icon artwork".to_owned()),
        None => {}
    }
    for (name, perks) in [
        ("added perks", &edits.added_perks),
        ("removed perks", &edits.removed_perks),
    ] {
        if !perks.is_empty() {
            lines.push(format!("{name} {}", list(perks.iter())));
        }
    }
    if !edits.custom_perks.is_empty() {
        lines.push(format!(
            "custom perks {}",
            list(
                edits
                    .custom_perks
                    .iter()
                    .map(|perk| format!("{:?}", perk.name))
            )
        ));
    }
    lines
}

/// The identities a build writes are fixed by the recipe; only table indices wait for the build.
pub(super) fn append_planned(out: &mut String, recipe: &WeaponRecipe) {
    let _ = writeln!(out, "NEXT BUILD  {}", recipe.name);
    let _ = writeln!(
        out,
        "  No staged build. Indices are assigned at build time."
    );
    field(out, "namespace", &recipe.namespace);
    field(out, "schema", recipe.schema);
    match recipe.identity.parsed_hashes(&recipe.namespace) {
        Ok(hashes) => {
            for (name, value) in [
                "item hash",
                "collectible hash",
                "unlock hash",
                "pattern global id",
                "name string",
                "type string",
                "flavor string",
                "source string",
                "collection name",
                "collection description",
                "inventory hint",
                "collection requirement",
            ]
            .into_iter()
            .zip(hashes)
            {
                field(out, name, hex(value));
            }
        }
        Err(error) => field(out, "identity", format!("<invalid: {error}>")),
    }
    field(
        out,
        "custom plugs",
        recipe.overrides.socket_plug_variants.len(),
    );
}

pub(super) fn donor_reference(
    out: &mut String,
    name: &str,
    reference: &crate::recipe::WeaponDonorReference,
) {
    field(
        out,
        name,
        format!(
            "{}  {}",
            reference.item_hash,
            reference
                .expected_name
                .as_deref()
                .unwrap_or("<no expected name>")
        ),
    );
}

pub(super) fn append_donors(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
    let weapon = recipe.kind.is_weapon();
    let base = if weapon {
        "gameplay donor"
    } else {
        "base item"
    };
    let _ = writeln!(out, "\nDONORS");
    donor_reference(out, base, &recipe.donor);
    match donor {
        Some(donor) => {
            let summary = &donor.summary;
            field(out, "  catalog name", &summary.name);
            field(out, "  catalog hash", hex(summary.hash));
            field(out, "  type", &summary.type_name);
            field(out, "  rarity", format!("{:?}", summary.rarity));
            field(
                out,
                "  bucket hash",
                format!("0x{:016X}", summary.bucket_hash),
            );
            field(out, "  collection backed", summary.collection_backed);
            if weapon {
                field(out, "  power cap", opt(summary.power_cap));
                field(
                    out,
                    "  damage profile",
                    format!("{:?}", summary.damage_profile),
                );
                field(
                    out,
                    "  translation group",
                    opt(summary.weapon_translation_group),
                );
                field(out, "  equipment slot", opt_debug(donor.equipment_slot));
            }
        }
        None => field(out, "  catalog", "not loaded"),
    }
    for (name, reference) in [
        ("presentation donor", recipe.presentation_donor.as_ref()),
        ("render gear donor", recipe.render_gear_donor.as_ref()),
        ("icon donor", recipe.icon_donor.as_ref()),
    ] {
        match reference {
            Some(reference) => donor_reference(out, name, reference),
            None => field(out, name, format!("none  ({base})")),
        }
    }
    if !weapon {
        return;
    }
    for (name, reference, follows) in [
        (
            "animation donor",
            recipe.overrides.animation_donor.as_ref(),
            "none  (appearance)",
        ),
        (
            "type marker donor",
            recipe.overrides.type_marker_donor.as_ref(),
            "none  (base weapon)",
        ),
    ] {
        match reference {
            Some(reference) => donor_reference(out, name, reference),
            None => field(out, name, follows),
        }
    }
    let _ = writeln!(
        out,
        "  component splices ({})",
        recipe.overrides.component_splices.len()
    );
    for splice in &recipe.overrides.component_splices {
        let _ = writeln!(
            out,
            "    binding {}  donor {}  {}",
            splice.binding_hash,
            splice.donor.item_hash,
            splice
                .donor
                .expected_name
                .as_deref()
                .unwrap_or("<no expected name>")
        );
    }
    let _ = writeln!(
        out,
        "  runtime component donors ({})",
        recipe.runtime_component_donors.len()
    );
    for component in &recipe.runtime_component_donors {
        let _ = writeln!(
            out,
            "    binding {}  donor {}  {}",
            component.binding_hash,
            component.donor.item_hash,
            component
                .donor
                .expected_name
                .as_deref()
                .unwrap_or("<no expected name>")
        );
    }
    field(
        out,
        "pattern donor hash",
        opt(recipe.overrides.weapon_pattern_donor_hash.as_ref()),
    );
    field(
        out,
        "stat group donor hash",
        opt(recipe.overrides.stat_group_donor_hash.as_ref()),
    );
}

pub(super) fn opt<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

pub(super) fn opt_debug<T: std::fmt::Debug>(value: Option<T>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| format!("{value:?}"))
}

pub(super) fn append_item(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
    let overrides = &recipe.overrides;
    let weapon = recipe.kind.is_weapon();
    let _ = writeln!(
        out,
        "\nITEM DEFINITION  (recipe override, else donor value)"
    );
    if weapon {
        resolved(
            out,
            "inventory slot",
            overrides.inventory_slot.as_ref().map(json),
            donor.map(|donor| donor.summary.inventory_slot.map(|slot| format!("{slot:?}"))),
        );
        resolved(
            out,
            "ammo type",
            overrides.ammo_type.as_ref().map(json),
            donor.map(|donor| donor.summary.ammo_type.map(|ammo| format!("{ammo:?}"))),
        );
        resolved(
            out,
            "damage type",
            overrides.modern_damage_type.as_ref().map(json),
            donor.map(|donor| {
                donor
                    .summary
                    .damage_type
                    .map(|damage| format!("{damage:?}"))
            }),
        );
    }
    resolved(
        out,
        "rarity",
        overrides.rarity.as_ref().map(json),
        donor.map(|donor| Some(format!("{:?}", donor.summary.rarity))),
    );
    if weapon || recipe.kind == ItemKind::Armor {
        resolved(
            out,
            "power cap groups",
            overrides
                .power_cap_groups
                .as_ref()
                .map(|groups| list(groups.iter()))
                .or_else(|| overrides.power_cap_group.map(|group| list([group]))),
            donor.map(|donor| Some(list(donor.power_cap_groups.iter()))),
        );
    }
    resolved(
        out,
        "max stack size",
        overrides.max_stack_size,
        donor.map(|donor| donor.max_stack_size),
    );
    resolved(
        out,
        "socket entry list",
        overrides.socket_entry_list_index,
        donor.map(|donor| donor.socket_entry_list_index),
    );
    resolved(
        out,
        "plug category hash",
        overrides
            .plug_category_hash
            .as_ref()
            .map(ToString::to_string),
        donor.map(|donor| donor.plug_category_hash.map(hex)),
    );
    if weapon {
        append_weapon_item(out, recipe, donor);
    }
    // A subclass or mod has no Collections entry.
    if recipe.kind.has_collections() {
        field(
            out,
            "collection placement",
            json(&recipe.collection_placement),
        );
        if weapon {
            field(
                out,
                "collection destination",
                overrides
                    .collection_destination
                    .map_or_else(|| "none".to_owned(), |destination| destination.label()),
            );
        }
        field(
            out,
            "exclude from Sunrise badge",
            overrides.exclude_from_sunrise_badge,
        );
    }
}

/// The item definition fields only a weapon carries.
pub(super) fn append_weapon_item(
    out: &mut String,
    recipe: &WeaponRecipe,
    donor: Option<&WeaponDonor>,
) {
    let overrides = &recipe.overrides;
    resolved(
        out,
        "roll set index",
        overrides.roll_set_index,
        donor.map(|donor| donor.roll_set_index),
    );
    resolved(
        out,
        "linked plug index",
        overrides.linked_plug_index,
        donor.map(|donor| donor.linked_plug_index),
    );
    if let Some(donor) = donor {
        field(
            out,
            "linked plug hash (donor)",
            opt(donor.linked_plug_hash.map(hex)),
        );
    }
    resolved(
        out,
        "weapon pattern index",
        overrides.weapon_pattern_index,
        donor.map(|donor| donor.summary.weapon_pattern_index),
    );
    resolved(
        out,
        "stat group index",
        overrides.stat_group_index,
        donor.map(|donor| donor.summary.stat_group_index),
    );
    field(
        out,
        "variable damage",
        overrides
            .variable_damage
            .as_ref()
            .map_or_else(|| "none".to_owned(), |damage| json(&damage.elements)),
    );
    field(
        out,
        "projectile speed bits",
        opt(overrides
            .behavior_projectile_speed_bits
            .map(|bits| format!("{} ({})", hex(bits), f32::from_bits(bits)))),
    );
    field(out, "skip behavior perks", overrides.skip_behavior_perks);
    field(
        out,
        "behavior firing",
        overrides
            .behavior_firing
            .map_or_else(|| "behavior (default)".to_owned(), |firing| json(&firing)),
    );
}

pub(super) fn append_stats(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
    let overrides = &recipe.overrides;
    let _ = writeln!(out, "\nINVESTMENT STATS");
    let Some(donor) = donor else {
        field(out, "donor stats", "catalog not loaded");
        for stat in &overrides.investment_stats {
            let _ = writeln!(
                out,
                "  definition {:<5} value {:<6} (recipe)",
                stat.definition_index, stat.value
            );
        }
        field(
            out,
            "removed",
            list(overrides.removed_investment_stats.iter()),
        );
        return;
    };
    let override_value = |index: u16| {
        overrides
            .investment_stats
            .iter()
            .find(|stat| stat.definition_index == index)
            .map(|stat| stat.value)
    };
    let mut seen = std::collections::BTreeSet::new();
    for stat in &donor.investment_stats {
        seen.insert(stat.definition_index);
        let removed = overrides
            .removed_investment_stats
            .contains(&stat.definition_index);
        let (value, origin) = match (removed, override_value(stat.definition_index)) {
            (true, _) => ("removed".to_owned(), "recipe"),
            (false, Some(value)) => (value.to_string(), "recipe"),
            (false, None) => (stat.value.to_string(), "donor"),
        };
        let _ = writeln!(
            out,
            "  definition {:<5} hash {}  {:<28} value {:<8} donor {:<6} range {}  display {}  {}  ({origin})",
            stat.definition_index,
            opt(stat.definition_hash.map(hex)),
            stat.name,
            value,
            stat.value,
            stat.value_range().map_or_else(
                || "none".to_owned(),
                |(low, high)| format!("{low}..={high}")
            ),
            if stat.display_as_numeric {
                "numeric"
            } else {
                "bar"
            },
            if stat.is_linear {
                "linear"
            } else {
                "interpolated"
            },
        );
        if !stat.display_interpolation.is_empty() {
            let _ =
                writeln!(
                    out,
                    "      interpolation {}",
                    list(stat.display_interpolation.iter().map(|point| format!(
                        "{}->{}",
                        point.investment_value, point.display_value
                    )))
                );
        }
    }
    for stat in &overrides.investment_stats {
        if seen.contains(&stat.definition_index) {
            continue;
        }
        let addable = donor
            .addable_investment_stats
            .iter()
            .find(|candidate| candidate.definition_index == stat.definition_index);
        let _ = writeln!(
            out,
            "  definition {:<5} hash {}  {:<28} value {:<8} added  (recipe)",
            stat.definition_index,
            opt(addable.and_then(|stat| stat.definition_hash).map(hex)),
            addable.map_or("<not an installed stat>", |stat| stat.name.as_str()),
            stat.value
        );
    }
    field(
        out,
        "removed",
        list(overrides.removed_investment_stats.iter()),
    );
}

pub(super) fn append_perks(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
    let overrides = &recipe.overrides;
    let _ = writeln!(out, "\nBASE PERKS AND TRAITS");
    resolved(
        out,
        "base sandbox perks",
        overrides
            .base_sandbox_perks
            .as_ref()
            .map(|perks| list(perks.iter())),
        donor.map(|donor| Some(list(donor.base_sandbox_perks.iter()))),
    );
    resolved(
        out,
        "trait indices",
        overrides
            .trait_indices
            .as_ref()
            .map(|traits| list(traits.iter())),
        donor.map(|donor| Some(list(donor.trait_indices.iter()))),
    );
}

pub(super) fn append_text(out: &mut String, recipe: &WeaponRecipe) {
    let _ = writeln!(out, "\nTEXT");
    field(out, "name", &recipe.name);
    field(
        out,
        "type name",
        recipe
            .type_name
            .as_deref()
            .unwrap_or("inherits donor  (no item-type override)"),
    );
    field(out, "flavor", &recipe.flavor);
    field(out, "source", &recipe.source);
    for (name, value) in [
        ("collection name", recipe.collection_name.as_deref()),
        (
            "collection description",
            recipe.collection_description.as_deref(),
        ),
        ("inventory hint", recipe.inventory_hint.as_deref()),
        (
            "collection requirement",
            recipe.collection_requirement.as_deref(),
        ),
    ] {
        field(out, name, value.unwrap_or("none"));
    }
    field(
        out,
        "lore",
        match (&recipe.overrides.lore, recipe.overrides.remove_lore) {
            (Some(lore), _) => format!("{} chars", lore.chars().count()),
            (None, true) => "removed".to_owned(),
            (None, false) => "inherits donor".to_owned(),
        },
    );
    let _ = writeln!(
        out,
        "  locale overrides ({})",
        recipe.locale_overrides.len()
    );
    for locale in &recipe.locale_overrides {
        let _ = writeln!(
            out,
            "    locale {:<3} {}",
            locale.locale_index,
            json(locale)
        );
    }
}

pub(super) fn append_appearance(
    out: &mut String,
    recipe: &WeaponRecipe,
    donor: Option<&WeaponDonor>,
) {
    let overrides = &recipe.overrides;
    let weapon = recipe.kind.is_weapon();
    let _ = writeln!(out, "\nAPPEARANCE");
    if weapon {
        #[cfg(feature = "d2-model-importer")]
        field(
            out,
            "imported graph",
            overrides
                .imported_graph
                .as_ref()
                .map_or_else(|| "none".to_owned(), json),
        );
        field(
            out,
            "hud icon",
            overrides
                .hud_icon
                .as_ref()
                .map_or_else(|| "none".to_owned(), json_summary),
        );
        field(out, "shader glow", overrides.shader_glow);
    }
    field(
        out,
        "icon edit",
        if overrides.icon_edit.is_identity() {
            "identity".to_owned()
        } else {
            json_summary(&overrides.icon_edit)
        },
    );
    if weapon || overrides.badge.is_some() {
        field(
            out,
            "badge",
            overrides.badge.as_ref().map_or_else(
                || "none".to_owned(),
                |badge| {
                    format!(
                        "{:?}  {:?}  icon {}",
                        badge.name,
                        badge.description,
                        badge.icon.as_ref().map_or("none", |_| "set")
                    )
                },
            ),
        );
    }
    if weapon || overrides.corner_icon.is_some() {
        field(
            out,
            "corner icon",
            overrides
                .corner_icon
                .as_ref()
                .map_or_else(|| "none".to_owned(), json_summary),
        );
    }
    if wears_gear_art(recipe.kind) {
        append_gear_art(out, recipe, donor);
    }
    if !weapon {
        return;
    }
    let [forward, side, up] = overrides.held_offset_um.map(|um| f64::from(um) / 1000.0);
    field(
        out,
        "held offset",
        if overrides.held_offset_um == [0; 3] {
            "none".to_owned()
        } else {
            format!("forward {forward:+.3} mm  side {side:+.3} mm  up {up:+.3} mm")
        },
    );
    let _ = writeln!(out, "  moved markers ({})", overrides.marker_offsets.len());
    for offset in &overrides.marker_offsets {
        let label = offset
            .marker
            .parse_u32()
            .ok()
            .and_then(marker_name)
            .map_or_else(|| offset.marker.to_string(), ToOwned::to_owned);
        let [forward, side, up] = offset.offset_um.map(|um| f64::from(um) / 1000.0);
        let _ = writeln!(
            out,
            "    {label:<24}  forward {forward:+.3} mm  side {side:+.3} mm  up {up:+.3} mm"
        );
    }
}

/// The gear art an item wears: its arrangements and dye rows.
pub(super) fn append_gear_art(
    out: &mut String,
    recipe: &WeaponRecipe,
    donor: Option<&WeaponDonor>,
) {
    let overrides = &recipe.overrides;
    resolved(
        out,
        "art arrangements",
        overrides.art_arrangements.as_ref().map(|rows| {
            list(rows.iter().map(|row| {
                format!(
                    "class {} arrangement {}",
                    row.character_class, row.arrangement
                )
            }))
        }),
        donor.map(|donor| {
            Some(list(donor.art_arrangements.iter().map(|row| {
                format!(
                    "class {} arrangement {}",
                    row.character_class, row.arrangement
                )
            })))
        }),
    );
    for (channel, name) in ["custom", "default", "locked"].into_iter().enumerate() {
        resolved(
            out,
            &format!("dye rows {name}"),
            overrides
                .render_dye_rows
                .as_ref()
                .map(|rows| json(&rows[channel])),
            donor.map(|donor| {
                Some(list(donor.render_dye_rows[channel].iter().map(|row| {
                    format!(
                        "channel {} dye {}",
                        row.channel_index, row.dye_reference_index
                    )
                })))
            }),
        );
    }
}

pub(super) fn append_recipe(out: &mut String, recipe: &WeaponRecipe) {
    let overrides = &recipe.overrides;
    let _ = writeln!(
        out,
        "\nUNIQUE WEAPON BEHAVIOR ({})",
        overrides.additional_behaviors.len()
    );
    if overrides.additional_behaviors.is_empty() {
        let _ = writeln!(out, "  none");
    }
    for request in &overrides.additional_behaviors {
        let Some(entry) = crate::weapon::behavior::behavior(&request.behavior) else {
            let _ = writeln!(out, "  {}  <not in the catalog>", request.behavior);
            continue;
        };
        let _ = writeln!(out, "  {}  from {}", entry.id, entry.source_name);
        field(out, "  source item", hex(entry.source_item_hash));
        if let Some(tag) = entry.graph_tag() {
            field(out, "  firing graph", hex(tag));
        }
        if let Some(owner) = entry.owner_tag() {
            field(out, "  record owner", hex(owner));
        }
        if let Some(record) = crate::weapon::behavior::paired_record_source(entry) {
            field(out, "  carries record of", record);
        }
        for (name, plug) in [
            ("  pinned intrinsic", entry.intrinsic_plug),
            ("  pinned trait", entry.trait_plug),
        ] {
            if let Some(plug) = plug {
                field(out, name, hex(plug));
            }
        }
    }
}

pub(super) fn append_sockets(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
    let overrides = &recipe.overrides;
    let lanes = overrides
        .socket_columns
        .len()
        .max(donor.map_or(0, |donor| donor.sockets.len()));
    let _ = writeln!(
        out,
        "\nSOCKETS ({lanes} lanes, {} recipe columns)",
        overrides.socket_columns.len()
    );
    for lane in 0..lanes {
        let column = overrides.socket_columns.get(lane).and_then(Option::as_ref);
        let native = donor.and_then(|donor| donor.sockets.get(lane));
        match native {
            Some(socket) => {
                let _ = writeln!(
                    out,
                    "  lane {lane:<3} donor type {:<6} {:<24} default {}  embedded {}  max authored {}  compatible {}  reusable {}  randomized {}",
                    socket.socket_type,
                    socket.label,
                    opt(socket.native_default.map(hex)),
                    hex_list(socket.ordered_embedded_choices.iter().copied()),
                    socket.max_authored_choices,
                    socket.compatible_plug_count,
                    opt(socket.reusable_plug_set_index),
                    opt(socket.randomized_plug_set_index),
                );
            }
            None if donor.is_some() => {
                let _ = writeln!(out, "  lane {lane:<3} donor has no socket here");
            }
            None => {
                let _ = writeln!(
                    out,
                    "  lane {lane:<3} donor socket unresolved  (catalog not loaded)"
                );
            }
        }
        let Some(column) = column else {
            let _ = writeln!(out, "           recipe   inherits the donor's column");
            continue;
        };
        let _ = writeln!(
            out,
            "           recipe   type {:<6} choices {}",
            column
                .socket_type
                .map_or_else(|| "donor".to_owned(), |kind| kind.to_string()),
            list(column.choices.iter())
        );
        if !column.choice_weight_bits.is_empty() {
            let _ = writeln!(
                out,
                "                    weight bits {}",
                json(&column.choice_weight_bits)
            );
        }
        if !column.choice_conditions.is_empty() {
            let _ = writeln!(
                out,
                "                    conditions {}",
                json(&column.choice_conditions)
            );
        }
        if !column.randomized_selection_program.is_empty() {
            let _ = writeln!(
                out,
                "                    selection program {}",
                json(&column.randomized_selection_program)
            );
        }
        for (name, index) in [
            ("reusable plug set", column.reusable_plug_set_index),
            ("randomized plug set", column.randomized_plug_set_index),
        ] {
            if let Some(index) = index {
                let _ = writeln!(out, "                    {name} {index}");
            }
        }
    }

    let _ = writeln!(
        out,
        "\nPRIVATE PERK VARIANTS ({})",
        overrides.socket_plug_variants.len()
    );
    for variant in &overrides.socket_plug_variants {
        let _ = writeln!(
            out,
            "  socket {} choice {}  source {}  {}",
            variant.socket_index,
            variant.choice_index,
            variant.source_plug_hash,
            variant.name.as_deref().unwrap_or("<inherits its name>")
        );
        let _ = writeln!(out, "    {}", json(variant));
    }
}

pub(super) fn append_runtime(out: &mut String, recipe: &WeaponRecipe) {
    let overrides = &recipe.overrides;
    let _ = writeln!(
        out,
        "\nRUNTIME PATCHES  values {}  resource patches {}  raw payload patches {}",
        overrides.runtime_values.len(),
        overrides.runtime_resource_patches.len(),
        overrides.raw_payload_patches.len()
    );
    for (name, entries) in [
        ("value", json_lines(&overrides.runtime_values)),
        ("resource", json_lines(&overrides.runtime_resource_patches)),
        ("raw payload", json_lines(&overrides.raw_payload_patches)),
    ] {
        for entry in entries {
            let _ = writeln!(out, "  {name:<12}{entry}");
        }
    }
}

pub(super) fn json_lines<T: serde::Serialize>(values: &[T]) -> Vec<String> {
    values.iter().map(json).collect()
}

/// The recipe document itself, every key, so a field no curated section names is still shown.
/// Embedded images and other long strings are summarized by length and digest.
pub(super) fn append_document(out: &mut String, recipe: &WeaponRecipe) {
    let _ = writeln!(out, "\nRECIPE DOCUMENT  (every field as saved)");
    match serde_json::to_value(recipe) {
        Ok(value) => append_value(out, "", &value, 1),
        Err(error) => field(out, "document", format!("<unserializable: {error}>")),
    }
}

pub(super) fn append_value(out: &mut String, key: &str, value: &serde_json::Value, depth: usize) {
    let indent = "  ".repeat(depth);
    let label = if key.is_empty() {
        String::new()
    } else {
        format!("{key}: ")
    };
    match value {
        serde_json::Value::Object(map) => {
            if !key.is_empty() {
                let _ = writeln!(out, "{indent}{key}:");
            }
            for (child, value) in map {
                append_value(out, child, value, depth + usize::from(!key.is_empty()));
            }
        }
        serde_json::Value::Array(items) if items.iter().any(serde_json::Value::is_object) => {
            let _ = writeln!(out, "{indent}{key}: [{}]", items.len());
            for (index, item) in items.iter().enumerate() {
                append_value(out, &format!("[{index}]"), item, depth + 1);
            }
        }
        serde_json::Value::Array(items) => {
            let _ = writeln!(out, "{indent}{label}{}", json(items));
        }
        serde_json::Value::String(text) if text.len() > LONG_STRING_BYTES => {
            let _ = writeln!(out, "{indent}{label}{}", long_string_summary(text));
        }
        other => {
            let _ = writeln!(out, "{indent}{label}{other}");
        }
    }
}

pub(super) fn sha256_prefix(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(bytes);
    digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
