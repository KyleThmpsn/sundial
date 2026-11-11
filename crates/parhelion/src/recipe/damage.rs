//! Keep a Rise of Iron weapon's private damage talent and fixed carrier in agreement.
use super::*;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
use sundial::package_authoring::sandbox_perk::program::{Action, Program};

#[derive(Deserialize)]
struct Talent {
    damage_type: RecipeDamageType,
    name: String,
    description: String,
    icon: crate::perk::Icon,
    program: Program,
}

/// The four plain damage talents, including their original Rise of Iron labels and artwork.
/// Their programs use the source's unconditional activation and native SetDamageType equivalent.
fn talents() -> &'static [Talent] {
    static TALENTS: OnceLock<Vec<Talent>> = OnceLock::new();
    TALENTS.get_or_init(|| {
        serde_json::from_str(include_str!("damage/d1.json")).expect("Embedded D1 damage talents")
    })
}

struct Source {
    original: RecipeDamageType,
    slots: std::collections::BTreeMap<(u16, u32), BTreeSet<String>>,
}

type Sources = std::collections::BTreeMap<(PathBuf, String), Option<Arc<Source>>>;

/// The pinned graph owns source provenance and identifies the translated socket choices. A name
/// alone cannot authorize changing a user-authored perk that happens to have the same label.
fn source(overrides: &WeaponRecipeOverrides) -> Result<Option<Arc<Source>>, String> {
    let Some(reference) = &overrides.imported_graph else {
        return Ok(None);
    };
    static SOURCES: OnceLock<Mutex<Sources>> = OnceLock::new();
    let cache = SOURCES.get_or_init(Mutex::default);
    let key = (reference.directory.clone(), reference.sha256.clone());
    if let Some(value) = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key)
    {
        return Ok(value.clone());
    }
    let bytes = std::fs::read(reference.directory.join("asset-graph.json"))
        .map_err(|e| format!("Read imported damage talents: {e}"))?;
    let graph: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("Read imported damage talents: {e}"))?;
    let result = if graph["source"] == "d1_roi" && graph["kind"] == "weapon" {
        if graph["perk_translation"]["damage_type"].is_null() {
            return Ok(None);
        }
        let original = serde_json::from_value(graph["perk_translation"]["damage_type"].clone())
            .map_err(|_| "Imported weapon has an unsupported damage type.".to_owned())?;
        let mut slots = std::collections::BTreeMap::<_, BTreeSet<String>>::new();
        for step in graph["perk_translation"]["translated"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if !step["damage_choice"].is_null() || talents().iter().any(|t| step["name"] == t.name)
            {
                let Some(slot) = step["socket"].as_u64().and_then(|n| u16::try_from(n).ok()) else {
                    continue;
                };
                let Some(plug) = step["template_plug"].as_str() else {
                    continue;
                };
                let plug = u32::from_str_radix(plug, 16)
                    .map_err(|e| format!("Imported damage talent template: {e}"))?;
                if let Some(name) = step["name"].as_str() {
                    slots
                        .entry((slot, plug))
                        .or_default()
                        .insert(name.to_owned());
                }
            }
        }
        Some(Arc::new(Source { original, slots }))
    } else {
        None
    };
    cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key, result.clone());
    Ok(result)
}

pub(crate) fn original(overrides: &WeaponRecipeOverrides) -> Option<RecipeDamageType> {
    source(overrides)
        .ok()
        .flatten()
        .map(|source| source.original)
}

fn modes(variant: &WeaponSocketPlugVariantRecipe) -> impl Iterator<Item = u8> + '_ {
    variant
        .sandbox_perks
        .iter()
        .filter_map(|perk| perk.program.as_ref())
        .flat_map(|program| &program.actions)
        .filter_map(|action| {
            if let Action::SetDamageType { mode, .. } = action {
                Some(mode.byte())
            } else {
                None
            }
        })
}

