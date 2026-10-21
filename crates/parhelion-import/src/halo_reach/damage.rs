//! Source model damage sections, state transitions and detached-piece contracts.
use super::cache::{Cache, Tag};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize)]
pub(super) struct Region {
    pub name: String,
    pub source_permutation: usize,
    pub source_alternatives: usize,
    pub selected_probability: f32,
    /// Unreferenced gaps retain the previous form. Actual transitions must name
    /// an explicit source state, so gap filling does not invent a runtime rule.
    pub states: [String; 5],
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Group {
    pub region: usize,
    pub permutation: String,
    pub states: Vec<u8>,
    pub sections: BTreeSet<usize>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct Geometry {
    pub regions: Vec<Region>,
    pub groups: Vec<Group>,
}

impl Geometry {
    pub fn group(&self, primitive: &super::model::Primitive) -> Result<u16> {
        ensure!(
            primitive.instance.is_none(),
            "Damage selection of model instances requires an explicit translation"
        );
        let groups = self
            .groups
            .iter()
            .enumerate()
            .filter(|(_, group)| group.sections.contains(&primitive.section))
            .map(|(index, _)| index + 1)
            .collect::<Vec<_>>();
        ensure!(
            groups.len() == 1,
            "Source damage draw has an absent or ambiguous region"
        );
        Ok(u16::try_from(groups[0])?)
    }
}

/// Include every reachable source damage permutation in the same rig and palette.
pub(super) fn expand(
    cache: &Cache,
    pages: &mut super::resource::Pages,
    scene: &mut super::Scene,
    request: &super::ExportRequest,
) -> Result<()> {
    if scene.object.tag.group != "vehi" {
        return Ok(());
    }
    let variant = super::selected_variant(&scene.object, request.variant.as_deref())?
        .context("Source vehicle has no damage variant")?;
    let model = &scene.models[0].model;
    let mut geometry = Geometry::default();
    let mut additional = BTreeMap::new();
    for region in &model.regions {
        let initial = model
            .selected
            .get(&region.name)
            .context("Initial source region selection")?;
        let permutations = variant
            .regions
            .get(&region.name)
            .context("Source damage variant region")?;
        let (source_permutation, selected) =
            super::object::select_permutation(permutations, Some(initial)).with_context(|| {
                format!(
                    "Source damage permutation {} / {initial} is absent",
                    region.name
                )
            })?;
        for section in scene.object.gameplay["damage"]["sections"]
            .as_array()
            .context("Source damage sections")?
        {
            for response in section["responses"]
                .as_array()
                .context("Source damage responses")?
            {
                for transition in response["regions"]
                    .as_array()
                    .context("Source region transitions")?
                {
                    if transition["region"] == region.name {
                        let state = transition["state"]
                            .as_i64()
                            .context("Source damage state ordinal")?;
                        ensure!(
                            state == 0
                                || selected.states.iter().any(|s| i64::from(s.state) == state),
                            "Source transition targets an undeclared damage state"
                        );
                    }
                }
            }
        }
        let mut states = std::array::from_fn::<_, 5, _>(|_| initial.clone());
        let mut previous = initial.clone();
        for (ordinal, state) in states.iter_mut().enumerate() {
            let options = selected
                .states
                .iter()
                .filter(|s| usize::try_from(s.state).ok() == Some(ordinal))
                .collect::<Vec<_>>();
            ensure!(
                options.len() <= 1,
                "Random damage permutations require an explicit native selection rule"
            );
            if let Some(option) = options.first() {
                if option.runtime_index < 0 {
                    ensure!(
                        option.name.is_empty(),
                        "Hidden source damage state names a permutation"
                    );
                    previous.clear();
                } else {
                    let permutation = region
                        .permutations
                        .get(option.runtime_index as usize)
                        .context("Source damage permutation index")?;
                    ensure!(
                        permutation.name == option.name,
                        "Source damage permutation name and index disagree"
                    );
                    previous.clone_from(&option.name);
                }
            }
            state.clone_from(&previous);
        }
        ensure!(
            &states[0] == initial,
            "Initial source model differs from its intact damage state"
        );
        let names = states
            .iter()
            .filter(|name| !name.is_empty())
            .cloned()
            .collect::<BTreeSet<_>>();
        let region_index = geometry.regions.len();
        for name in &names {
            let permutation = region
                .permutations
                .iter()
                .find(|p| &p.name == name)
                .context("Source damage permutation")?;
            ensure!(
                permutation.instance_mask == [0; 4],
                "Instanced damage permutation requires an explicit translation"
            );
            geometry.groups.push(Group {
                region: region_index,
                permutation: name.clone(),
                states: states
                    .iter()
                    .enumerate()
                    .filter_map(|(i, value)| (value == name).then_some(i as u8))
                    .collect(),
                sections: permutation.sections.iter().copied().collect(),
            });
        }
        additional.insert(region.name.clone(), names);
        geometry.regions.push(Region {
            name: region.name.clone(),
            source_permutation,
            source_alternatives: permutations.len(),
            selected_probability: selected.probability,
            states,
        });
    }
    ensure!(
        !geometry.regions.is_empty() && geometry.groups.len() < i16::MAX as usize,
        "Source vehicle damage geometry is empty or too large"
    );
    let expanded =
        super::model::read_permutations(cache, pages, &model.tag, &model.selected, &additional)?;
    for primitive in &expanded.primitives {
        geometry.group(primitive)?;
    }
    scene.models[0].model = expanded;
    scene.damage = Some(geometry);
    Ok(())
}

fn name(cache: &Cache, at: usize) -> Result<String> {
    cache.string_id(cache.u32(at)?)
}

fn fraction(cache: &Cache, at: usize) -> Result<f32> {
    let value = cache.f32(at)?;
    ensure!(
        value.is_finite() && (0.0..=1.0).contains(&value),
        "Invalid source damage fraction"
    );
    Ok(value)
}

fn number(cache: &Cache, at: usize) -> Result<f32> {
    let value = cache.f32(at)?;
    ensure!(
        value.is_finite() && value >= 0.,
        "Invalid source damage scalar"
    );
    Ok(value)
}

fn transfers(cache: &Cache, at: usize, sections: usize) -> Result<Vec<Value>> {
    cache
        .block(at, 0x10)?
        .into_iter()
        .map(|row| {
            let flags = cache.u32(row)?;
            let section = cache.i16(row + 0xa)?;
            let function = cache.i16(row + 8)?;
            // Only bit 0 selects a local section. Parent, child and seat routes
            // can retain an unused index outside this model's section table.
            ensure!(
                flags & 1 == 0
                    || (section >= -1 && (section < 0 || (section as usize) < sections)),
                "Source damage transfer at {row:#x} selects section {section} outside {sections} sections (flags {flags:#x})"
            );
            ensure!(
                (0..=2).contains(&function),
                "Invalid source damage transfer function {function} at {row:#x}"
            );
            Ok(
                json!({"flags":flags,"amount":number(cache,row+4)?,
            "function":function,"section":section,"seat":name(cache,row+0xc)?}),
            )
        })
        .collect()
}

fn response(cache: &Cache, row: usize, sections: usize) -> Result<Value> {
    let regions = cache
        .block(row + 0x58, 8)?
        .into_iter()
        .map(|at| {
            let state = cache.i16(at + 4)?;
            ensure!(
                (0..=4).contains(&state),
                "Invalid source region damage state"
            );
            Ok(json!({"region":name(cache,at)?,"state":state,"runtime_index":cache.i16(at+6)?}))
        })
        .collect::<Result<Vec<_>>>()?;
    let seats = cache
        .block(row + 0x90, 4)?
        .into_iter()
        .map(|at| name(cache, at))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "flags":cache.u32(row)?,"label":name(cache,row+4)?,
        "threshold":fraction(cache,row+8)?,
        "generic_effect":cache.reference(row+0xc,None)?,"generic_marker":name(cache,row+0x1c)?,
        "specific_effect":cache.reference(row+0x20,None)?,"specific_marker":name(cache,row+0x30)?,
        "damage_effect":cache.reference(row+0x34,None)?,"damage_marker":name(cache,row+0x44)?,
        "looping_effect":cache.reference(row+0x48,None)?,"regions":regions,
        "damage_transfers":transfers(cache,row+0x64,sections)?,
        "destroy_instance_group":cache.i16(row+0x70)?,
        "custom_behavior":cache.i16(row+0x72)?,"custom_label":name(cache,row+0x74)?,
        "delay_seconds":number(cache,row+0x78)?,"delay_effect":cache.reference(row+0x7c,None)?,
        "delay_marker":name(cache,row+0x8c)?,"eject_seats":seats,
        "skip_fraction":fraction(cache,row+0x9c)?,"destroyed_child_marker":name(cache,row+0xa0)?,
        "total_threshold":number(cache,row+0xa4)?,"constraint_group":name(cache,row+0xa8)?,
        "constraint_damage":cache.i16(row+0xac)?
    }))
}

