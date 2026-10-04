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
use sundial::package_authoring::gear_markers::{self, MarkerSet, marker_name};
use sundial::package_authoring::runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldSource, WeaponRuntimeGraph, WeaponRuntimeRoot,
    WeaponRuntimeValue, WeaponRuntimeValueKind,
};

use crate::recipe::{MarkerOffsetRecipe, WeaponRecipe};
use crate::workflow::BuildReport;

/// The effective runtime entity behind the report, once the background scan has produced it.
/// `None` while no scan has finished for the current recipe.
pub(super) type Registry<'a> = Option<Result<&'a WeaponRuntimeGraph, &'a str>>;

/// The gear-art markers behind the appearance, once the background read has produced them.
/// `None` while no read has finished for the arrangement on screen.
pub(super) type Markers<'a> = Option<Result<&'a [MarkerSet], &'a str>>;

/// A finished marker read and the arrangements it covers.
pub(super) type MarkerRead = (Vec<u16>, Result<Vec<MarkerSet>, String>);

/// What a rendered runtime-registry section was rendered from. A graph is held weakly, so its
/// address cannot be reused by another graph while the section is cached.
#[derive(Clone)]
enum RegistrySource {
    Pending,
    Failed(String),
    Graph(std::sync::Weak<WeaponRuntimeGraph>),
}

impl PartialEq for RegistrySource {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Pending, Self::Pending) => true,
            (Self::Failed(left), Self::Failed(right)) => left == right,
            (Self::Graph(left), Self::Graph(right)) => left.ptr_eq(right),
            _ => false,
        }
    }
}

/// The rendered runtime-registry section, with the source and field choice it was rendered for.
pub(super) struct RegistryCache {
    source: RegistrySource,
    fields: bool,
    text: String,
}

/// The inputs the report's donor and marker arrangements are derived from.
#[derive(Clone, Copy, PartialEq)]
struct DonorInputs {
    recipe: u64,
    donor: Option<u32>,
    stat_group: Option<u16>,
    catalog: u64,
    catalog_loaded: bool,
}

/// Everything the report text is rendered from.
#[derive(PartialEq)]
struct ReportKey {
    donor: DonorInputs,
    build: Option<std::path::PathBuf>,
    markers: u64,
    registry: RegistrySource,
    fields: bool,
    parts: String,
}

/// The rendered report, rebuilt only when one of its inputs changes.
pub(super) struct ReportCache {
    key: ReportKey,
    arrangements: Option<Vec<u16>>,
    text: String,
    lines: usize,
}

/// Renders the report. Pure, so its content is tested without drawing a frame. Without a staged
/// build it reports the identities the next build will assign. With a donor from the loaded
/// catalog, inherited fields show the donor's value instead of "donor".
pub(super) fn technical_build_report(
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
            for (index, weapon) in build.weapons.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "\n{} {}/{}  {}",
                    weapon.kind.label().to_uppercase(),
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
    out.push_str(art);
    append_recipe(&mut out, recipe);
    append_sockets(&mut out, recipe, donor);
    append_runtime(&mut out, recipe);
    out.push_str(registry);
    append_document(&mut out, recipe);
    out
}

fn hex(value: u32) -> String {
    format!("0x{value:08X}")
}

pub(super) fn field(out: &mut String, name: &str, value: impl std::fmt::Display) {
    let _ = writeln!(out, "  {name:<26}{value}");
}

/// Compact JSON for a value whose type has no display of its own. Everything in a recipe
/// serializes, so this never leaves a field out.
fn json(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| format!("<unserializable: {error}>"))
}

/// Strings longer than this are summarized by length and digest.
const LONG_STRING_BYTES: usize = 96;

/// Compact JSON with every long string summarized as the recipe document summarizes it, so an
/// embedded image stays one short line.
fn json_summary(value: &impl serde::Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(mut value) => {
            summarize_long_strings(&mut value);
            value.to_string()
        }
        Err(error) => format!("<unserializable: {error}>"),
    }
}

