//! A vehicle import's armament: how its seats and mounts ride on its carrier, and the converted
//! projectiles its own barrel, secondary pod, stationary gun and gunners' seats fire.
use super::*;

/// How a vehicle's seats and mounts ride on its native carrier.
pub(super) struct Layout {
    /// Seats and a secondary pod ride as children on unit turret rows, which only a carrier whose
    /// stock unit carries them creates. Another carrier's seats ride on its own entity, where a
    /// gunner has no unit of its own to aim and rides as a passenger.
    carries_children: bool,
    stationary_driver: Option<Value>,
}

impl Layout {
    /// Reads how `scene` rides on the entry's vehicle runtime, detaching the mounts that ride as
    /// the carrier's children. An item without a vehicle runtime has neither.
    pub(super) fn read(
        cache: &cache::Cache,
        native: &mut Reader,
        entry: &ImportRequest,
        scene: &mut Scene,
    ) -> Result<Self> {
        let runtime = entry
            .runtime_entity
            .filter(|_| scene.object.tag.group == "vehi");
        let carries_children = match runtime {
            Some(entity) => vehicle::carries_children(native, entity)?,
            None => false,
        };
        if runtime.is_some() && vehicle::attaches_mounts(scene) {
            vehicle::detach_mounts(cache, scene)?;
            if !carries_children {
                for mount in &mut scene.mounts {
                    mount.gunner = false;
                }
            }
        }
        let stationary_driver = if runtime.is_some() {
            vehicle::stationary_driver(cache, scene)?
        } else {
            None
        };
        Ok(Self {
            carries_children,
            stationary_driver,
        })
    }
}

/// The converted projectiles a vehicle's own barrels fire, with its stationary driver report.
/// Each gunner's projectile stays on its mount.
pub(super) struct Armament {
    stationary_driver: Option<Value>,
    primary: Option<Value>,
    secondary: Option<Value>,
}

impl Armament {
    /// Converts the vehicle's own weapon, its secondary pod's and each gunner's.
    pub(super) fn convert(
        sources: &Sources<'_>,
        pages: &mut resource::Pages,
        native: &mut Reader,
        scene: &mut Scene,
        layout: Layout,
    ) -> Result<Self> {
        let primary = primary(sources, pages, native, scene, &layout)?;
        // An explosive source round whose flight effects did not translate flies unseen on the
        // effect-only carrier, as the Scorpion's shell did. The vehicle keeps its native barrel's
        // own explosive round instead.
        let primary = detonating(primary, sources.graph)?;
        let secondary = match &primary {
            Some(primary) if layout.carries_children => secondary(sources, pages, native, primary)?,
            _ => None,
        };
        gunners(sources, pages, native, scene, primary.as_ref())?;
        Ok(Self {
            stationary_driver: layout.stationary_driver,
            primary,
            secondary,
        })
    }

    /// The projectiles the vehicle build points its barrel and secondary pod at.
    pub(super) fn sections(&self) -> [Option<&Value>; 2] {
        [self.primary.as_ref(), self.secondary.as_ref()]
    }

    /// Places the converted projectiles in `graph`. The vehicle's own, or else its first gunner's,
    /// is the fired projectile and the rest sit beside it. Only a secondary the vehicle build
    /// mounted is kept. Its nodes load with the fired one.
    pub(super) fn place(
        self,
        graph: &mut Value,
        scene: &Scene,
        projectile_import: &mut Value,
        entry: &ImportRequest,
    ) -> Result<()> {
        if let Some(section) = self.primary {
            *projectile_import = json!({"status":"imported","selection":"vehicle_weapon",
                "source":section["conversion"]["source"],"native_carrier":section["conversion"]["native_carrier"]});
            graph["projectile"] = section;
        }
        let mut additional = Vec::new();
        for section in scene
            .mounts
            .iter()
            .filter_map(|mount| mount.projectile.clone())
        {
            if graph["projectile"].is_null() {
                *projectile_import = json!({"status":"imported","selection":"mounted_weapon",
                    "source":section["conversion"]["source"],"native_carrier":section["conversion"]["native_carrier"]});
                graph["projectile"] = section;
            } else {
                additional.push(section);
            }
        }
        if let Some(report) = self.stationary_driver {
            graph["vehicle"]["stationary_driver"] = report;
        }
        if let Some(section) = self.secondary
            && graph["vehicle"]["secondary_weapon"].is_object()
        {
            projectile_import["secondary"] = section["conversion"]["source"].clone();
            additional.push(section);
        }
        if !additional.is_empty() {
            graph["additional_projectiles"] = json!(additional);
        }
        if entry.runtime_entity.is_some() && scene.object.tag.group == "vehi" {
            carry_nodes(graph)?;
        }
        Ok(())
    }
}

