//! Saved source ability candidates using private native graphs and property grants.
use std::{collections::BTreeSet, fs, path::Path};

use parhelion_import::{
    d1_roi::{AbilityChoice, Source, ability::Translation, packages_path},
    tiger::reader::{Reader, outside, package_stamp},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sundial::{
    investment::{InvestmentCatalog, SubclassSummary},
    package_authoring::{
        ability_bank::AbilityTarget, open_shadowkeep_package_manager, resolve_live_named_tag,
        sandbox_perk::load_sandbox_perk_runtime_action,
    },
};

use super::{EntryEdits, StockModifier, SubclassChoice, layout};
use crate::{ItemKind, WeaponRecipe, perk::PerkRecipe};
mod revival;
pub use revival::prepare as prepare_revival;

pub struct Request<'a> {
    pub source_packages: &'a Path,
    pub native_packages: &'a Path,
    pub output: &'a Path,
    pub recipe: &'a WeaponRecipe,
    pub choice: &'a AbilityChoice,
    pub entry: u8,
}

/// The complete baseline protects all draft fields from changes made during preparation.
pub struct Prepared {
    baseline: WeaponRecipe,
    recipe: WeaponRecipe,
    pub provenance: Value,
}

impl Prepared {
    #[must_use]
    pub fn recipe(&self) -> &WeaponRecipe {
        &self.recipe
    }

    pub fn apply(&self, recipe: &mut WeaponRecipe) -> Result<(), String> {
        if *recipe != self.baseline {
            return Err(
                "The subclass changed after import preparation. Prepare the import again.".into(),
            );
        }
        *recipe = self.recipe.clone();
        Ok(())
    }
}

fn error(error: impl std::fmt::Display) -> String {
    format!("{error:#}")
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Resolve by the translated row and graph. Names are presentation and never establish that
/// a native controller can consume the source properties.
fn donor<'a>(
    request: &Request<'_>,
    catalog: &InvestmentCatalog,
    subclasses: &'a [SubclassSummary],
    translated: &Translation,
) -> Result<(&'a SubclassSummary, u8), String> {
    let base = request.recipe.donor.item_hash.parse_u32().map_err(error)?;
    let base = subclasses
        .iter()
        .find(|candidate| candidate.hash == base)
        .ok_or("The subclass base is absent from the native stock catalog")?;
    let row = catalog
        .ability_row(translated.native_row)
        .ok_or("The imported movement controller has no native ability row")?;
    if row.entity != Some(translated.native_graph)
        || row.bank != Some(translated.native_bank)
        || row.slot != Some(AbilityTarget::Jump)
    {
        return Err("The native catalog and translated movement controller disagree".into());
    }
    let current = request
        .recipe
        .overrides
        .subclass_abilities
        .as_ref()
        .map(|abilities| abilities.ability(base.hash, request.entry))
        .unwrap_or_else(|| SubclassChoice::stock(request.entry, base.hash, request.entry));
    let mut candidates = Vec::new();
    for candidate in subclasses {
        for &entry in &layout::MOVEMENT {
            if candidate.entry_rows.get(&entry) != Some(&translated.native_row)
                || candidate.entry_entities.get(&entry) != Some(&translated.native_graph)
            {
                continue;
            }
            let preference = if candidate.hash == current.source && entry == current.source_entry {
                0
            } else if candidate.hash == base.hash && entry == request.entry {
                1
            } else if candidate.hash == base.hash {
                2
            } else if candidate.class_type == base.class_type {
                3
            } else {
                4
            };
            candidates.push((preference, candidate.hash, entry, candidate));
        }
    }
    candidates.sort_by_key(|&(preference, hash, entry, _)| (preference, hash, entry));
    candidates
        .first()
        .map(|&(_, _, entry, subclass)| (subclass, entry))
        .ok_or_else(|| "No native movement entry equips the validated controller".into())
}

