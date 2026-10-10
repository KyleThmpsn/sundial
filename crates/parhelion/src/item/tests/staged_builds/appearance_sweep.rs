//! Builds one weapon of every rig family wearing the appearance of every other family, and
//! writes each outcome to a JSON report so a sweep can be compared against the next.
//!
//! Every pair that builds is read back from staged packages. A weapon either wears the
//! appearance's whole rig or keeps the base's, never a mix, and its content block names the base
//! weapon's type, which its type-specific firing follows. A mixed rig built cleanly before and only showed in game, as a
//! support hand on the wrong foregrip and parts floating off the model.
use super::*;
use crate::weapon::rig::PRESENTATION_BINDINGS;
use std::sync::Mutex;
use sundial::package_authoring::entity::{
    weapon_component_binding_hashes, weapon_component_bindings,
};
use sundial::package_authoring::runtime::{
    load_weapon_runtime_entity_at_pattern_index_with_manager,
    load_weapon_runtime_entity_with_manager,
};

type Slot = sundial::investment::WeaponInventorySlot;
type Weapon = (u32, String, String, Option<Slot>);
type Pair<'a> = (&'a (u32, Weapon), &'a (u32, Weapon));

/// One pair's result: why it failed to build or read back, else how it wears the rig.
#[derive(Clone)]
struct Outcome {
    error: Option<String>,
    rig: Option<&'static str>,
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn real_buildable_families_accept_other_buildable_familys_appearance() {
    let packages = crate::test_support::stock_packages();
    let report = crate::test_support::artifact_dir("appearance-sweep-report.json");
    let threads = std::env::var("PARHELION_APPEARANCE_SWEEP_THREADS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(4usize);
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let mut candidates = BTreeMap::<u32, Vec<Weapon>>::new();
    let mut patterns = BTreeMap::<u32, u16>::new();
    let mut donors = catalog.weapon_donors();
    donors.sort_by_key(|donor| (!donor.collection_backed, donor.hash));
    for donor in donors {
        let Some(group) = donor.weapon_translation_group else {
            continue;
        };
        if donor.name.is_empty() || matches!(group, 0 | 0x811C_9DC5) {
            continue;
        }
        if let Some(pattern) = donor.weapon_pattern_index {
            patterns.insert(donor.hash, pattern);
        }
        candidates.entry(group).or_default().push((
            donor.hash,
            donor.name,
            donor.type_name,
            donor.inventory_slot,
        ));
    }
    // Each family is represented by its first weapon that builds unchanged, so a failure in the
    // sweep belongs to the appearance rather than to the gameplay weapon's own sockets.
    let groups = candidates.keys().copied().collect::<Vec<_>>();
    let chosen = parallel(threads, &groups, |_, group| {
        candidates[group]
            .iter()
            .take(12)
            .find(|weapon| build(&packages, vec![spec(weapon, None)]).is_ok())
            .cloned()
    });
    let mut families = Vec::new();
    let mut unbuildable = Vec::new();
    for (group, weapon) in groups.iter().zip(chosen) {
        match weapon {
            Some(weapon) => families.push((*group, weapon)),
            None => unbuildable.push(format!("0x{group:08X}")),
        }
    }
    assert!(
        families.len() >= 2,
        "the sweep needs distinct buildable families"
    );
    let pairs = families
        .iter()
        .flat_map(|gameplay| {
            families
                .iter()
                .filter(move |appearance| appearance.0 != gameplay.0)
                .map(move |appearance| (gameplay, appearance))
        })
        .collect::<Vec<_>>();
    eprintln!(
        "{} families, {} without a buildable weapon, {} pairs",
        families.len(),
        unbuildable.len(),
        pairs.len()
    );
    // One project per gameplay weapon carries every appearance at once. Only a project that
    // fails is rebuilt one pair at a time, so each failure is attributed to its own pair.
    let batches = parallel(threads, &families, |index, gameplay| {
        let batch = pairs
            .iter()
            .filter(|(owner, _)| owner.0 == gameplay.0)
            .collect::<Vec<_>>();
        let specs = batch
            .iter()
            .map(|(gameplay, appearance)| spec(&gameplay.1, Some(&appearance.1)))
            .collect::<Vec<_>>();
        let outcomes = match build(&packages, specs) {
            Ok(bundle) => check(&packages, &bundle, &batch, &patterns),
            Err(_) => batch
                .iter()
                .map(
                    |pair| match build(&packages, vec![spec(&pair.0.1, Some(&pair.1.1))]) {
                        Ok(bundle) => check(&packages, &bundle, &[*pair], &patterns).remove(0),
                        Err(error) => Outcome {
                            error: Some(error),
                            rig: None,
                        },
                    },
                )
                .collect(),
        };
        eprintln!(
            "{}/{} {}: {} of {} failed",
            index + 1,
            families.len(),
            gameplay.1.1,
            outcomes
                .iter()
                .filter(|outcome| outcome.error.is_some())
                .count(),
            batch.len()
        );
        batch.into_iter().zip(outcomes).collect::<Vec<_>>()
    });
    let (pairs, outcomes): (Vec<_>, Vec<_>) = batches.into_iter().flatten().unzip();
    let describe = |(group, (hash, name, kind, slot)): &(u32, Weapon)| {
        serde_json::json!({
            "group": format!("0x{group:08X}"), "hash": format!("0x{hash:08X}"),
            "name": name, "type": kind, "slot": format!("{slot:?}"),
        })
    };
    let results = pairs
        .iter()
        .zip(&outcomes)
        .map(|(&(gameplay, appearance), outcome)| {
            serde_json::json!({
                "gameplay": describe(gameplay),
                "appearance": describe(appearance),
                "rig": outcome.rig,
                "error": outcome.error,
            })
        })
        .collect::<Vec<_>>();
    let failures = outcomes
        .iter()
        .filter(|outcome| outcome.error.is_some())
        .count();
    let rigs = |kind| {
        outcomes
            .iter()
            .filter(|outcome| outcome.rig == Some(kind))
            .count()
    };
    eprintln!(
        "{} moved, {} pinned, {} shared, {failures} failed",
        rigs("moved"),
        rigs("pinned"),
        rigs("shared")
    );
    fs::write(
        &report,
        serde_json::to_vec_pretty(&serde_json::json!({
            "families": families.iter().map(describe).collect::<Vec<_>>(),
            "unbuildable_families": unbuildable,
            "pairs": results.len(),
            "moved": rigs("moved"),
            "pinned": rigs("pinned"),
            "shared": rigs("shared"),
            "failures": failures,
            "results": results,
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(failures, 0, "see {}", report.display());
}

fn spec(gameplay: &Weapon, appearance: Option<&Weapon>) -> WeaponCloneSpec {
    let namespace = format!(
        "parhelion.appearance-sweep.{:08x}.{:08x}",
        gameplay.0,
        appearance.map_or(0, |weapon| weapon.0)
    );
    // The picker allows a Power appearance on a primary weapon, and the reverse, only once the
    // authored weapon moves to the appearance's slot.
    let primary = |slot| matches!(slot, Some(Slot::Kinetic | Slot::Energy));
    let inventory_slot = appearance
        .filter(|weapon| weapon.3 != gameplay.3 && !(primary(weapon.3) && primary(gameplay.3)))
        .and_then(|weapon| weapon.3)
        .map(|slot| match slot {
            Slot::Kinetic => WeaponInventorySlot::Kinetic,
            Slot::Energy => WeaponInventorySlot::Energy,
            Slot::Power => WeaponInventorySlot::Power,
        });
    WeaponCloneSpec {
        kind: crate::ItemKind::Weapon,
        namespace: namespace.clone(),
        donor_item_hash: gameplay.0,
        expected_donor_name: Some(gameplay.1.clone()),
        presentation_donor: appearance.map(|weapon| WeaponPresentationDonorReference {
            item_hash: weapon.0,
            expected_name: Some(weapon.1.clone()),
        }),
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(&namespace).unwrap(),
        text: WeaponCloneText {
            name: format!("Sweep {:08X}", appearance.map_or(0, |weapon| weapon.0)),
            flavor: "Appearance sweep.".to_owned(),
            source: "Source: integration test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides {
            inventory_slot,
            ..WeaponCloneOverrides::default()
        },
    }
}

fn build(packages: &Path, weapons: Vec<WeaponCloneSpec>) -> Result<NewWeaponProjectBundle, String> {
    build_weapon_project(packages, &WeaponProjectSpec { weapons })
        .map_err(|error| format!("{error:?}"))
}

/// Stages one built project and reads back how each of its weapons wears its appearance.
fn check(
    packages: &Path,
    bundle: &NewWeaponProjectBundle,
    batch: &[&Pair],
    patterns: &BTreeMap<u32, u16>,
) -> Vec<Outcome> {
    let view = staged_view(packages, "appearance-sweep", bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let stock = open_manager(packages).unwrap();
    batch
        .iter()
        .map(|(gameplay, appearance)| {
            let item = spec(&gameplay.1, Some(&appearance.1)).identity.item_hash;
            let pattern = |weapon: &Weapon| {
                patterns
                    .get(&weapon.0)
                    .copied()
                    .ok_or_else(|| format!("{} has no pattern row", weapon.1))
            };
            match pattern(&gameplay.1)
                .and_then(|base| rig(&staged, &stock, item, base, pattern(&appearance.1)?))
            {
                Ok(rig) => Outcome {
                    error: None,
                    rig: Some(rig),
                },
                Err(problem) => Outcome {
                    error: Some(format!("read back: {problem}")),
                    rig: None,
                },
            }
        })
        .collect()
}

/// `moved` when the weapon wears the appearance's whole rig, `pinned` when it keeps the base's,
/// `shared` when both weapons have one rig. Anything mixed is the error.
///
/// A pinned build gives the first-person attachment a private copy holding the appearance
/// family's hold, so only that binding may differ from the base there.
fn rig(
    staged: &PackageManager,
    stock: &PackageManager,
    item: u32,
    base: u16,
    appearance: u16,
) -> Result<&'static str, String> {
    const ATTACHMENT: u32 = 0xD3A5_500E;
    const MARKER_SET: u32 = 0x04CE_0B42;
    let authored = load_weapon_runtime_entity_with_manager(staged, item)?;
    let base = load_weapon_runtime_entity_at_pattern_index_with_manager(stock, base)?;
    let look = load_weapon_runtime_entity_at_pattern_index_with_manager(stock, appearance)?;
    let owners = |entity: &[u8], binding: u32| {
        weapon_component_bindings(entity, binding)
            .ok()
            .map(|rows| rows.iter().map(|row| row.owner_tag).collect::<Vec<_>>())
    };
    let rig_of = |entity: &[u8]| PRESENTATION_BINDINGS.map(|binding| owners(entity, binding));
    let (built, theirs, ours) = (
        rig_of(&authored.payload),
        rig_of(&base.payload),
        rig_of(&look.payload),
    );
    if theirs == ours {
        return Ok("shared");
    }
    // A moved rig's marker set may be a private copy that also carries the base's markers.
    let moved = PRESENTATION_BINDINGS
        .iter()
        .zip(built.iter().zip(&ours))
        .all(|(&binding, (built, look))| binding == MARKER_SET || built == look);
    let (kind, worn, other) = if moved {
        let index = PRESENTATION_BINDINGS
            .iter()
            .position(|&b| b == MARKER_SET)
            .unwrap();
        let names = |manager: &PackageManager, entity: &[u8]| -> Result<BTreeSet<u32>, String> {
            let Some(owner) = owners(entity, MARKER_SET).and_then(|tags| tags.first().copied())
            else {
                return Ok(BTreeSet::new());
            };
            let owner = manager.read_tag(TagHash(owner))?;
            let data = crate::tag_payload::relative_target(&owner, 0x18)
                .map_err(|error| format!("{error:?}"))?;
            let (count, _, rows, _) = crate::tag_payload::array_at(&owner, data + 0xB0)
                .map_err(|error| format!("{error:?}"))?;
            (0..count)
                .map(|row| read_u32(&owner, rows + row * 64 + 0x30).map_err(|e| format!("{e:?}")))
                .collect()
        };
        if built[index] != ours[index] {
            let have = names(staged, &authored.payload)?;
            let wanted = names(stock, &base.payload)?
                .into_iter()
                .chain(names(stock, &look.payload)?)
                .collect::<BTreeSet<_>>();
            if let Some(missing) = wanted.difference(&have).next() {
                return Err(format!("moved rig's marker set lacks marker {missing:08X}"));
            }
        }
        ("moved", &look, &theirs)
    } else if PRESENTATION_BINDINGS
        .iter()
        .zip(built.iter().zip(&theirs))
        .all(|(&binding, (built, base))| binding == ATTACHMENT || built == base)
    {
        ("pinned", &base, &ours)
    } else {
        return Err(format!(
            "mixed rig: built {built:X?}, base {theirs:X?}, appearance {ours:X?}"
        ));
    };
    // The other rig's owners must not survive anywhere, including in aggregate bindings.
    let worn_rig = rig_of(&worn.payload);
    let stale = other
        .iter()
        .flatten()
        .flatten()
        .filter(|tag| !worn_rig.iter().flatten().flatten().any(|kept| kept == *tag))
        .collect::<BTreeSet<_>>();
    for binding in weapon_component_binding_hashes(&authored.payload)? {
        if let Some(tag) = owners(&authored.payload, binding)
            .into_iter()
            .flatten()
            .find(|tag| stale.contains(tag))
        {
            return Err(format!(
                "{kind} rig keeps the other rig's owner {tag:08X} in binding {binding:08X}"
            ));
        }
    }
    // With no Type Markers chosen, the block keeps the base weapon's type whichever rig it wears.
    let wanted =
        crate::weapon::behavior::markers(stock, &base.payload, base.weapon_content_group_hash)
            .map_err(|error| format!("{error:?}"))?;
    let found = crate::weapon::behavior::resolved_markers(
        staged,
        &authored.payload,
        authored.weapon_content_group_hash,
    )
    .map_err(|error| format!("{error:?}"))?;
    if wanted != found {
        return Err(format!(
            "{kind} rig's block names type {found:08X?}, the base's is {wanted:08X?}"
        ));
    }
    Ok(kind)
}

/// Runs `work` over `items` on `threads` workers and returns the results in item order.
fn parallel<T: Sync, R: Send>(
    threads: usize,
    items: &[T],
    work: impl Fn(usize, &T) -> R + Sync,
) -> Vec<R> {
    let next = Mutex::new(0usize);
    let results = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = {
                        let mut next = next.lock().unwrap();
                        *next += 1;
                        *next - 1
                    };
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let result = work(index, item);
                    results.lock().unwrap().push((index, result));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}