pub(super) fn read(cache: &Cache, model: &Tag) -> Result<Value> {
    ensure!(
        model.group == "hlmt",
        "Source damage requires an object model"
    );
    let at = model.address()?;
    cache.meta(at, 0x220)?;
    let rows = cache.block(at + 0xe0, 0xc0)?;
    let sections = rows.iter().map(|&row| {
        let responses = cache.block(row + 0x84, 0xb0)?.into_iter()
            .map(|r| response(cache,r,rows.len())).collect::<Result<Vec<_>>>()?;
        let recharge = cache.block(row + 0x28, 4)?.into_iter()
            .map(|r| fraction(cache,r)).collect::<Result<Vec<_>>>()?;
        let curves = cache.block(row + 0x1c, 0x14)?.into_iter()
            .map(|r| super::vehicle::function(cache,r)).collect::<Result<Vec<_>>>()?;
        let rendering = cache.block(row + 0xa8, 0x20)?.into_iter().map(|r|
            Ok(json!({"third_person":cache.reference(r,None)?,"first_person":cache.reference(r+0x10,None)?})))
            .collect::<Result<Vec<_>>>()?;
        Ok(json!({
            "name":name(cache,row)?,"flags":cache.u32(row+4)?,
            "vitality_fraction":number(cache,row+8)?,"shield_material":name(cache,row+0xc)?,
            "stun_seconds":number(cache,row+0x10)?,"minimum_stun_damage":number(cache,row+0x14)?,
            "recharge_seconds":number(cache,row+0x18)?,"recharge_curves":curves,"recharge_fractions":recharge,
            "recharge_effect":cache.reference(row+0x34,None)?,
            "pre_recharge_seconds":number(cache,row+0x44)?,"pre_recharge_effect":cache.reference(row+0x48,None)?,
            "pre_recharge_marker":name(cache,row+0x58)?,"recharge_abort_effect":cache.reference(row+0x5c,None)?,
            "recharge_abort_marker":name(cache,row+0x6c)?,"overcharge_seconds":number(cache,row+0x70)?,
            "overcharge_fraction":number(cache,row+0x74)?,"pre_decay_seconds":number(cache,row+0x78)?,
            "decay_seconds":number(cache,row+0x7c)?,"restored_region":name(cache,row+0x80)?,
            "responses":responses,"damage_transfers":transfers(cache,row+0x90,rows.len())?,
            "health_transfers":transfers(cache,row+0x9c,rows.len())?,"rendering":rendering,
            "recharge_velocity":number(cache,row+0xb4)?,"overcharge_velocity":number(cache,row+0xb8)?
        }))
    }).collect::<Result<Vec<_>>>()?;
    let materials = cache.block(at + 0xb4, 0x14)?.into_iter().map(|row| {
        let section = cache.i16(row + 6)?;
        ensure!(section >= -1 && (section < 0 || (section as usize) < rows.len()),
            "Source material damage section exceeds table");
        Ok(json!({"name":name(cache,row)?,"section":section,
            "collision_material_index":cache.i16(row+8)?,"damage_material_index":cache.i16(row+0xa)?,
            "global_material":name(cache,row+0xc)?,"global_material_index":cache.i16(row+0x10)?}))
    }).collect::<Result<Vec<_>>>()?;
    let constraints = cache.block(at + 0xec, 0x14)?.into_iter().map(|row| Ok(json!({
        "physics_name":name(cache,row)?,"damage_name":name(cache,row+4)?,"group":name(cache,row+8)?,
        "probability_scale":number(cache,row+0xc)?,"kind":cache.i16(row+0x10)?,"index":cache.i16(row+0x12)?
    }))).collect::<Result<Vec<_>>>()?;
    let regions =
        cache
            .block(at + 0x120, 0x14)?
            .into_iter()
            .map(|row| {
                let permutations = cache.block(row+8,8)?.into_iter().map(|p| Ok(json!({
            "name":name(cache,p)?,"flags":cache.u8(p+4)?,
            "collision_index":cache.u8(p+5)? as i8,"physics_index":cache.u8(p+6)? as i8
        }))).collect::<Result<Vec<_>>>()?;
                Ok(
                    json!({"name":name(cache,row)?,"collision_index":cache.u8(row+4)? as i8,
            "physics_index":cache.u8(row+5)? as i8,"permutations":permutations}),
                )
            })
            .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"model":model,"flags":cache.u32(at+0xcc)?,"vitality":number(cache,at+0xd0)?,
        "indirect_material":name(cache,at+0xd4)?,"indirect_section":cache.i16(at+0xd8)?,
        "shielded_section":cache.i16(at+0xda)?,"sections":sections,
        "materials":materials,"constraints":constraints,"collision_regions":regions}),
    )
}
