//! What an imported model brings to a weapon's runtime entity, and the stages that author it
//! there: particle groups before the fired graph names them, a complete prepared vehicle, the
//! overrides and stat patches the graph adds, its projectile root, and the input callbacks and
//! media authored once the runtime edits are compiled.
use super::*;

/// What an imported model brings to its runtime, read from its asset graph.
pub(super) struct Runtime {
    animation: Option<crate::item::custom_runtime::animation::ImportedAnimation>,
    character: Option<crate::item::custom_runtime::animation::character::Binding>,
    audio: Option<crate::item::custom_runtime::audio::ImportedAudio>,
    firing_effect: Option<crate::item::custom_runtime::audio::FiringEffect>,
    extensions: Vec<crate::item::custom_runtime::extensions::Extension>,
    particles: Option<crate::item::custom_runtime::particles::ImportedParticles>,
    equipment: Option<crate::item::custom_runtime::animation::equipment::EquipmentAnimation>,
    crosshair: Option<crate::item::custom_runtime::crosshair::Crosshair>,
    zoom: Option<f32>,
    /// Source rows for markers the weapon runtime's own marker set also names.
    runtime_markers: Vec<crate::weapon::rig::RuntimeMarker>,
    inputs: Vec<crate::weapon::input::Input>,
    timing: Option<crate::weapon::input::timing::Timing>,
    projectile: Option<crate::weapon::behavior::imported::Prepared>,
}

impl Runtime {
    /// Whether the recipe imports anything onto the runtime entity.
    pub(super) fn any(&self) -> bool {
        self.animation.is_some()
            || self.character.is_some()
            || self.audio.is_some()
            || self.firing_effect.is_some()
            || self.equipment.is_some()
            || self.particles.is_some()
            || !self.extensions.is_empty()
            || self.crosshair.is_some()
            || self.zoom.is_some()
            || !self.runtime_markers.is_empty()
            || !self.inputs.is_empty()
            || self.timing.is_some()
            || self.projectile.is_some()
    }

    /// Reads each part from `donor`'s imported graph, all empty for a weapon without one.
    pub(super) fn load(
        donor: &resolve::ResolvedWeapon,
        reads: &mut Option<custom_runtime::imports::Fingerprints>,
    ) -> AuthoringResult<Self> {
        use crate::item::custom_runtime::{animation, audio, crosshair, extensions, particles};
        let mut loaded = Self {
            animation: None,
            character: None,
            audio: None,
            firing_effect: None,
            extensions: Vec::new(),
            particles: None,
            equipment: None,
            crosshair: None,
            zoom: None,
            runtime_markers: Vec::new(),
            inputs: Vec::new(),
            timing: None,
            projectile: None,
        };
        let mut directories = BTreeSet::new();
        if let Some(reference) = donor.weapon.overrides.imported_graph.as_ref() {
            let inputs = custom_runtime::imports::Inputs::open(reference)?;
            let graph = &inputs;
            loaded = Self {
                animation: animation::load(graph)?,
                character: animation::character::load(graph)?,
                audio: audio::load(graph).map_err(|error| {
                    error.context(format!(
                        "Loading prepared audio from {}",
                        reference.directory.display()
                    ))
                })?,
                firing_effect: audio::load_firing_effect(graph)?,
                extensions: extensions::load(graph)?,
                particles: particles::load(graph)?,
                equipment: animation::equipment::load(graph)?,
                crosshair: crosshair::load(graph)?,
                zoom: crate::weapon::zoom::imported(graph.value())?,
                runtime_markers: crate::weapon::rig::imported_runtime_markers(graph.value())?,
                inputs: crate::weapon::input::imported(graph.value())?,
                timing: crate::weapon::input::timing::imported(graph.value())?,
                projectile: crate::weapon::behavior::imported::load(graph.value())?,
            };
            if let Some(audio) = loaded.audio.as_mut() {
                audio.set_item(donor.weapon.identity.item_hash);
            }
            directories.insert(
                fs::canonicalize(&reference.directory)
                    .map_err(|error| invalid(error.to_string()))?,
            );
            *reads = inputs.fingerprints();
        }
        // Without a runtime pattern there is no entity to author, so the groups these perks name
        // are allocated with the subclass groups instead.
        for imported in donor
            .weapon
            .overrides
            .socket_plug_variants
            .iter()
            .filter(|_| donor.runtime_pattern_source.is_some())
            .flat_map(|variant| &variant.sandbox_perks)
            .filter_map(|perk| perk.program.as_ref())
            .flat_map(sundial::package_authoring::sandbox_perk::program::Program::owned_programs)
            .flat_map(|program| &program.imported_assets)
        {
            // The cache key holds each group's prepared fingerprint, and a replay checks the
            // directory against it again.
            attachments::load(imported, &mut directories, &mut loaded.particles)?;
        }
        Ok(loaded)
    }