/// The projectile a vehicle's own source weapon fires from its native barrel. It is converted
/// first, so the vehicle build can point the barrel at it and write the launch cue it hosts.
/// Without one, a stationary gun whose seat rides on an attached gun object, as the Shade's
/// does, fires that object's weapon. The behavior names every variant's gun, so the attached one
/// is taken from the scene.
fn primary(
    sources: &Sources<'_>,
    pages: &mut resource::Pages,
    native: &mut Reader,
    scene: &Scene,
    layout: &Layout,
) -> Result<Option<Value>> {
    let entry = sources.entry;
    if entry.runtime_entity.is_some() && scene.object.tag.group == "vehi" {
        let launch = if entry.source.audio.is_some() {
            vehicle::launch(native, sources.behavior, sources.cache, None)?
        } else {
            None
        };
        let projectile = crate::halo_reach::projectile::vehicle(
            (sources.cache, pages),
            native,
            sources.behavior,
            sources.projectiles,
            entry,
            sources.graph,
            launch.as_ref(),
        )
        .with_context(|| format!("Vehicle weapon projectile of {}", entry.slug))?;
        if projectile.is_some() {
            return Ok(projectile);
        }
    }
    let Some(parent) = layout
        .stationary_driver
        .as_ref()
        .and_then(|report| report["seat_model"].as_u64())
        .and_then(|model| scene.models.get(usize::try_from(model).ok()?))
        .and_then(|placed| placed.parent.as_ref())
    else {
        return Ok(None);
    };
    // The stationary gun's own weapon is the driver's, so its projectile hosts the launch sound.
    let owner = u64::from(parent.object.datum);
    let weapon = crate::halo_reach::projectile::mounted_weapon(sources.behavior, owner)?;
    let launch = match weapon {
        Some(weapon) if entry.source.audio.is_some() => {
            vehicle::launch(native, sources.behavior, sources.cache, Some(weapon))?
        }
        _ => None,
    };
    crate::halo_reach::projectile::mounted(
        sources.cache,
        pages,
        native,
        sources.behavior,
        sources.projectiles,
        entry,
        sources.graph,
        owner,
        None,
        launch.as_ref(),
    )
    .with_context(|| format!("Stationary gun projectile of {}", entry.slug))
}

/// The driver's secondary trigger fires its own converted projectile from a driver-aimed weapon
/// child, as the stock tank's second weapon does. It needs the primary's launch.
fn secondary(
    sources: &Sources<'_>,
    pages: &mut resource::Pages,
    native: &mut Reader,
    primary: &Value,
) -> Result<Option<Value>> {
    let section = crate::halo_reach::projectile::vehicle_secondary(
        sources.cache,
        pages,
        native,
        sources.behavior,
        sources.projectiles,
        sources.entry,
        sources.graph,
    )
    .with_context(|| format!("Vehicle secondary projectile of {}", sources.entry.slug))?;
    Ok(detonating(section, sources.graph)?.filter(|section| {
        section["conversion"]["launch_policy"] == primary["conversion"]["launch_policy"]
    }))
}

/// Each gunner's seated child fires its own source gun's converted projectile. Without a driver
/// projectile the first one is the graph's fired projectile, and the rest sit beside it in barrel
/// sections that share its launch policy.
fn gunners(
    sources: &Sources<'_>,
    pages: &mut resource::Pages,
    native: &mut Reader,
    scene: &mut Scene,
    primary: Option<&Value>,
) -> Result<()> {
    let mut fired_policy = primary.map(|section| section["conversion"]["launch_policy"].clone());
    let mut next_barrel = 10u8;
    for mount in scene.mounts.iter_mut().filter(|mount| mount.gunner) {
        let barrel = fired_policy.is_some().then(|| {
            next_barrel += 1;
            next_barrel - 1
        });
        let section = crate::halo_reach::projectile::mounted(
            sources.cache,
            pages,
            native,
            sources.behavior,
            sources.projectiles,
            sources.entry,
            sources.graph,
            u64::from(mount.object.datum),
            barrel,
            None,
        )
        .with_context(|| format!("Mounted gun projectile of {}", mount.object.path))?;
        let Some(section) = detonating(section, sources.graph)? else {
            continue;
        };
        let policy = section["conversion"]["launch_policy"].clone();
        match &fired_policy {
            Some(fired) if *fired != policy => continue,
            Some(_) => {}
            None => fired_policy = Some(policy),
        }
        mount.projectile = Some(section);
    }
    Ok(())
}

/// A vehicle's own nodes name its converted projectiles: its barrel, its secondary pod and its
/// gunners' barrels. The item build links the main nodes before the asset sections, so a vehicle
/// carries those projectiles' nodes among its own. Their files already sit under the graph root,
/// and the sections keep their records without nodes.
fn carry_nodes(graph: &mut Value) -> Result<()> {
    let object = graph.as_object_mut().context("Vehicle graph")?;
    let mut sections = Vec::new();
    sections.extend(object.remove("projectile"));
    if let Some(Value::Array(extra)) = object.remove("additional_projectiles") {
        sections.extend(extra);
    }
    let mut records = Vec::new();
    for mut section in sections {
        let nodes = section
            .as_object_mut()
            .and_then(|section| section.remove("nodes"))
            .context("Vehicle projectile section nodes")?;
        let Value::Array(nodes) = nodes else {
            anyhow::bail!("Vehicle projectile section nodes are not an array");
        };
        // The runtime presentation mirrors the vehicle's runtime nodes for the item's runtime
        // build, which links apart from the main nodes, so it carries them too.
        if let Some(mirror) = graph
            .get_mut("runtime_presentation")
            .and_then(|section| section.get_mut("nodes"))
            .and_then(Value::as_array_mut)
        {
            mirror.extend(nodes.iter().cloned());
        }
        graph["nodes"]
            .as_array_mut()
            .context("Vehicle graph nodes")?
            .extend(nodes);
        records.push(section);
    }
    if !records.is_empty() {
        graph["vehicle_projectiles"] = json!(records);
    }
    Ok(())
}