/// Prepare a complete movement property group without changing the recipe or installed files.
/// The imported choice retains native input handling, animation and the controller's base state.
pub fn prepare(request: &Request<'_>) -> Result<Prepared, String> {
    if request.recipe.kind != ItemKind::Subclass {
        return Err("An ability import requires a subclass recipe".into());
    }
    if !layout::MOVEMENT.contains(&request.entry) {
        return Err("This source ability requires a movement entry".into());
    }
    request
        .recipe
        .to_spec()
        .map_err(error)?
        .validate()
        .map_err(error)?;
    let source_path = packages_path(request.source_packages).map_err(error)?;
    let native_path = request.native_packages.canonicalize().map_err(error)?;
    let output =
        outside(request.output, source_path.parent().unwrap_or(&source_path)).map_err(error)?;
    outside(&output, native_path.parent().unwrap_or(&native_path)).map_err(error)?;
    if output.exists() && fs::read_dir(&output).map_err(error)?.next().is_some() {
        return Err("Choose a new ability import output folder".into());
    }
    fs::create_dir_all(&output).map_err(error)?;
    let native_stamp = package_stamp(&native_path).map_err(error)?;
    let source = Source::open(&source_path, |_, _| {}).map_err(error)?;
    let mut native = Reader::new(&native_path, &output.join("native"), false).map_err(error)?;
    let manager = open_shadowkeep_package_manager(&native_path)?;
    let translated = source
        .prepare_ability(&mut native, request.choice)
        .map_err(error)?;
    let catalog = InvestmentCatalog::load_with_cache_path(
        native_path
            .parent()
            .ok_or("The native packages have no installation parent")?,
        &output.join("native-catalog.json"),
        true,
        |_| {},
    )?;
    let subclasses = catalog.subclasses(crate::package_profile::is_stock_item_definition);
    let (donor, source_entry) = donor(request, &catalog, &subclasses, &translated)?;
    let globals_tag = resolve_live_named_tag(&manager, "investment_globals", None)?;
    let globals = native.tag(globals_tag.0, None).map_err(error)?;
    let source_perk = catalog
        .item_sandbox_perk_indices(crate::perk::DEFAULT_PLUG_LAYOUT)
        .into_iter()
        .find(|&index| {
            load_sandbox_perk_runtime_action(&manager, &globals.0, usize::from(index)).is_ok()
        })
        .ok_or("The native private-perk template has no usable action")?;
    let mut effect = PerkRecipe::effect(source_perk);
    effect.program = Some(translated.program);
    let mut perk = PerkRecipe::new();
    perk.id = digest(
        &serde_json::to_vec(&json!({
            "source":"d1_roi", "namespace":request.recipe.namespace,
            "entry":request.entry, "choice":request.choice,
        }))
        .map_err(error)?,
    );
    perk.name = translated.choice.name.clone();
    perk.description = translated.choice.description.clone();
    perk.effects.push(effect);
    // The chosen native entry contributes its controller only. Leaving its stock keys or
    // perks active would mix donor behavior with the source group, including gated rows.
    let removed_perks = donor
        .entry_perks
        .get(&source_entry)
        .into_iter()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let removed_modifiers = donor
        .entry_modifiers
        .get(&source_entry)
        .into_iter()
        .flatten()
        .map(|&(key, row)| StockModifier { key, row })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let choice = SubclassChoice {
        entry: request.entry,
        source: donor.hash,
        source_entry,
        edits: EntryEdits {
            name: Some(translated.choice.name.clone()),
            description: (!translated.choice.description.trim().is_empty())
                .then(|| translated.choice.description.clone()),
            removed_perks,
            removed_modifiers,
            custom_perks: vec![perk],
            ..EntryEdits::default()
        },
    };
    let mut recipe = request.recipe.clone();
    recipe
        .overrides
        .subclass_abilities
        .get_or_insert_default()
        .set_ability(
            request.recipe.donor.item_hash.parse_u32().map_err(error)?,
            choice.clone(),
        );
    recipe.to_spec().map_err(error)?.validate().map_err(error)?;
    if package_stamp(&native_path).map_err(error)? != native_stamp
        || package_stamp(&source_path).map_err(error)? != source.report().stamp
    {
        return Err("The package inputs changed during ability import preparation".into());
    }
    native.finish().map_err(error)?;
    let encoded = recipe.to_json_pretty().map_err(error)?;
    let provenance = json!({
        "status":"prepared movement property candidate", "gameplay_verified":false,
        "prepared_recipe_sha256":digest(encoded.as_bytes()),
        "baseline_recipe_sha256":digest(request.recipe.to_json_pretty().map_err(error)?.as_bytes()),
        "source":source.report(), "translation":translated.evidence,
        "native":{"packages":native_path,"stamp":native_stamp,"entry":request.entry,
            "source_subclass":donor.hash,"source_entry":source_entry,
            "pattern":translated.native_pattern,"row":translated.native_row,
            "graph":translated.native_graph,"bank":translated.native_bank,
            "selected_source_keys":translated.selected_keys,
            "removed_perks":choice.edits.removed_perks,
            "removed_modifiers":choice.edits.removed_modifiers},
        "limits":translated.limits,
    });
    fs::write(output.join("prepared-subclass.parhelion.json"), encoded).map_err(error)?;
    fs::write(
        output.join("provenance.json"),
        serde_json::to_vec_pretty(&provenance).map_err(error)?,
    )
    .map_err(error)?;
    Ok(Prepared {
        baseline: request.recipe.clone(),
        recipe,
        provenance,
    })
}