fn effect(talent: &Talent, index: u16) -> WeaponSandboxPerkRuntimeRecipe {
    WeaponSandboxPerkRuntimeRecipe {
        program: Some(talent.program.clone()),
        source_perk_index: index,
        projectiles: Vec::new(),
        activation: None,
        runtime_values: Vec::new(),
        action_float_values: Vec::new(),
    }
}

fn retarget(variant: &mut WeaponSocketPlugVariantRecipe, talent: &Talent) -> Result<(), String> {
    let Some(Action::SetDamageType { mode: target, .. }) = talent.program.actions.first() else {
        return Err("The D1 damage talent has no native damage effect.".into());
    };
    let mut found = false;
    for perk in &mut variant.sandbox_perks {
        if let Some(program) = &mut perk.program {
            for action in &mut program.actions {
                if let Action::SetDamageType { mode, .. } = action {
                    *mode = *target;
                    found = true;
                }
            }
            if talents().iter().any(|talent| talent.name == program.name) {
                program.name = talent.name.clone();
            }
        }
    }
    if !found {
        if variant.sandbox_perks.len() >= crate::perk::SANDBOX_PERK_CAPACITY {
            return Err("The damage talent has no room for its element effect.".into());
        }
        let carrier = crate::perk::EFFECT_CARRIERS
            .into_iter()
            .find(|index| {
                !variant
                    .sandbox_perks
                    .iter()
                    .any(|p| p.source_perk_index == *index)
            })
            .ok_or("The damage talent has no unused effect carrier.")?;
        variant.sandbox_perks.push(effect(talent, carrier));
    }
    // A signature trait can also set the element. It keeps its own text and art.
    if variant
        .name
        .as_deref()
        .is_none_or(|name| talents().iter().any(|t| t.name == name))
    {
        variant.name = Some(talent.name.clone());
        variant.description = Some(talent.description.clone());
        variant.icon = Some(talent.icon.clone());
    }
    Ok(())
}

fn select_alternative(
    overrides: &mut WeaponRecipeOverrides,
    slot: u16,
    members: &[usize],
    talent: &Talent,
) -> Result<(), String> {
    // Source alternatives remain alternatives. Pick the requested one as the default,
    // copying the current default's other contributions when the source lacks that element.
    let column = overrides
        .socket_columns
        .get(usize::from(slot))
        .and_then(Option::as_ref)
        .ok_or("Damage alternatives have no socket")?;
    if !column.choice_conditions.is_empty()
        || !column.choice_weight_bits.is_empty()
        || column.reusable_choices.is_some()
        || !column.randomized_selection_program.is_empty()
    {
        return Err("Damage alternatives have an unsupported conditional layout.".into());
    }
    let wanted = members
        .iter()
        .copied()
        .find(|&index| modes(&overrides.socket_plug_variants[index]).eq(modes_from(talent)));
    let wanted = match wanted {
        Some(index) => index,
        None => {
            let column = overrides
                .socket_columns
                .get_mut(usize::from(slot))
                .and_then(Option::as_mut)
                .ok_or("Damage alternatives have no socket")?;
            let first = *members
                .iter()
                .find(|&&index| overrides.socket_plug_variants[index].choice_index == 0)
                .ok_or("Damage socket has no default talent")?;
            let mut variant = overrides.socket_plug_variants[first].clone();
            variant.choice_index =
                u16::try_from(column.choices.len()).map_err(|e| e.to_string())?;
            retarget(&mut variant, talent)?;
            column.choices.push(variant.source_plug_hash.clone());
            overrides.socket_plug_variants.push(variant);
            overrides.socket_plug_variants.len() - 1
        }
    };
    let first = *members
        .iter()
        .find(|&&index| overrides.socket_plug_variants[index].choice_index == 0)
        .ok_or("Damage socket has no default talent")?;
    let choice = overrides.socket_plug_variants[wanted].choice_index;
    overrides.socket_plug_variants[first].choice_index = choice;
    overrides.socket_plug_variants[wanted].choice_index = 0;
    let column = overrides
        .socket_columns
        .get_mut(usize::from(slot))
        .and_then(Option::as_mut)
        .ok_or("Damage alternatives have no socket")?;
    column.choices.swap(0, usize::from(choice));
    Ok(())
}