    /// Composes the graph's component extensions onto `entity`, and gives its particle systems
    /// their own asset group, recorded for the private perks of the item at `weapon_ordinal`.
    /// Returns their tags by node symbol. The fired graph names these systems by symbol, so this
    /// runs before the graph is authored. Entries another item's group already holds are shared
    /// through `shared`, the build's content record the runtime cache also tracks.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &mut self,
        manager: &PackageManager,
        donor: &resolve::ResolvedWeapon,
        entity: &mut Vec<u8>,
        groups: crate::weapon::behavior::ContentGroups,
        assets: &mut RuntimeAssets<'_>,
        shared: &mut super::cache::AnimationCache,
        weapon_ordinal: usize,
    ) -> AuthoringResult<BTreeMap<String, TagHash>> {
        let overrides = &donor.weapon.overrides;
        crate::item::custom_runtime::extensions::author(manager, &self.extensions, entity)?;
        let symbols = match &mut self.particles {
            Some(particles) => {
                particles.prepare_runtime(
                    manager,
                    overrides
                        .fired_graph
                        .as_ref()
                        .and_then(|fired| fired.imported.as_deref()),
                    entity,
                    groups.own.or(groups.selected).unwrap_or(0),
                    overrides
                        .behavior_projectile_speed
                        .unwrap_or(crate::weapon::behavior::DEFAULT_PROJECTILE_SPEED_BOOST),
                )?;
                // A private perk's group can carry HUD statuses. They take their identities after
                // the runtime patches are placed, and their images join the attachment groups'.
                let displays = particles.bind_hud(manager)?;
                assets.hud_statuses.extend(displays);
                let (symbols, eager) = particles.author(
                    manager,
                    (&mut *assets.packages, shared),
                    donor.weapon.identity.item_hash,
                )?;
                assets.placed.extend(eager);
                assets
                    .particle_symbols
                    .insert(weapon_ordinal, symbols.clone());
                symbols
            }
            None => BTreeMap::new(),
        };
        self.check_perk_projectiles(overrides)?;
        Ok(symbols)
    }

    /// A private perk may name one of this weapon's imported projectile roots in place of a
    /// native projectile. The root must be declared by the weapon's graph, and it leaves the
    /// Barrel only beside the imported projectile the weapon fires, whose launch policy it was
    /// converted for.
    fn check_perk_projectiles(&self, overrides: &WeaponCloneOverrides) -> AuthoringResult<()> {
        for symbol in overrides
            .socket_plug_variants
            .iter()
            .flat_map(|variant| &variant.sandbox_perks)
            .filter_map(|perk| perk.program.as_ref())
            .flat_map(sundial::package_authoring::sandbox_perk::program::Program::owned_programs)
            .flat_map(sundial::package_authoring::sandbox_perk::program::Program::assets)
            .filter_map(|asset| asset.imported.as_deref())
        {
            let particles = self.particles.as_ref().ok_or_else(|| {
                invalid(format!(
                    "A private perk names imported projectile {symbol}, which this weapon's imported assets lack"
                ))
            })?;
            particles.perk_projectile(symbol)?;
            if overrides
                .fired_graph
                .as_ref()
                .and_then(|fired| fired.imported.as_deref())
                != particles.fired_projectile()
            {
                return Err(invalid(
                    "A private perk fires an imported projectile only while the weapon fires its own imported projectile, whose launch it shares",
                ));
            }
        }
        Ok(())
    }

    /// The tag of the complete prepared vehicle the particle group carries, once its
    /// presentation is applied to `entity`, the copy of the `source` entity. Such a vehicle
    /// supplies its own controls and parts, so neither native runtime edits, component grafts
    /// nor the weapon's other imported parts may change it.
    pub(super) fn vehicle(
        &self,
        manager: &PackageManager,
        donor: &resolve::ResolvedWeapon,
        (source, entity): (TagHash, &mut Vec<u8>),
        symbols: &BTreeMap<String, TagHash>,
        component_grafts: bool,
    ) -> AuthoringResult<Option<TagHash>> {
        let Some(particles) = &self.particles else {
            return Ok(None);
        };
        let Some(tag) = particles.apply_presentation(manager, source, entity, symbols)? else {
            return Ok(None);
        };
        let overrides = &donor.weapon.overrides;
        if overrides
            .sparrow
            .as_ref()
            .is_some_and(crate::vehicle::Sparrow::runtime_changes)
            || weapon_runtime_edits(donor)
            || overrides.fired_graph.is_some()
            || overrides.behavior_projectile_speed.is_some_and(|speed| {
                speed != crate::weapon::behavior::DEFAULT_PROJECTILE_SPEED_BOOST
            })
            || component_grafts
        {
            return Err(invalid(
                "This imported vehicle supplies its own controls. Carrier driving, durability and weapon overrides cannot be applied to it.",
            ));
        }
        if self.animation.is_some()
            || self.audio.is_some()
            || self.equipment.is_some()
            || self.crosshair.is_some()
            || !self.extensions.is_empty()
        {
            return Err(invalid(
                "A complete imported vehicle must include its animation, audio and component extensions in its prepared graph.",
            ));
        }
        Ok(Some(tag))
    }

    /// `base` with what the imported graph adds to it on `entity`: the graph's own fired graph
    /// unless the recipe overrides firing, the sword, character, extension and crosshair
    /// patches, and the fired graph's particle patches by symbol. `content_group` is the base
    /// weapon's own content group.
    pub(super) fn overrides(
        &self,
        manager: &PackageManager,
        base: &WeaponCloneOverrides,
        entity: &[u8],
        content_group: u32,
        groups: crate::weapon::behavior::ContentGroups,
        symbols: &BTreeMap<String, TagHash>,
    ) -> AuthoringResult<WeaponCloneOverrides> {
        let mut overrides = base.clone();
        if !crate::weapon::behavior::imported::overridden(&overrides)
            && let Some(prepared) = &self.projectile
        {
            overrides.fired_graph = Some(prepared.fired(manager, entity, content_group)?);
        }
        if let Some(particles) = &self.particles {
            overrides
                .runtime_resource_patches
                .extend(particles.sword_patches(
                    manager,
                    entity,
                    symbols,
                    overrides.sword_profile.is_some(),
                )?);
        }
        if let Some(character) = &self.character {
            character.apply(
                manager,
                entity,
                groups
                    .selected
                    .or(groups.own)
                    .ok_or_else(|| invalid("Imported character has no selected weapon content"))?,
                &mut overrides.runtime_resource_patches,
            )?;
        }
        overrides.runtime_resource_patches.extend(
            crate::item::custom_runtime::extensions::input_patches(
                manager,
                &self.extensions,
                entity,
            )?,
        );
        overrides.runtime_resource_patches.extend(
            crate::item::custom_runtime::extensions::attachment_patches(
                manager,
                &self.extensions,
                entity,
            )?,
        );
        // The hip-fire crosshair follows the runtime content's keys, not the item strings.
        if let Some(crosshair) = &self.crosshair
            && crosshair.replaces(&crate::weapon::crosshair::type_keys(manager, entity)?)?
        {
            overrides
                .runtime_resource_patches
                .extend(crate::weapon::crosshair::content_patches(
                    manager,
                    entity,
                    crosshair.type_key,
                )?);
        }
        if let Some(fired) = overrides.fired_graph.as_mut() {
            fired.validate()?;
            let patches =
                crate::item::custom_runtime::particles::fired_graph_patches(fired, symbols)?;
            fired.patches.extend(patches);
        }
        Ok(overrides)
    }

    /// The imported projectile root `overrides` fire, validated against the graph's declared
    /// root, with its trajectory pool size and whether it keeps its source trajectory.
    pub(super) fn projectile(
        &self,
        overrides: &WeaponCloneOverrides,
        symbols: &BTreeMap<String, TagHash>,
    ) -> AuthoringResult<ImportedProjectile> {
        let symbol = overrides
            .fired_graph
            .as_ref()
            .and_then(|fired| fired.imported.as_deref());
        let root = symbol
            .map(|symbol| {
                self.particles
                    .as_ref()
                    .ok_or_else(|| invalid("Imported projectile has no asset group"))?
                    .projectile(symbol)?;
                symbols
                    .get(symbol)
                    .copied()
                    .ok_or_else(|| invalid("Imported projectile root has not been allocated"))
            })
            .transpose()?;
        let trajectories = symbol
            .zip(self.particles.as_ref())
            .and_then(|(symbol, particles)| particles.trajectory_capacity(symbol));
        Ok(ImportedProjectile {
            root,
            trajectories: root.and(trajectories),
            source_trajectory: root.is_some()
                && self
                    .particles
                    .as_ref()
                    .is_some_and(|particles| particles.source_trajectory),
        })
    }

    /// The stat translator patches the imported fire timing, inputs, hand targets and zoom make
    /// on `entity`, in the table of translation `group`.
    pub(super) fn stat_patches(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        group: u32,
    ) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
        let mut patches = Vec::new();
        if let Some(timing) = &self.timing {
            patches.extend(timing.patches(manager, entity, group)?);
        }
        patches.extend(crate::weapon::input::patches(
            manager,
            entity,
            group,
            &self.inputs,
        )?);
        patches.extend(crate::weapon::rig::runtime_marker_patches(
            manager,
            entity,
            &self.runtime_markers,
        )?);
        if let Some(magnification) = self.zoom {
            patches.extend(crate::weapon::zoom::patches(
                manager,
                entity,
                group,
                magnification,
            )?);
        }
        Ok(patches)
    }

    /// Adds the input callbacks the graph's component extensions name to the edited `entity`.
    pub(super) fn callbacks(
        &self,
        manager: &PackageManager,
        entity: &mut [u8],
        allocator: AppendedTagAllocator,
        tags: &mut Vec<NewTagSpec>,
    ) -> AuthoringResult<()> {
        crate::item::custom_runtime::extensions::input_callbacks(
            manager,
            &self.extensions,
            entity,
            allocator,
            tags,
        )
    }

    /// Authors the imported clips, equipment animation, sounds and firing effect onto the edited
    /// `entity`, the copy of the `source` entity. Clips and audio media grow with every imported
    /// weapon, so they take a group in the asset packages. Only the owners that route them stay
    /// with `tags` in the host package.
    pub(super) fn author_media(
        mut self,
        manager: &PackageManager,
        (source, entity): (TagHash, &mut Vec<u8>),
        (allocator, tags): (AppendedTagAllocator, &mut Vec<NewTagSpec>),
        assets: &mut RuntimeAssets<'_>,
        animation_cache: &mut super::cache::AnimationCache,
        symbols: &BTreeMap<String, TagHash>,
    ) -> AuthoringResult<()> {
        let mut stream = crate::asset_packages::stream::Stream::new(assets.packages)?;
        // Media an earlier sound of the build allocated are shared through the content record the
        // clips use, which cached runtimes replay against.
        if let (Some(audio), Some(animation)) = (&self.audio, &mut self.animation) {
            let (stream_allocator, stream_tags) = stream.target();
            crate::item::custom_runtime::audio::author_clip_events(
                manager,
                audio,
                animation,
                stream_allocator,
                stream_tags,
                &mut crate::item::custom_runtime::audio::Media::new(&mut *animation_cache),
            )?;
        }
        let private_animation_owner = match &self.animation {
            Some(animation) => {
                let (_, owner) = crate::item::custom_runtime::animation::author(
                    manager,
                    animation,
                    source,
                    entity,
                    allocator,
                    tags,
                    stream.target(),
                    animation_cache,
                )?;
                Some(owner)
            }
            _ => None,
        };
        if let Some(animation) = &self.equipment {
            crate::item::custom_runtime::animation::equipment::author(
                manager,
                animation,
                source,
                entity,
                allocator,
                tags,
                stream.target(),
                animation_cache,
                symbols,
            )?;
        }
        if let Some(audio) = &self.audio {
            let mut media = crate::item::custom_runtime::audio::Media::new(&mut *animation_cache);
            let (stream_allocator, stream_tags) = stream.target();
            if let Some(impact) = crate::item::custom_runtime::audio::author_impacts(
                manager,
                audio,
                stream_allocator,
                stream_tags,
                &mut media,
            )? {
                assets.impacts.push(impact);
            }
            crate::item::custom_runtime::audio::author(
                manager,
                audio,
                self.firing_effect.as_ref().map(|effect| (effect, symbols)),
                source,
                entity,
                private_animation_owner,
                allocator,
                tags,
                stream.target(),
                &mut media,
            )?;
        } else if let Some(effect) = &self.firing_effect {
            // A firing flow rebuilt around source particles joins the same private presentation
            // its sounds would, after the particle group has taken its tags.
            crate::item::custom_runtime::audio::author_firing_effect(
                manager,
                effect,
                symbols,
                entity,
                allocator,
                tags,
                stream.target(),
            )?;
        }
        assets.placed.extend(stream.finish(assets.packages)?);
        Ok(())
    }
}
