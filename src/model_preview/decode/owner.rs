//! Keep geometry, material inputs and animation banks inside their actual entity owner.
use super::*;
use std::ops::Range;

/// Assignments and non-entity geometry have separate native loading contracts.
pub(in crate::model_preview) fn specialized(
    manager: &PackageManager,
    tag: u32,
    class: u32,
    cancel: &Load,
    clip: Option<u32>,
) -> Result<Option<Model>, String> {
    let model = match class {
        0x8080_744A => {
            let relation = checked(manager, tag, 0x8080_744A)?;
            let entity = u32_at(&relation, 0x10)?;
            if manager
                .get_entry(entity)
                .is_none_or(|e| e.reference != ENTITY)
            {
                return Err("This assignment does not point to a renderable object".into());
            }
            load_with_manager(manager, entity, cancel, clip)?
        }
        class if statics::is_static(class) => {
            cancel.say("Reading static mesh", 0, 0);
            statics::load(manager, tag)?
        }
        terrain::TERRAIN => {
            cancel.say("Reading terrain", 0, 0);
            terrain::load(manager, tag)?
        }
        terrain::RESOURCE => {
            let bytes = checked(manager, tag, terrain::RESOURCE)?;
            cancel.say("Reading terrain", 0, 0);
            terrain::load(manager, u32_at(&bytes, 0x18)?)?
        }
        light::SHADOWING_LIGHT | light::LIGHT_COLLECTION => light::load(manager, tag)?,
        _ => return Ok(None),
    };
    Ok(Some(model))
}

pub(in crate::model_preview) fn empty(
    manager: &PackageManager,
    tag: u32,
    inventory: &Inventory,
    resources: &[Vec<u8>],
    cancel: &Load,
) -> Result<Model, String> {
    let mut model = Model {
        assets: if inventory.particle_systems.is_empty()
            && inventory.sounds.is_empty()
            && inventory.lights.is_empty()
            && inventory.children.is_empty()
            && inventory.components.is_empty()
        {
            assets::generic(manager, tag)?
        } else {
            assets::read(manager, tag, inventory, cancel)?
        },
        ..Default::default()
    };
    model.clips = animation::clips(manager, resources);
    particles::collect_points(&mut model);
    light::append(manager, &inventory.lights, &mut model);
    Ok(model)
}

pub(in crate::model_preview) fn finish(
    manager: &PackageManager,
    mut model: Model,
    inventory: &Inventory,
    resources: &[Vec<u8>],
    emitter_only: bool,
) -> Result<Model, String> {
    if model.triangles.is_empty() {
        model.clips = animation::clips(manager, resources);
        light::append(manager, &inventory.lights, &mut model);
        if model.triangles.is_empty() && !emitter_only {
            return Err("The model contains no supported triangles.".into());
        }
    } else {
        particles::collect_points(&mut model);
        light::append(manager, &inventory.lights, &mut model);
    }
    Ok(model)
}

pub(in crate::model_preview) struct Owner {
    pub models: BTreeSet<u32>,
    pub resources: Range<usize>,
    pub components: Range<usize>,
}

pub(in crate::model_preview) fn owners(
    manager: &PackageManager,
    owners: &[Owner],
    resources: &[Vec<u8>],
    components: &[Vec<u8>],
    cancel: &Load,
    selected: Option<u32>,
    emitter_only: bool,
) -> Result<Model, String> {
    let mut result = Model::default();
    for (index, owner) in owners.iter().enumerate() {
        cancel.check()?;
        cancel.say(
            format!("Reading model owner {} of {}", index + 1, owners.len()),
            index,
            owners.len(),
        );
        let mut disabled = std::collections::BTreeMap::new();
        let model = loop {
            let (mut model, pending) = owner.load(
                manager,
                &components[owner.components.clone()],
                cancel,
                emitter_only,
                &disabled,
            )?;
            let resources = &resources[owner.resources.clone()];
            model.clips = animation::clips(manager, resources);
            let clip = selected
                .filter(|tag| owners.len() == 1 || model.clips.iter().any(|c| c.tag == *tag));
            match clip {
                Some(tag) => match animation::load_clip(manager, resources, &model, tag) {
                    Ok(animation) => model.animation = Some(animation),
                    Err(error) => model.animation_notice = Some(error),
                },
                None => match animation::load(manager, resources, &model) {
                    Ok(animation) => model.animation = animation,
                    Err(error) => model.animation_notice = Some(error),
                },
            }
            let failed = crate::model_preview::cloth::finish(&mut model, pending, cancel)?;
            if failed.is_empty() {
                break model;
            }
            // An invalid timeline must restore the authored skinned render group as well
            // as its stored positions. Rebuild this owner with only failed solvers disabled.
            disabled.extend(failed);
        };
        if owners.len() == 1 {
            return Ok(model);
        }
        appearance::append(&mut result, model, &format!("Owner {}", index + 1))?;
    }
    Ok(result)
}

impl Owner {
    fn load(
        &self,
        manager: &PackageManager,
        components: &[Vec<u8>],
        cancel: &Load,
        emitter_only: bool,
        disabled: &std::collections::BTreeMap<u32, String>,
    ) -> Result<(Model, Vec<(u32, crate::model_preview::cloth::Pending)>), String> {
        let mut model = Model::default();
        let mut pending = Vec::new();
        for &tag in &self.models {
            cancel.check()?;
            let component = components.iter().find(|bytes| {
                pointer(bytes, 0x18)
                    .ok()
                    .and_then(|data| u32_at(bytes, data + 0x1DC).ok())
                    == Some(tag)
            });
            let first_triangle = model.triangles.len();
            let inputs = effects::inputs(component.map(Vec::as_slice), components);
            let cloth = component.is_some_and(|bytes| {
                pointer(bytes, 0x18)
                    .ok()
                    .and_then(|data| data.checked_sub(4))
                    .and_then(|at| u32_at(bytes, at).ok())
                    == Some(0x8080_7286)
            });
            let result = append(
                manager,
                tag,
                component.map(Vec::as_slice),
                &inputs,
                &mut model,
                !disabled.contains_key(&tag),
            );
            let result = match result {
                Err(error) if cloth && !disabled.contains_key(&tag) => {
                    model
                        .notices
                        .push(format!("Cloth: {error}. Showing stored geometry."));
                    append(
                        manager,
                        tag,
                        component.map(Vec::as_slice),
                        &inputs,
                        &mut model,
                        false,
                    )
                }
                result => result,
            };
            match result {
                Ok(simulation) => {
                    model.tags.push(tag);
                    if let Some(simulation) = simulation {
                        pending.push((tag, simulation));
                    }
                    if let Some(error) = disabled.get(&tag) {
                        model
                            .notices
                            .push(format!("Cloth: {error}. Showing stored geometry."));
                    }
                    if emitter_only {
                        model.triangle_emitter.resize(model.triangles.len(), false);
                        model.triangle_emitter[first_triangle..].fill(true);
                    }
                }
                Err(error) if emitter_only => model.notices.push(format!(
                    "Particle geometry 0x{tag:08X} could not be drawn: {error}"
                )),
                Err(error) => return Err(error),
            }
        }
        model.particle_geometry = emitter_only;
        Ok((model, pending))
    }
}