fn update(overrides: &mut WeaponRecipeOverrides, source: &Source) -> Result<(), String> {
    let selected = overrides.modern_damage_type.unwrap_or(source.original);
    let talent = talents()
        .iter()
        .find(|talent| talent.damage_type == selected)
        .ok_or("Unsupported D1 damage talent")?;
    let mut slots = source.slots.clone();
    if let Some(slot) = overrides.damage_talent_socket {
        slots.insert(
            (slot, crate::perk::DEFAULT_PLUG_LAYOUT),
            talents().iter().map(|talent| talent.name.clone()).collect(),
        );
    }
    let mut retained = false;
    for ((slot, plug), names) in slots {
        let members = overrides
            .socket_plug_variants
            .iter()
            .enumerate()
            .filter(|(_, variant)| {
                variant.socket_index == slot
                    && variant.source_plug_hash.parse_u32().ok() == Some(plug)
                    && !variant.offer_everywhere
                    && overrides
                        .socket_columns
                        .get(usize::from(slot))
                        .and_then(Option::as_ref)
                        .and_then(|column| column.choices.get(usize::from(variant.choice_index)))
                        == Some(&variant.source_plug_hash)
                    && variant.name.as_ref().is_some_and(|name| {
                        names.contains(name) || talents().iter().any(|talent| &talent.name == name)
                    })
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if members.is_empty() {
            continue;
        }
        retained = true;
        let distinct = members
            .iter()
            .flat_map(|&index| modes(&overrides.socket_plug_variants[index]))
            .collect::<BTreeSet<_>>();
        if distinct.len() > 1 {
            select_alternative(overrides, slot, &members, talent)?;
        } else {
            for index in members {
                retarget(&mut overrides.socket_plug_variants[index], talent)?;
            }
        }
    }
    if !retained && (selected != source.original || overrides.damage_talent_socket.is_some()) {
        let slot = u16::try_from(overrides.socket_columns.len()).map_err(|e| e.to_string())?;
        if usize::from(slot) >= sundial::investment::MAX_WEAPON_SOCKETS {
            return Err("The weapon has no room for a damage talent.".into());
        }
        let mut variant = WeaponSocketPlugVariantRecipe {
            projectile: None,
            replace_effects: true,
            investment_stats: Vec::new(),
            socket_index: slot,
            choice_index: 0,
            source_plug_hash: crate::perk::DEFAULT_PLUG_LAYOUT.into(),
            name: None,
            classification_donor_hash: None,
            icon: None,
            description: None,
            offer_everywhere: false,
            additional_sandbox_perks: Vec::new(),
            sandbox_perks: vec![effect(talent, crate::perk::EFFECT_CARRIERS[0])],
        };
        retarget(&mut variant, talent)?;
        overrides
            .socket_columns
            .push(Some(WeaponSocketColumnRecipe {
                choices: vec![variant.source_plug_hash.clone()],
                socket_type: Some(92),
                ..Default::default()
            }));
        overrides.socket_plug_variants.push(variant);
        overrides.damage_talent_socket = Some(slot);
    }
    overrides.modern_damage_type = Some(selected);
    Ok(())
}

fn modes_from(talent: &Talent) -> impl Iterator<Item = u8> + '_ {
    talent.program.actions.iter().filter_map(|action| {
        if let Action::SetDamageType { mode, .. } = action {
            Some(mode.byte())
        } else {
            None
        }
    })
}

/// Apply transactionally, so an unsupported source layout leaves the draft intact.
pub(crate) fn apply(recipe: &mut WeaponRecipe) -> Result<(), String> {
    if recipe.kind != ItemKind::Weapon
        || recipe.overrides.variable_damage.is_some()
        || recipe.overrides.modern_damage_type.is_none()
    {
        return Ok(());
    }
    let Some(source) = source(&recipe.overrides)? else {
        return Ok(());
    };
    let mut overrides = recipe.overrides.clone();
    update(&mut overrides, &source)?;
    recipe.overrides = overrides;
    Ok(())
}
