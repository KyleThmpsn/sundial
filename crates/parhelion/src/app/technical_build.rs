//! Every value a build writes into the weapon, in one window.
//!
//! A weapon tag is the donor's item definition with the recipe's overrides applied, plus the
//! identities, table indices and artifacts a build assigns. Until now none of that was visible
//! in one place: a build either installed or failed, and the numbers behind it lived only in
//! the staged manifest and the packages. This renders the whole of it, resolved against the
//! loaded catalog, so a weapon can be checked field by field before it is built and against
//! what a build produced before it reaches the game.
//!
//! The report is built as text rather than laid out as widgets. It is dense by intent, every
//! line is selectable, and the whole thing copies in one action, which is what makes it useful
//! in a bug report. The final section walks the recipe document itself, so a value the curated
//! sections do not name still appears.
use std::fmt::Write as _;

use sundial::investment::WeaponDonor;

use crate::recipe::WeaponRecipe;
use crate::workflow::BuildReport;

/// Renders the report. Pure, so its content is tested without drawing a frame. Without a staged
/// build it reports the identities the next build will assign. With a donor from the loaded
/// catalog, inherited fields show the donor's value instead of "donor".
pub(super) fn technical_build_report(
    build: Option<&BuildReport>,
    recipe: &WeaponRecipe,
    donor: Option<&WeaponDonor>,
    frame_fits: &dyn Fn(&crate::weapon_behavior::Behavior) -> bool,
) -> String {
    let mut out = String::new();
    match build {
        Some(build) => {
            append_build(&mut out, build);
            for (index, weapon) in build.weapons.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "\nWEAPON {}/{}  {}",
                    index + 1,
                    build.weapons.len(),
                    weapon.name
                );
                append_weapon(&mut out, weapon);
            }
        }
        None => append_planned(&mut out, recipe),
    }
    append_donors(&mut out, recipe, donor);
    append_item(&mut out, recipe, donor);
    append_stats(&mut out, recipe, donor);
    append_perks(&mut out, recipe, donor);
    append_text(&mut out, recipe);
    append_appearance(&mut out, recipe, donor);
    append_recipe(&mut out, recipe, frame_fits);
    append_sockets(&mut out, recipe, donor);
    append_runtime(&mut out, recipe);
    append_document(&mut out, recipe);
    out
}

fn hex(value: u32) -> String {
    format!("0x{value:08X}")
}

fn field(out: &mut String, name: &str, value: impl std::fmt::Display) {
    let _ = writeln!(out, "  {name:<26}{value}");
}

/// Compact JSON for a value whose type has no display of its own. Everything in a recipe
/// serializes, so this never leaves a field out.
fn json(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| format!("<unserializable: {error}>"))
}

