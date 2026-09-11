//! On-demand component-donor screening using the compiler's actual owner-partition graft.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

use sundial::{
    investment::WeaponDonorSummary,
    package_authoring::{
        open_shadowkeep_package_manager,
        weapon_entity::{
            WEAPON_BARREL_COMPONENT_KEY, WEAPON_CONTROLLER_COMPONENT_KEY,
            WEAPON_INPUT_COMPONENT_KEY, WEAPON_MAGAZINE_COMPONENT_KEY, WEAPON_RELOAD_COMPONENT_KEY,
            WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
            WEAPON_TRIGGER_COMPONENT_KEY, WeaponComponentBinding,
            coupled_weapon_component_bindings, graft_weapon_component_bindings,
            weapon_component_binding_hashes, weapon_component_bindings,
        },
        weapon_runtime::{
            WeaponRuntimeEntitySource, WeaponRuntimeResourceShape,
            load_weapon_runtime_entity_at_pattern_index_with_manager,
            load_weapon_runtime_entity_with_manager, load_weapon_runtime_resource_shape,
        },
    },
};
use tiger_pkg::PackageManager;

use super::RuntimeGraphKey;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum DonorCompatibility {
    LowerRisk,
    Experimental,
    Incompatible,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentDonorAssessment {
    pub status: DonorCompatibility,
    pub reasons: Vec<String>,
    pub affected_bindings: Vec<u32>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct EffectiveComponentSource {
    pub donor_item_hash: u32,
    pub via_binding_hash: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentCompatibilityReport {
    pub binding_hash: u32,
    pub candidates: BTreeMap<u32, ComponentDonorAssessment>,
    pub affected_bindings: Vec<(u32, String)>,
    pub current_sources: BTreeMap<u32, Vec<EffectiveComponentSource>>,
    pub current_error: Option<String>,
}

type Bindings = BTreeMap<u32, Vec<WeaponComponentBinding>>;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum EntityKey {
    Pattern(u16),
    Item(u32),
}

struct ScanCache<'a> {
    manager: &'a PackageManager,
    entities: BTreeMap<EntityKey, Result<Arc<WeaponRuntimeEntitySource>, String>>,
    shapes: BTreeMap<(u32, u32, u64), Result<WeaponRuntimeResourceShape, String>>,
}

impl<'a> ScanCache<'a> {
    fn new(manager: &'a PackageManager) -> Self {
        Self {
            manager,
            entities: BTreeMap::new(),
            shapes: BTreeMap::new(),
        }
    }

    fn entity(
        &mut self,
        pattern: Option<u16>,
        item: u32,
    ) -> Result<Arc<WeaponRuntimeEntitySource>, String> {
        let key = pattern.map_or(EntityKey::Item(item), EntityKey::Pattern);
        self.entities
            .entry(key)
            .or_insert_with(|| {
                match key {
                    EntityKey::Pattern(index) => {
                        load_weapon_runtime_entity_at_pattern_index_with_manager(
                            self.manager,
                            index,
                        )
                    }
                    EntityKey::Item(hash) => {
                        load_weapon_runtime_entity_with_manager(self.manager, hash)
                    }
                }
                .map(Arc::new)
            })
            .clone()
    }

    fn shape(
        &mut self,
        binding: &WeaponComponentBinding,
    ) -> Result<WeaponRuntimeResourceShape, String> {
        self.shapes
            .entry((
                binding.owner_tag,
                binding.concrete_class,
                binding.resource_offset,
            ))
            .or_insert_with(|| load_weapon_runtime_resource_shape(self.manager, binding))
            .clone()
    }
}

#[derive(Clone)]
struct RequestedDonor {
    binding_hash: u32,
    item_hash: u32,
    source: Arc<WeaponRuntimeEntitySource>,
}

/// Run on a worker thread when a single component picker opens. Every result is relative to this
/// exact selection key, including the other requested donors. Lower risk is not gameplay proof.
pub(crate) fn assess_component_donors(
    packages: &Path,
    key: &RuntimeGraphKey,
    binding_hash: u32,
    donors: &[WeaponDonorSummary],
) -> Result<ComponentCompatibilityReport, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let mut cache = ScanCache::new(&manager);
    let baseline = cache.entity(key.pattern_index, key.fallback_item_hash)?;
    let baseline_bindings = collect_bindings(&baseline.payload)?;
    let affected = coupled_weapon_component_bindings(&baseline.payload, binding_hash)?;
    let baseline_summary = baseline_summary(donors, key, &baseline);
    let baseline_hash = baseline_summary.map_or(baseline.item_hash, |summary| summary.hash);
    let current = requested_donors(&mut cache, key, &[]);
    let (current_sources, current_error) = match current.and_then(|requested| {
        compose(&baseline, &requested)?;
        effective_sources(
            &baseline_bindings,
            baseline_hash,
            &requested,
            baseline.item_hash,
        )
    }) {
        Ok(sources) => (sources, None),
        Err(error) => (BTreeMap::new(), Some(error)),
    };
    let retained = requested_donors(&mut cache, key, &affected);
    let mut candidates = BTreeMap::new();
    for donor in donors {
        let assessment = match &retained {
            Ok(retained) => assess_candidate(
                &mut cache,
                &baseline,
                &baseline_bindings,
                baseline_summary,
                baseline_hash,
                retained,
                donor,
                donors,
                &affected,
            ),
            Err(error) => incompatible(
                format!("Another selected donor could not be read: {error}"),
                &affected,
            ),
        };
        candidates.insert(donor.hash, assessment);
    }
    Ok(ComponentCompatibilityReport {
        binding_hash,
        candidates,
        affected_bindings: affected
            .into_iter()
            .map(|hash| (hash, binding_label(hash)))
            .collect(),
        current_sources,
        current_error,
    })
}

fn requested_donors(
    cache: &mut ScanCache<'_>,
    key: &RuntimeGraphKey,
    excluded_bindings: &[u32],
) -> Result<Vec<RequestedDonor>, String> {
    key.component_donors
        .iter()
        .filter(|(binding, _, _)| !excluded_bindings.contains(binding))
        .map(|&(binding_hash, pattern, item_hash)| {
            Ok(RequestedDonor {
                binding_hash,
                item_hash,
                source: cache.entity(pattern, item_hash)?,
            })
        })
        .collect()
}

fn collect_bindings(entity: &[u8]) -> Result<Bindings, String> {
    weapon_component_binding_hashes(entity)?
        .into_iter()
        .map(|hash| Ok((hash, weapon_component_bindings(entity, hash)?)))
        .collect()
}

#[cfg(test)]
fn affected_bindings(bindings: &Bindings, selected: u32) -> Result<Vec<u32>, String> {
    let owners = bindings
        .get(&selected)
        .filter(|resources| !resources.is_empty())
        .ok_or_else(|| format!("The runtime baseline has no binding 0x{selected:08X}"))?
        .iter()
        .map(|resource| resource.owner_tag)
        .collect::<BTreeSet<_>>();
    Ok(bindings
        .iter()
        .filter(|(_, resources)| {
            resources
                .iter()
                .any(|resource| owners.contains(&resource.owner_tag))
        })
        .map(|(&hash, _)| hash)
        .collect())
}

fn compose(
    baseline: &WeaponRuntimeEntitySource,
    requested: &[RequestedDonor],
) -> Result<Vec<u8>, String> {
    let mut active = requested
        .iter()
        .filter(|request| request.source.item_hash != baseline.item_hash)
        .collect::<Vec<_>>();
    active.sort_by_key(|request| request.binding_hash);
    let grafts = active
        .iter()
        .map(|request| (request.binding_hash, request.source.payload.as_slice()))
        .collect::<Vec<_>>();
    let mut authored = baseline.payload.clone();
    // This is the same atomic implementation used by the package compiler, with all retained
    // selections included. Assessing the candidate in isolation would miss shared-owner conflicts.
    graft_weapon_component_bindings(&mut authored, &grafts)?;
    Ok(authored)
}

fn effective_sources(
    baseline: &Bindings,
    baseline_hash: u32,
    requested: &[RequestedDonor],
    baseline_runtime_item: u32,
) -> Result<BTreeMap<u32, Vec<EffectiveComponentSource>>, String> {
    let mut owners = BTreeMap::new();
    let mut active = requested
        .iter()
        .filter(|request| request.source.item_hash != baseline_runtime_item)
        .collect::<Vec<_>>();
    active.sort_by_key(|request| request.binding_hash);
    for request in active {
        let targets = baseline.get(&request.binding_hash).ok_or_else(|| {
            format!(
                "The runtime baseline has no binding 0x{:08X}",
                request.binding_hash
            )
        })?;
        for target in targets {
            owners
                .entry(target.owner_tag)
                .or_insert(EffectiveComponentSource {
                    donor_item_hash: request.item_hash,
                    via_binding_hash: Some(request.binding_hash),
                });
        }
    }
    Ok(baseline
        .iter()
        .map(|(&hash, resources)| {
            let sources = resources
                .iter()
                .map(|resource| {
                    owners
                        .get(&resource.owner_tag)
                        .cloned()
                        .unwrap_or(EffectiveComponentSource {
                            donor_item_hash: baseline_hash,
                            via_binding_hash: None,
                        })
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            (hash, sources)
        })
        .collect())
}

fn baseline_summary<'a>(
    donors: &'a [WeaponDonorSummary],
    key: &RuntimeGraphKey,
    baseline: &WeaponRuntimeEntitySource,
) -> Option<&'a WeaponDonorSummary> {
    // Prefer the actual sandbox-row identity. Never infer compatibility from an arbitrary
    // catalog item that merely happens to represent the same row.
    donors
        .iter()
        .find(|donor| donor.hash == baseline.item_hash)
        .or_else(|| {
            donors.iter().find(|donor| {
                donor.hash == key.fallback_item_hash
                    && donor.weapon_pattern_index == key.pattern_index
            })
        })
}

#[allow(clippy::too_many_arguments)]
fn assess_candidate(
    cache: &mut ScanCache<'_>,
    baseline: &WeaponRuntimeEntitySource,
    baseline_bindings: &Bindings,
    baseline_summary: Option<&WeaponDonorSummary>,
    baseline_hash: u32,
    retained: &[RequestedDonor],
    donor: &WeaponDonorSummary,
    donors: &[WeaponDonorSummary],
    affected: &[u32],
) -> ComponentDonorAssessment {
    let Some(pattern) = donor.weapon_pattern_index else {
        return incompatible("The donor has no active runtime row".into(), affected);
    };
    let source = match cache.entity(Some(pattern), donor.hash) {
        Ok(source) => source,
        Err(error) => {
            return incompatible(
                format!("The donor runtime could not be read: {error}"),
                affected,
            );
        }
    };
    let mut requested = retained.to_vec();
    requested.extend(affected.iter().map(|&binding_hash| RequestedDonor {
        binding_hash,
        item_hash: donor.hash,
        source: Arc::clone(&source),
    }));
    let authored = match compose(baseline, &requested) {
        Ok(authored) => authored,
        Err(error) => return incompatible(error, affected),
    };
    let effective = match collect_bindings(&authored).and_then(|bindings| {
        effective_sources(
            baseline_bindings,
            baseline_hash,
            &requested,
            baseline.item_hash,
        )
        .map(|sources| (bindings, sources))
    }) {
        Ok(effective) => effective,
        Err(error) => return incompatible(error, affected),
    };
    let mut reasons = Vec::new();
    let effective_donors = affected
        .iter()
        .filter_map(|binding| effective.1.get(binding))
        .flatten()
        .map(|source| source.donor_item_hash)
        .collect::<BTreeSet<_>>();
    for hash in effective_donors {
        let effective_summary = donors.iter().find(|summary| summary.hash == hash);
        metadata_reasons(baseline_summary, effective_summary, &mut reasons);
    }
    // The chosen donor can be a no-op because it shares the baseline row. Its item metadata must
    // still be known and match before this candidate receives the lower-risk label.
    metadata_reasons(baseline_summary, Some(donor), &mut reasons);
    for binding in affected {
        let before = &baseline_bindings[binding];
        let Some(after) = effective.0.get(binding) else {
            return incompatible(
                format!("The composed runtime lost binding 0x{binding:08X}"),
                affected,
            );
        };
        if before.len() != after.len() {
            return incompatible(
                format!(
                    "The composed runtime changed the resource count for binding 0x{binding:08X}"
                ),
                affected,
            );
        }
        for (before, after) in before.iter().zip(after) {
            shape_reasons(cache.shape(before), cache.shape(after), &mut reasons);
        }
    }
    assessed(reasons, affected)
}

fn assessed(mut reasons: Vec<String>, affected: &[u32]) -> ComponentDonorAssessment {
    reasons.sort();
    reasons.dedup();
    let status = if reasons.is_empty() {
        DonorCompatibility::LowerRisk
    } else {
        DonorCompatibility::Experimental
    };
    if reasons.is_empty() {
        reasons.push("Known family, ammo, animations and affected component schemas match. In-game behavior is not verified.".into());
    }
    ComponentDonorAssessment {
        status,
        reasons,
        affected_bindings: affected.to_vec(),
    }
}

fn metadata_reasons(
    baseline: Option<&WeaponDonorSummary>,
    candidate: Option<&WeaponDonorSummary>,
    reasons: &mut Vec<String>,
) {
    let (Some(baseline), Some(candidate)) = (baseline, candidate) else {
        reasons.push("Runtime donor metadata is not fully known".into());
        return;
    };
    let known_family = |name: &str| {
        !name.trim().is_empty()
            && !name.to_ascii_lowercase().contains("unknown")
            && !name.eq_ignore_ascii_case("weapon")
    };
    if !known_family(&baseline.type_name) || !known_family(&candidate.type_name) {
        reasons.push("Weapon family is not fully known".into());
    } else if baseline.type_name != candidate.type_name {
        reasons.push("Different weapon family".into());
    }
    match (baseline.ammo_type, candidate.ammo_type) {
        (Some(left), Some(right)) if left == right => {}
        (Some(_), Some(_)) => reasons.push("Different native ammo type".into()),
        _ => reasons.push("Native ammo type is not fully known".into()),
    }
    let known_group =
        |group: Option<u32>| group.filter(|value| !matches!(*value, 0 | u32::MAX | 0x811C_9DC5));
    match (
        known_group(baseline.weapon_translation_group),
        known_group(candidate.weapon_translation_group),
    ) {
        (Some(left), Some(right)) if left == right => {}
        (Some(_), Some(_)) => reasons.push("Different native animation group".into()),
        _ => reasons.push("Native animation group is not fully known".into()),
    }
}

fn shape_reasons(
    before: Result<WeaponRuntimeResourceShape, String>,
    after: Result<WeaponRuntimeResourceShape, String>,
    reasons: &mut Vec<String>,
) {
    match (before, after) {
        (Ok(before), Ok(after)) if before == after => {}
        (Ok(_), Ok(_)) => {
            reasons.push("Affected component instance or definition schemas differ".into())
        }
        _ => reasons.push("Affected component schemas could not all be verified".into()),
    }
}

fn incompatible(reason: String, affected: &[u32]) -> ComponentDonorAssessment {
    ComponentDonorAssessment {
        status: DonorCompatibility::Incompatible,
        reasons: vec![reason],
        affected_bindings: affected.to_vec(),
    }
}

fn binding_label(hash: u32) -> String {
    match hash {
        WEAPON_INPUT_COMPONENT_KEY => "Input".into(),
        WEAPON_TRIGGER_COMPONENT_KEY => "Trigger".into(),
        WEAPON_BARREL_COMPONENT_KEY => "Barrel".into(),
        WEAPON_CONTROLLER_COMPONENT_KEY => "Weapon Controller".into(),
        WEAPON_MAGAZINE_COMPONENT_KEY => "Magazine".into(),
        WEAPON_RELOAD_COMPONENT_KEY => "Reload".into(),
        WEAPON_TRIGGER_CHARGE_COMPONENT_KEY => "Trigger Charge".into(),
        WEAPON_STAT_TRANSLATOR_COMPONENT_KEY => "Weapon Stats / Translator".into(),
        _ => format!("Binding 0x{hash:08X}"),
    }
}