fn summarize_long_strings(value: &mut serde_json::Value) {
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

fn long_string_summary(text: &str) -> String {
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
        None => field(out, "  catalog", "not loaded"),
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
        "behavior firing",
        overrides
            .behavior_firing
            .map_or_else(|| "behavior (default)".to_owned(), |firing| json(&firing)),
    );
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
        "exclude from Sunrise badge",
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
            .map_or_else(|| "none".to_owned(), json_summary),
    );
    field(
        out,
        "icon edit",
        if overrides.icon_edit.is_identity() {
            "identity".to_owned()
        } else {
            json_summary(&overrides.icon_edit)
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
            .map_or_else(|| "none".to_owned(), json_summary),
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

/// The named points the appearance's gear art carries: where the weapon is held, where it
/// fires, where a case leaves it. The runtime resolves these by name, so an imported model that
/// kept a donor's marker set aims at the donor's sight rather than its own. Seeing the names and
/// positions side by side is what makes that visible before a build reaches the game.
///
/// Read from the packages rather than the recipe, so it is rendered separately and only while
/// the window is open.
///
/// A marker the recipe moves is listed where the build puts it, with how far it moved.
pub(super) fn marker_section(markers: Markers<'_>, moved: &[MarkerOffsetRecipe]) -> String {
    let mut text = String::new();
    let out = &mut text;
    let sets = match markers {
        None => {
            let _ = writeln!(out, "\nMARKERS  not read");
            return text;
        }
        Some(Err(error)) => {
            let _ = writeln!(out, "\nMARKERS  unavailable: {error}");
            return text;
        }
        Some(Ok(sets)) => sets,
    };
    let all = || sets.iter().flat_map(|set| &set.markers);
    let total = all().count();
    let named = all()
        .filter(|marker| marker_name(marker.name).is_some())
        .count();
    let offset = |name: u32| {
        moved
            .iter()
            .find(|offset| offset.marker.parse_u32().ok() == Some(name))
            .map(|offset| offset.offset_um.map(|um| um as f32 / 1_000_000.0))
    };
    let shifted = all().filter(|marker| offset(marker.name).is_some()).count();
    let _ = writeln!(
        out,
        "\nMARKERS  {total} on {} objects, {named} named, {shifted} moved",
        sets.len()
    );
    if sets.is_empty() {
        field(out, "markers", "this appearance carries none");
        return text;
    }
    for set in sets {
        let _ = writeln!(
            out,
            "  object {}  set {}  ({})",
            hex(set.entity),
            hex(set.component),
            set.markers.len()
        );
        for marker in &set.markers {
            let moved = offset(marker.name);
            let [x, y, z] = moved.map_or(marker.position, |delta| {
                std::array::from_fn(|axis| marker.position[axis] + delta[axis])
            });
            let moved = moved.map_or_else(String::new, |[dx, dy, dz]| {
                format!("  moved {dx:+.5} {dy:+.5} {dz:+.5}")
            });
            let label = column(&marker.label(), 24);
            // A weapon really does carry two markers of one name at one point, differing only
            // in which way they face, so the rotation has to be shown or they read as a bug.
            let facing = if marker.is_aligned() {
                String::new()
            } else {
                let [i, j, k, w] = marker.orientation;
                format!("  facing {i:>7.4} {j:>7.4} {k:>7.4} {w:>7.4}")
            };
            // A marker whose name is unrecovered is still somewhere meaningful. Naming what it
            // sits next to says more than the hash alone, without guessing at the name.
            let near = gear_markers::nearest_named(sets, marker)
                .map_or_else(String::new, |near| format!("  {near}"));
            let _ = writeln!(
                out,
                "    {label:<24}  {x:>10.5} {y:>10.5} {z:>10.5}{moved}{facing}{near}"
            );
        }
    }
    text
}

fn append_recipe(out: &mut String, recipe: &WeaponRecipe) {
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

/// The effective runtime entity this weapon compiles against: the component bindings, the
/// resources they select, and every readable field of every component owner. This is the
/// registry a runtime value or binary patch addresses, so a locator in the recipe above can be
/// matched to the field it points at. The graph already includes the recipe's runtime baseline
/// and its component donors, so it is what the build sees rather than the bare donor.
/// Rendered separately from the rest of the report because it is the one section whose size
/// depends on the packages rather than the recipe: a weapon carries a few thousand readable
/// fields, so this is cached against the graph it came from instead of rebuilt every frame.
pub(super) fn runtime_registry_section(registry: Registry<'_>, include_fields: bool) -> String {
    let mut text = String::new();
    let out = &mut text;
    let graph = match registry {
        None => {
            let _ = writeln!(out, "\nRUNTIME REGISTRY  not read");
            return text;
        }
        Some(Err(error)) => {
            let _ = writeln!(out, "\nRUNTIME REGISTRY  unavailable: {error}");
            return text;
        }
        Some(Ok(graph)) => graph,
    };
    let resource_roots = || {
        graph.resources.iter().flat_map(|resource| {
            std::iter::once(&resource.instance).chain(resource.definition.iter())
        })
    };
    let owner_roots = || graph.owners.iter().flat_map(|owner| &owner.roots);
    let count_fields = |roots: &mut dyn Iterator<Item = &WeaponRuntimeRoot>| {
        roots.map(|root| root.fields.len()).sum::<usize>()
    };
    let _ = writeln!(
        out,
        "\nRUNTIME REGISTRY  entity {}  item {}  pattern {}",
        hex(graph.entity_tag),
        hex(graph.item_hash),
        hex(graph.pattern_global_id_hash)
    );
    field(out, "bindings", graph.bindings.len());
    field(out, "resources", graph.resources.len());
    field(out, "resource fields", count_fields(&mut resource_roots()));
    field(out, "owners", graph.owners.len());
    field(out, "owner-only fields", count_fields(&mut owner_roots()));

    let _ = writeln!(out, "\nRUNTIME BINDINGS  ({})", graph.bindings.len());
    for binding in &graph.bindings {
        let _ = writeln!(
            out,
            "  {}  {:<34}[{}/{}]  owner {}  class {}  +{:#x}",
            hex(binding.binding_hash),
            binding.binding_label,
            binding.resource_index,
            binding.resource_count,
            hex(binding.owner_tag),
            hex(binding.concrete_class),
            binding.resource_offset
        );
    }

    let _ = writeln!(out, "\nRUNTIME RESOURCES  ({})", graph.resources.len());
    for resource in &graph.resources {
        let aliases = if resource.alias_bindings.is_empty() {
            String::new()
        } else {
            format!(
                "  also {}",
                resource
                    .alias_bindings
                    .iter()
                    .map(|(hash, index)| format!("{}#{index}", hex(*hash)))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        let _ = writeln!(
            out,
            "  {}  {:<34}[{}/{}]  owner {}  class {}{aliases}",
            hex(resource.binding_hash),
            resource.binding_label,
            resource.resource_index,
            resource.resource_count,
            hex(resource.owner_tag),
            hex(resource.concrete_class)
        );
        for root in std::iter::once(&resource.instance).chain(resource.definition.iter()) {
            let _ = writeln!(out, "      {}", root_summary(root));
            if include_fields {
                for entry in &root.fields {
                    let _ = writeln!(out, "        {}", registry_field(entry));
                }
            }
        }
    }

    // An owner root keeps only what no bound resource above already covers, so this section is
    // the remainder of each component payload rather than a second copy of it.
    let _ = writeln!(
        out,
        "\nRUNTIME OWNERS  ({})  fields outside every bound resource",
        graph.owners.len()
    );
    for owner in &graph.owners {
        let _ = writeln!(
            out,
            "\n  OWNER {}  anchor {}#{}  roots {}",
            hex(owner.owner_tag),
            hex(owner.anchor_binding_hash),
            owner.anchor_resource_index,
            owner.roots.len()
        );
        for root in &owner.roots {
            let _ = writeln!(out, "    {}", root_summary(root));
            if include_fields {
                for entry in &root.fields {
                    let _ = writeln!(out, "      {}", registry_field(entry));
                }
            }
        }
    }
    if !include_fields {
        let _ = writeln!(out, "\n  Field values omitted.");
    }
    text
}

/// How many readable field values the registry would add, for the control that turns them on.
pub(super) fn runtime_registry_field_count(graph: &WeaponRuntimeGraph) -> usize {
    graph
        .resources
        .iter()
        .flat_map(|resource| std::iter::once(&resource.instance).chain(resource.definition.iter()))
        .chain(graph.owners.iter().flat_map(|owner| &owner.roots))
        .map(|root| root.fields.len())
        .sum()
}

fn root_summary(root: &WeaponRuntimeRoot) -> String {
    // A root's own schema is often not the class its binding selects, so naming it here adds
    // what the binding line cannot: an instance and its definition are different components.
    let schema = match sundial::package_authoring::runtime::native_type_name(root.schema) {
        Some(name) => format!("{} {name}", hex(root.schema)),
        None => hex(root.schema),
    };
    format!(
        "{:<22}schema {:<34}+{:#x}  {} bytes  {} fields{}",
        root.kind.label(),
        schema,
        root.owner_offset,
        root.byte_size,
        root.fields.len(),
        if root.generated_schema {
            "  generated schema"
        } else {
            ""
        }
    )
}

/// One field line: what it is called, what it holds now, and where it lives. The path is the
/// locator's own label, so it matches a runtime value override recorded in the recipe, and is
/// left off when it only repeats the name. Long opaque values are cut so the columns hold;
/// the binary patch editor is the place to read a whole block byte for byte.
fn registry_field(entry: &WeaponRuntimeField) -> String {
    let name = if entry.name_inferred {
        format!("{} (inferred)", entry.name)
    } else {
        entry.name.clone()
    };
    let redundant = !entry.path_label.contains(" / ")
        && (entry.path_label.ends_with(&entry.name) || entry.path_label.ends_with(&name));
    let path = if redundant {
        ""
    } else {
        entry.path_label.as_str()
    };
    let line = format!(
        "{:<40}{:<32}{:<9}{:<15}+{:<10}{path}",
        column(&name, 39),
        column(&registry_value(&entry.kind, &entry.value), 31),
        value_kind_label(&entry.kind),
        field_source_label(entry.source),
        format!("{:#x}", entry.owner_offset)
    );
    line.trim_end().to_owned()
}

/// Pad-or-cut, so one long value cannot push every later column out of line.
fn column(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let kept = text
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    format!("{kept}…")
}

fn registry_value(kind: &WeaponRuntimeValueKind, value: &WeaponRuntimeValue) -> String {
    match value {
        WeaponRuntimeValue::Boolean(value) => value.to_string(),
        WeaponRuntimeValue::Signed(value) => value.to_string(),
        WeaponRuntimeValue::Unsigned(value) => match kind {
            WeaponRuntimeValueKind::HexIdentifier { bits }
            | WeaponRuntimeValueKind::BitFlags { bits } => {
                format!("0x{value:0>width$X}", width = (*bits as usize).div_ceil(4))
            }
            _ => value.to_string(),
        },
        WeaponRuntimeValue::Float32Bits(bits) => f32::from_bits(*bits).to_string(),
        WeaponRuntimeValue::Float64Bits(bits) => f64::from_bits(*bits).to_string(),
        WeaponRuntimeValue::Vector4Float32Bits(bits) => format!(
            "[{}]",
            bits.iter()
                .map(|bits| f32::from_bits(*bits).to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ),
        // A long opaque block is identified, not reproduced; the bytes are in the packages.
        WeaponRuntimeValue::Bytes(bytes) if bytes.len() > 6 => format!(
            "{} +{} bytes",
            super::runtime_fields::format_runtime_bytes(&bytes[..6]),
            bytes.len() - 6
        ),
        WeaponRuntimeValue::Bytes(bytes) => super::runtime_fields::format_runtime_bytes(bytes),
    }
}

fn value_kind_label(kind: &WeaponRuntimeValueKind) -> String {
    match kind {
        WeaponRuntimeValueKind::Boolean => "bool".to_owned(),
        WeaponRuntimeValueKind::SignedInteger { bits } => format!("i{bits}"),
        WeaponRuntimeValueKind::UnsignedInteger { bits } => format!("u{bits}"),
        WeaponRuntimeValueKind::Enum { bits } => format!("enum{bits}"),
        WeaponRuntimeValueKind::BitFlags { bits } => format!("flags{bits}"),
        WeaponRuntimeValueKind::HexIdentifier { bits } => format!("hex{bits}"),
        WeaponRuntimeValueKind::Float32 => "f32".to_owned(),
        WeaponRuntimeValueKind::Float64 => "f64".to_owned(),
        WeaponRuntimeValueKind::Vector4Float32 => "vec4".to_owned(),
        WeaponRuntimeValueKind::FixedBytes { size } => format!("bytes{size}"),
    }
}

fn field_source_label(source: WeaponRuntimeFieldSource) -> &'static str {
    match source {
        WeaponRuntimeFieldSource::GeneratedSchema => "generated",
        WeaponRuntimeFieldSource::NativeMember => "native member",
        WeaponRuntimeFieldSource::OpaqueNativeType => "opaque type",
        WeaponRuntimeFieldSource::NativeDeclaration => "native decl",
    }
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
        serde_json::Value::String(text) if text.len() > LONG_STRING_BYTES => {
            let _ = writeln!(out, "{indent}{label}{}", long_string_summary(text));
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

/// The packages read that backs the MARKERS section. Held so its package handles can be
/// released before an installation, the same rule the runtime-graph job follows.
pub(super) struct MarkerJob {
    arrangements: Vec<u16>,
    receiver: std::sync::mpsc::Receiver<Result<Vec<MarkerSet>, String>>,
    worker: std::thread::JoinHandle<()>,
}

impl super::PackageAuthoringApp {
    pub(super) fn technical_markers_busy(&self) -> bool {
        self.technical_marker_job.is_some()
    }

    /// The arrangements the report describes: the recipe's override when it has one, else the
    /// geometry donor's. `None` while neither is known. Markers belong to the art, so this is the only
    /// thing the read depends on.
    pub(super) fn technical_marker_arrangements(
        &self,
        donor: Option<&WeaponDonor>,
    ) -> Option<Vec<u16>> {
        let mut rows: Vec<u16> = match self.recipe.overrides.art_arrangements.as_ref() {
            Some(rows) => rows.iter().map(|row| row.arrangement).collect(),
            None => donor?
                .art_arrangements
                .iter()
                .map(|row| row.arrangement)
                .collect(),
        };
        rows.sort_unstable();
        rows.dedup();
        Some(rows)
    }

    /// Starts the read when the arrangement on screen has no result yet. Only called while the
    /// window is open, so a closed window costs nothing. One read runs at a time: a replaced
    /// job would leave its thread holding package handles. The poll keeps a stale result and
    /// the next frame starts the read for the new arrangement.
    pub(super) fn ensure_technical_markers(&mut self, ctx: &egui::Context, arrangements: &[u16]) {
        if self.technical_marker_job.is_some()
            || self
                .technical_markers
                .as_ref()
                .is_some_and(|(read, _)| read == arrangements)
        {
            return;
        }
        if arrangements.is_empty() {
            self.technical_markers = Some((Vec::new(), Ok(Vec::new())));
            self.technical_marker_revision = self.technical_marker_revision.wrapping_add(1);
            return;
        }
        let packages = self.packages.clone();
        let wanted = arrangements.to_vec();
        let (sender, receiver) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let worker = {
            let wanted = wanted.clone();
            std::thread::spawn(move || {
                let mut sets = Vec::new();
                let mut result = Ok(());
                for arrangement in wanted {
                    match sundial::package_authoring::gear_markers::read_appearance(
                        &packages,
                        arrangement,
                    ) {
                        Ok(read) => sets.extend(read),
                        Err(error) => {
                            result = Err(format!("arrangement {arrangement}: {error}"));
                            break;
                        }
                    }
                }
                let _ = sender.send(result.map(|()| sets));
                ctx.request_repaint();
            })
        };
        self.technical_marker_job = Some(MarkerJob {
            arrangements: wanted,
            receiver,
            worker,
        });
    }

    pub(super) fn poll_technical_markers(&mut self) {
        let Some(job) = &self.technical_marker_job else {
            return;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("The marker reader stopped without a result".to_owned())
            }
        };
        let MarkerJob {
            arrangements,
            worker,
            ..
        } = self.technical_marker_job.take().expect("job was checked");
        // Join even a stale read: its package handles must be released before installation.
        let completion = worker.join();
        self.technical_markers = Some((
            arrangements,
            if completion.is_err() {
                Err("The marker reader panicked while reading packages".to_owned())
            } else {
                result
            },
        ));
        self.technical_marker_revision = self.technical_marker_revision.wrapping_add(1);
    }

    /// Draws the window when the preference is on, with or without a staged build.
    pub(super) fn draw_technical_build_window(&mut self, ctx: &egui::Context) {
        if !self.show_technical_build {
            self.technical_build_open = false;
            return;
        }
        if !self.technical_build_open {
            return;
        }
        // A window drawn earlier this frame may have edited the recipe. Observing it here keeps
        // the cached report from lagging the recipe on screen.
        self.synchronize_recipe_dirty();
        let inputs = DonorInputs {
            recipe: self.recipe_revision,
            donor: self.recipe.donor.item_hash.parse_u32().ok(),
            stat_group: self.recipe.overrides.stat_group_index,
            catalog: self.catalog_revision,
            catalog_loaded: self.catalog.is_some(),
        };
        // The donor is resolved only when the report has to be rendered again.
        let mut donor = None;
        let arrangements = match self
            .technical_report
            .as_ref()
            .filter(|report| report.key.donor == inputs)
        {
            Some(report) => report.arrangements.clone(),
            None => {
                let current = self.current_donor();
                // Markers belong to the art the weapon wears, the appearance's when it has one.
                let arrangements =
                    self.technical_marker_arrangements(self.current_geometry_donor().as_ref());
                donor = Some(current);
                arrangements
            }
        };
        // Started before anything borrows the app, so the read is already in flight by the
        // time the rest of the report is assembled. No read starts while an installation
        // replaces packages.
        if self.install_receiver.is_none()
            && let Some(arrangements) = &arrangements
        {
            self.ensure_technical_markers(ctx, arrangements);
        }
        // The rig, hold and type the part rows choose. It can start the type read.
        let parts = self.technical_parts(ctx);
        let build = self
            .latest_build
            .as_ref()
            .and_then(|build| build.as_ref().ok());
        // Only the graph scanned for the recipe on screen. A stale one would describe a
        // different runtime baseline than every other section of this report.
        let target = self.runtime_graph_target.as_ref();
        let graph = self
            .runtime_graph
            .as_ref()
            .and_then(|(key, graph)| (Some(key) == target).then_some(graph));
        let registry = match graph {
            Some(graph) => Some(Ok(graph.as_ref())),
            None => self
                .runtime_graph_error
                .as_ref()
                .and_then(|(key, error)| (Some(key) == target).then_some(Err(error.as_str()))),
        };
        let fields = graph.map_or(0, |graph| runtime_registry_field_count(graph));
        // A weapon carries a few thousand readable fields. Rendering them on every frame would
        // cost more than the rest of the report put together, so the section is kept until the
        // graph, the scan result or the choice changes.
        let source = match (graph, registry) {
            (Some(graph), _) => RegistrySource::Graph(std::sync::Arc::downgrade(graph)),
            (None, Some(Err(error))) => RegistrySource::Failed(error.to_owned()),
            (None, _) => RegistrySource::Pending,
        };
        if self.technical_registry.as_ref().is_none_or(|cache| {
            cache.source != source || cache.fields != self.technical_registry_fields
        }) {
            self.technical_registry = Some(RegistryCache {
                text: runtime_registry_section(registry, self.technical_registry_fields),
                source: source.clone(),
                fields: self.technical_registry_fields,
            });
        }
        // The whole report is about a megabyte of text with the field values on, so it is
        // rendered again only when something it reads has changed.
        let key = ReportKey {
            donor: inputs,
            build: build.map(|build| build.run_directory.clone()),
            markers: self.technical_marker_revision,
            registry: source,
            fields: self.technical_registry_fields,
            parts,
        };
        if self
            .technical_report
            .as_ref()
            .is_none_or(|report| report.key != key)
        {
            let donor = donor.unwrap_or_else(|| self.current_donor());
            let section = self
                .technical_registry
                .as_ref()
                .map_or("", |cache| cache.text.as_str());
            let markers = marker_section(
                arrangements.as_ref().and_then(|arrangements| {
                    let (read, result) = self.technical_markers.as_ref()?;
                    (read == arrangements).then_some(match result {
                        Ok(sets) => Ok(sets.as_slice()),
                        Err(error) => Err(error.as_str()),
                    })
                }),
                &self.recipe.overrides.marker_offsets,
            );
            let art = format!("{}{markers}", key.parts);
            let text = technical_build_report(build, &self.recipe, donor.as_ref(), &art, section);
            let lines = text.lines().count();
            self.technical_report = Some(ReportCache {
                key,
                arrangements,
                text,
                lines,
            });
        }
        let Some(report) = self.technical_report.as_ref() else {
            return;
        };
        let mut open = self.technical_build_open;
        let mut fields_shown = self.technical_registry_fields;
        egui::Window::new("Technical Build")
            .open(&mut open)
            .default_width(760.0)
            .default_height(520.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Copy Everything").clicked() {
                        ui.ctx().copy_text(report.text.clone());
                    }
                    ui.add_enabled(
                        fields > 0,
                        egui::Checkbox::new(&mut fields_shown, "Field Values"),
                    )
                    .on_hover_text(format!(
                        "Add every readable runtime field value ({fields}) to the registry."
                    ))
                    .on_disabled_hover_text("No runtime registry has been read for this weapon.");
                    ui.weak(format!("{} lines", report.lines));
                });
                ui.separator();
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Selectable so a single figure can be lifted out without copying it all.
                        ui.add(
                            egui::TextEdit::multiline(&mut report.text.as_str())
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .code_editor(),
                        );
                    });
            });
        if fields_shown != self.technical_registry_fields {
            ctx.request_repaint();
        }
        self.technical_build_open = open;
        self.technical_registry_fields = fields_shown;
    }
}

#[cfg(test)]
mod tests;