fn list<T: std::fmt::Display>(values: impl IntoIterator<Item = T>) -> String {
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

fn hex_list(values: impl IntoIterator<Item = u32>) -> String {
    list(values.into_iter().map(hex))
}

/// One resolved field: the recipe's override when it has one, else the donor's value, else a
/// note that the catalog is not loaded. The origin is always named.
fn resolved<T: std::fmt::Display>(
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

fn append_build(out: &mut String, build: &BuildReport) {
    let _ = writeln!(out, "BUILD");
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

fn append_weapon(out: &mut String, weapon: &crate::workflow::WeaponBuildReport) {
    field(out, "namespace", &weapon.namespace);
    field(out, "item hash", hex(weapon.item_hash));
    field(out, "item definition", hex(weapon.item_definition_hash));
    field(out, "item string", hex(weapon.item_string_hash));
    field(out, "icon definition", hex(weapon.icon_definition_hash));
    field(out, "item index", weapon.item_index);
    field(out, "collectible hash", hex(weapon.collectible_hash));
    field(out, "collectible index", weapon.collectible_index);
    field(out, "unlock hash", hex(weapon.unlock_hash));
    field(out, "unlock definition", weapon.unlock_definition_index);
    field(
        out,
        "unlock bank / slot",
        format!("{} / {}", weapon.unlock_bank, weapon.unlock_slot),
    );
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

/// The identities a build writes are fixed by the recipe; only table indices wait for the build.
fn append_planned(out: &mut String, recipe: &WeaponRecipe) {
    let _ = writeln!(out, "NEXT BUILD  {}", recipe.name);
    let _ = writeln!(
        out,
        "  No build is staged. Item, collectible and unlock indices are assigned when it runs."
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

fn donor_reference(out: &mut String, name: &str, reference: &crate::recipe::WeaponDonorReference) {
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

fn append_donors(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
    let _ = writeln!(out, "\nDONORS");
    donor_reference(out, "gameplay donor", &recipe.donor);
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
        None => field(
            out,
            "  catalog",
            "not loaded; inherited values are not resolved",
        ),
    }
    for (name, reference) in [
        ("presentation donor", recipe.presentation_donor.as_ref()),
        ("render gear donor", recipe.render_gear_donor.as_ref()),
        ("icon donor", recipe.icon_donor.as_ref()),
    ] {
        match reference {
            Some(reference) => donor_reference(out, name, reference),
            None => field(out, name, "none  (gameplay donor)"),
        }
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

fn opt<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

fn opt_debug<T: std::fmt::Debug>(value: Option<T>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| format!("{value:?}"))
}

fn append_item(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
    let overrides = &recipe.overrides;
    let _ = writeln!(
        out,
        "\nITEM DEFINITION  (recipe override, else donor value)"
    );
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
    resolved(
        out,
        "rarity",
        overrides.rarity.as_ref().map(json),
        donor.map(|donor| Some(format!("{:?}", donor.summary.rarity))),
    );
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
        "collection placement",
        json(&recipe.collection_placement),
    );
    field(
        out,
        "collection destination",
        overrides
            .collection_destination
            .map_or_else(|| "none".to_owned(), |destination| destination.label()),
    );
    field(
        out,
        "exclude from sunrise badge",
        overrides.exclude_from_sunrise_badge,
    );
}

fn append_stats(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
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

fn append_perks(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
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

fn append_text(out: &mut String, recipe: &WeaponRecipe) {
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

fn append_appearance(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
    let overrides = &recipe.overrides;
    let _ = writeln!(out, "\nAPPEARANCE");
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
            .map_or_else(|| "none".to_owned(), json),
    );
    field(
        out,
        "icon edit",
        if overrides.icon_edit.is_identity() {
            "identity".to_owned()
        } else {
            json(&overrides.icon_edit)
        },
    );
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
    field(
        out,
        "corner icon",
        overrides
            .corner_icon
            .as_ref()
            .map_or_else(|| "none".to_owned(), json),
    );
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

fn append_recipe(
    out: &mut String,
    recipe: &WeaponRecipe,
    frame_fits: &dyn Fn(&crate::weapon_behavior::Behavior) -> bool,
) {
    let overrides = &recipe.overrides;
    let _ = writeln!(
        out,
        "\nBORROWED BEHAVIOR ({})",
        overrides.additional_behaviors.len()
    );
    if overrides.additional_behaviors.is_empty() {
        let _ = writeln!(out, "  none");
    }
    for request in &overrides.additional_behaviors {
        let Some(entry) = crate::weapon_behavior::behavior(&request.behavior) else {
            let _ = writeln!(out, "  {}  <not in the catalogue>", request.behavior);
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
        if let Some(record) = crate::weapon_behavior::paired_record_source(entry) {
            field(out, "  carries record of", record);
        }
        let frame = frame_fits(entry);
        for (name, plug) in [
            ("  pinned intrinsic", entry.intrinsic_plug.filter(|_| frame)),
            ("  pinned trait", entry.trait_plug),
        ] {
            if let Some(plug) = plug {
                field(out, name, hex(plug));
            }
        }
        if !frame && let Some(plug) = entry.intrinsic_plug {
            field(
                out,
                "  intrinsic left behind",
                format!("{}  other weapon type; host keeps its frame", hex(plug)),
            );
        }
    }
}

fn append_sockets(out: &mut String, recipe: &WeaponRecipe, donor: Option<&WeaponDonor>) {
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

fn append_runtime(out: &mut String, recipe: &WeaponRecipe) {
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

fn json_lines<T: serde::Serialize>(values: &[T]) -> Vec<String> {
    values.iter().map(json).collect()
}

/// The recipe document itself, every key, so a field no curated section names is still shown.
/// Embedded images and other long strings are summarized by length and digest.
fn append_document(out: &mut String, recipe: &WeaponRecipe) {
    let _ = writeln!(out, "\nRECIPE DOCUMENT  (every field as saved)");
    match serde_json::to_value(recipe) {
        Ok(value) => append_value(out, "", &value, 1),
        Err(error) => field(out, "document", format!("<unserializable: {error}>")),
    }
}

fn append_value(out: &mut String, key: &str, value: &serde_json::Value, depth: usize) {
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
        serde_json::Value::String(text) if text.len() > 96 => {
            let _ = writeln!(
                out,
                "{indent}{label}<{} bytes, sha256 {}> {}…",
                text.len(),
                sha256_prefix(text.as_bytes()),
                &text[..text
                    .char_indices()
                    .nth(32)
                    .map_or(text.len(), |(index, _)| index)]
            );
        }
        other => {
            let _ = writeln!(out, "{indent}{label}{other}");
        }
    }
}

fn sha256_prefix(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(bytes);
    digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl super::PackageAuthoringApp {
    /// Draws the window when the preference is on, with or without a staged build.
    pub(super) fn draw_technical_build_window(&mut self, ctx: &egui::Context) {
        if !self.show_technical_build {
            self.technical_build_open = false;
            return;
        }
        if !self.technical_build_open {
            return;
        }
        let build = self
            .latest_build
            .as_ref()
            .and_then(|build| build.as_ref().ok());
        let donor = self.current_donor();
        let catalog = self.catalog.as_ref();
        let host_type = donor.as_ref().map(|donor| donor.summary.type_name.as_str());
        let report = technical_build_report(build, &self.recipe, donor.as_ref(), &|entry| {
            crate::weapon_behavior::same_family(
                host_type,
                catalog
                    .and_then(|catalog| catalog.item_type_name(entry.source_item_hash))
                    .as_deref(),
            )
        });
        let mut open = self.technical_build_open;
        egui::Window::new("Technical Build")
            .open(&mut open)
            .default_width(760.0)
            .default_height(520.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Copy Everything").clicked() {
                        ui.ctx().copy_text(report.clone());
                    }
                    ui.weak(format!("{} lines", report.lines().count()));
                });
                ui.separator();
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Selectable so a single figure can be lifted out without copying it all.
                        ui.add(
                            egui::TextEdit::multiline(&mut report.as_str())
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .code_editor(),
                        );
                    });
            });
        self.technical_build_open = open;
    }
}

#[cfg(test)]
mod tests;
