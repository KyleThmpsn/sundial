//! Projectile curve settings and ability settings authored the way the Gameplay tab writes them:
//! found by Sundial's readers in a stock ability's graphs, set, built into a Subclass, and found
//! again by the same readers in the staged copies, with each stock ability as it shipped. A curve
//! is found only where the definition, the instance's curve state and the scale agree, so finding
//! the edited one proves all three were written. Settings kept as bytes of an opaque field are
//! written into the field's override, and two of one field read back together.
use super::*;
use crate::subclass::{Place, SubclassAbilities};
use sundial::package_authoring::ability_palette::ability_graphs;
use sundial::package_authoring::ability_settings::{self, Kind, Setting};
use sundial::package_authoring::runtime::{
    WeaponRuntimeValueOverride, load_weapon_runtime_graph_for_entity,
};
use sundial::package_authoring::sandbox_perk::entity::projectile::parameters::{
    self, Curve, CurveKind,
};

/// Every curve setting and ability setting of `entity`'s graphs, as the Gameplay tab finds them.
fn found(
    manager: &sundial::package_authoring::PackageManager,
    entity: u32,
) -> (Vec<Curve>, Vec<Setting>) {
    let (mut curves, mut settings) = (Vec::new(), Vec::new());
    for (graph, payload) in ability_graphs(manager, entity, crate::subclass::SPAWN_DEPTH).unwrap() {
        let Ok(mut runtime) = load_weapon_runtime_graph_for_entity(manager, 0, 0, graph, &payload)
        else {
            continue;
        };
        runtime.scope_fields();
        curves.extend(parameters::curves(&runtime));
        settings.extend(ability_settings::discover(manager, &runtime));
    }
    (curves, settings)
}

fn curve(curves: &[Curve], kind: CurveKind) -> &Curve {
    curves
        .iter()
        .find(|curve| curve.kind == kind)
        .unwrap_or_else(|| panic!("no {} curve setting", kind.label()))
}

fn setting(settings: &[Setting], kind: Kind) -> &Setting {
    settings
        .iter()
        .find(|setting| setting.kind == kind)
        .unwrap_or_else(|| panic!("no {} setting", kind.label()))
}

/// The settings the survey's later rounds promoted: those read from a component program's one
/// constant, from both constants of tracking's turn-rate program or from the constant a spawner
/// program adds last, and an invisibility's grace period.
const PROGRAM_KINDS: [Kind; 25] = [
    Kind::DisruptionGracePeriod,
    Kind::DamageBreakResponse,
    Kind::StrengthLoss,
    Kind::QueryScale,
    Kind::TriggeredDuration,
    Kind::EnergyCost,
    Kind::RequiredEnergy,
    Kind::TrackingStrength,
    Kind::DistanceTracking,
    Kind::BounceTracking,
    Kind::TrackingVariation,
    Kind::TrackingSpeed,
    Kind::AcquisitionScale,
    Kind::TargetLimit,
    Kind::SpawnChance,
    Kind::SpawnCount,
    Kind::GenerationLimit,
    Kind::SpawnLimit,
    Kind::ConditionalDamage,
    Kind::FastThrowTracking,
    Kind::TurnRateOffset,
    Kind::AcquisitionScaleOffset,
    Kind::SpawnCountOffset,
    Kind::GenerationLimitOffset,
    Kind::SpawnLimitOffset,
];

/// The settings a definition keeps as bytes of an opaque field: a Glide's phase, a preparation's
/// impulse timing, Blink's direction, a projectile's clocks, limits and contacts, tracking's lead
/// and proximity, a target query's delay, range and owner exception, and a controller's energy.
const LANE_KINDS: [Kind; 32] = [
    Kind::ImpulseHoldTime,
    Kind::FallSpeedThreshold,
    Kind::ImpulseRampTime,
    Kind::ImpulseFadeTime,
    Kind::VerticalBias,
    Kind::MinimumFinishTime,
    Kind::MaximumFinishTime,
    Kind::FinishTravelTime,
    Kind::ExpirationTime,
    Kind::ExpirationUpdates,
    Kind::ExpirationResponse,
    Kind::PierceLimit,
    Kind::BounceLimit,
    Kind::CollisionMode,
    Kind::CollisionRadius,
    Kind::TurnRate,
    Kind::LeadTimeLimit,
    Kind::LeadDistanceLimit,
    Kind::SteeringAxisThreshold,
    Kind::TargetLead,
    Kind::ProximityRange,
    Kind::MinimumProximityTime,
    Kind::MaximumProximityTime,
    Kind::SearchDelay,
    Kind::SearchRange,
    Kind::IncludeSelf,
    Kind::ActivationEnergy,
    Kind::ActiveEnergyRate,
    Kind::EndingEnergyCost,
    Kind::RechargeDelay,
    Kind::MinimumActivationEnergy,
    Kind::EnergyFloor,
];

/// Asserts that the abilities `entities` together offer every setting of `kinds`.
fn assert_offered(
    manager: &sundial::package_authoring::PackageManager,
    entities: &[u32],
    kinds: &[Kind],
) {
    let offered = entities
        .iter()
        .flat_map(|entity| found(manager, *entity).1)
        .map(|setting| setting.kind)
        .collect::<Vec<_>>();
    for kind in kinds {
        assert!(
            offered.contains(kind),
            "a stock ability offers {}",
            kind.label()
        );
    }
}

/// One authored ability: its entry, and each value the test sets by what it is. A setting
/// without a value turns its stock flag the other way.
struct Authored {
    entry: u8,
    curves: Vec<(CurveKind, f32)>,
    settings: Vec<(Kind, Option<f32>)>,
}

/// Sets each wanted value on `entity`'s stock settings. Returns the overrides and the value each
/// setting took.
fn author(
    manager: &sundial::package_authoring::PackageManager,
    entity: u32,
    authored: &Authored,
) -> (Vec<WeaponRuntimeValueOverride>, Vec<(Kind, f32)>) {
    let (curves, settings) = found(manager, entity);
    let mut values = Vec::new();
    for (kind, value) in &authored.curves {
        let curve = curve(&curves, *kind);
        assert_ne!(curve.original(), *value, "{} changes", kind.label());
        curve.set(&mut values, *value).unwrap();
    }
    let mut taken = Vec::new();
    for (kind, value) in &authored.settings {
        let setting = setting(&settings, *kind);
        let value = value.unwrap_or(if setting.stock() >= 0.5 { 0.0 } else { 1.0 });
        assert_ne!(setting.stock(), value, "{} changes", kind.label());
        setting.set(&mut values, value).unwrap();
        taken.push((*kind, value));
    }
    (values, taken)
}

/// Checks that `copy`, the build of `each`, reads every authored value back. Returns each setting
/// that does not, with what the copy holds for it, so one run names every miss.
fn read_back(
    staged: &sundial::package_authoring::PackageManager,
    copy: u32,
    each: &Authored,
    wanted: &[(Kind, f32)],
    name: &str,
) -> Vec<String> {
    let (curves, settings) = found(staged, copy);
    for (kind, value) in &each.curves {
        assert!(
            curves
                .iter()
                .any(|curve| curve.kind == *kind && curve.original() == *value),
            "{name} entry {} reads {} {value} back with its curve state and scale",
            each.entry,
            kind.label()
        );
    }
    let mut missed = Vec::new();
    for (kind, value) in wanted {
        let held = settings
            .iter()
            .filter(|setting| setting.kind == *kind)
            .map(Setting::stock)
            .collect::<Vec<_>>();
        // A filter whose source test is off holds the empty key, which leaves no stock key to
        // restore and nothing to invert, so its copy offers neither.
        let read = if *kind == Kind::SourceFilter && *value == 0.0 {
            held.is_empty()
                && !settings
                    .iter()
                    .any(|setting| setting.kind == Kind::InvertSourceFilter)
        } else {
            held.contains(value)
        };
        if !read {
            missed.push(format!(
                "{name} entry {} reads {} {value} back, holds {held:?}",
                each.entry,
                kind.label()
            ));
        }
    }
    // A dormant retirement delay stays editable so the author can inspect or reset it.
    for off in settings
        .iter()
        .filter(|setting| setting.kind == Kind::RetireOnRemoval && setting.stock() == 0.0)
    {
        assert!(
            settings.iter().any(|setting| {
                setting.kind == Kind::RetirementDelay
                    && setting.owner_tag == off.owner_tag
                    && setting.offset() + 4 == off.offset()
            }),
            "{name} entry {} retains its inactive Retirement Delay",
            each.entry
        );
    }
    missed
}

/// Decode the paired curve and scale directly. The native reset clamps a reversed interval to
/// zero at D00D31..D00D58, independently of the editor's curve reader or its scale calculation.
fn read_back_reversed_curve(
    manager: &sundial::package_authoring::PackageManager,
    entity: u32,
) -> serde_json::Value {
    let mut matched = BTreeMap::new();
    for (graph, payload) in ability_graphs(manager, entity, crate::subclass::SPAWN_DEPTH).unwrap() {
        let Ok(runtime) = load_weapon_runtime_graph_for_entity(manager, 0, 0, graph, &payload)
        else {
            continue;
        };
        for resource in runtime.resources {
            let Some(definition) = resource.definition else {
                continue;
            };
            if resource.instance.schema != 0x8080_3B73 || definition.schema != 0x8080_388F {
                continue;
            }
            let bytes = manager.read_tag(TagHash(resource.owner_tag)).unwrap();
            // 37B3 at +C8 is a signed relative reference. Its target has a 3803 type word
            // immediately before the four floats, rather than storing them inline at +C8.
            let reference_at = definition.owner_offset as usize + 0xC8;
            let relative =
                i64::from_le_bytes(bytes[reference_at..reference_at + 8].try_into().unwrap());
            if relative == 0 {
                continue;
            }
            let definition_at = reference_at
                .checked_add_signed(isize::try_from(relative).unwrap())
                .expect("curve reference stays in its containing owner");
            assert!(definition_at >= 4 && definition_at + 16 <= bytes.len());
            let class =
                u32::from_le_bytes(bytes[definition_at - 4..definition_at].try_into().unwrap());
            assert_eq!(
                class, 0x8080_3803,
                "curve reference names its typed settings"
            );
            let state_at = resource.instance.owner_offset as usize + 0x14C;
            let words = |at| {
                std::array::from_fn::<_, 4, _>(|index| {
                    u32::from_le_bytes(
                        bytes[at + index * 4..at + index * 4 + 4]
                            .try_into()
                            .unwrap(),
                    )
                })
            };
            let configured = words(definition_at);
            if configured[0] != 42.0_f32.to_bits()
                || configured[2] != 64.0_f32.to_bits()
                || configured[3] != 33.0_f32.to_bits()
            {
                continue;
            }
            let state = words(state_at);
            let scale_at = resource.instance.owner_offset as usize + 0x15C;
            let scale = u32::from_le_bytes(bytes[scale_at..scale_at + 4].try_into().unwrap());
            assert_eq!(
                state, configured,
                "curve reset state matches its definition"
            );
            assert_eq!(
                scale, 0,
                "native reset clamps this reversed curve scale to zero"
            );
            matched.insert(
                resource.owner_tag,
                serde_json::json!({
                    "curve_owner": format!("0x{:08X}", resource.owner_tag),
                    "definition_offset": definition_at,
                    "state_offset": state_at,
                    "definition_bits": configured,
                    "state_bits": state,
                    "scale_offset": scale_at,
                    "scale_bits": scale,
                }),
            );
        }
    }
    assert_eq!(matched.len(), 1, "one authored reversed curve reads back");
    matched.into_values().next().unwrap()
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to Shadowkeep packages"]
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn real_ability_settings_build_into_private_copies() {
    let packages = crate::test_support::stock_packages();
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let install = view.path().parent().unwrap();
    fs::write(install.join("destiny2.exe"), []).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let cache_path = temporary.path().join("catalog.json");
    let baseline =
        InvestmentCatalog::load_with_cache_path(install, &cache_path, true, |_| {}).unwrap();
    let subclasses = baseline.subclasses(crate::package_profile::is_stock_item_definition);
    let base = |name: &str| {
        subclasses
            .iter()
            .find(|subclass| subclass.name == name)
            .unwrap_or_else(|| panic!("no stock {name}"))
            .clone()
    };
    // Nova Bomb flies on a speed and gravity curve, tracks and scales the damage it takes, also
    // under a condition. Two of its projectile settings share one opaque field, and it bounces
    // and searches for targets. Axion Bolt's seekers query, track and spawn from programs, and
    // spend energy through a gate, and two of its spawn programs add a constant to what their
    // inputs give. Healing Rift's object has health regions and an owner filter.
    // Strafe Glide holds its first phase and ramps its preparation impulse, and Blink tilts its
    // direction and waits before it recharges. Marksman's Dodge's invisibility breaks and
    // retires, and Gambler's Dodge waits before it retires. Shadowshot's tether lasts a while
    // after its query finds a target, its arrow counts updates, and its damage filter drops the
    // source test its stock key sets. Fusion Grenade turns at a rate a program computes from its
    // fast-throw input and an offset.
    let plans = [
        (
            base("Voidwalker"),
            vec![
                Authored {
                    entry: 10,
                    curves: vec![
                        (CurveKind::FinalSpeed, 42.0),
                        (CurveKind::Start, 64.0),
                        (CurveKind::End, 33.0),
                    ],
                    settings: vec![
                        (Kind::IncomingDamage, Some(0.5)),
                        (Kind::ConditionalDamage, Some(0.35)),
                        (Kind::TrackingStrength, Some(1.6)),
                        (Kind::ExpirationTime, Some(21.0)),
                        (Kind::CollisionRadius, Some(0.6)),
                        (Kind::BounceLimit, Some(7.0)),
                        (Kind::SearchRange, Some(18.0)),
                    ],
                },
                Authored {
                    entry: 4,
                    curves: Vec::new(),
                    settings: vec![
                        (Kind::ImpulseHoldTime, Some(0.25)),
                        (Kind::ImpulseRampTime, Some(0.4)),
                    ],
                },
                Authored {
                    entry: 5,
                    curves: Vec::new(),
                    settings: vec![
                        (Kind::VerticalBias, Some(2.0)),
                        (Kind::RechargeDelay, Some(3.5)),
                    ],
                },
                Authored {
                    entry: 9,
                    curves: Vec::new(),
                    settings: vec![
                        (Kind::QueryScale, Some(1.75)),
                        (Kind::EnergyCost, Some(0.35)),
                        (Kind::DistanceTracking, Some(1.7)),
                        (Kind::SpawnCount, Some(7.0)),
                        (Kind::AcquisitionScaleOffset, Some(1.5)),
                        (Kind::SpawnLimitOffset, Some(4.0)),
                    ],
                },
                Authored {
                    entry: 2,
                    curves: Vec::new(),
                    settings: vec![
                        (Kind::HealthScale, Some(2.5)),
                        (Kind::OwnerDamageOnly, None),
                    ],
                },
            ],
        ),
        (
            base("Nightstalker"),
            vec![
                Authored {
                    entry: 2,
                    curves: Vec::new(),
                    settings: vec![
                        (Kind::DamageBreakThreshold, Some(3.5)),
                        (Kind::IgnoreMovement, None),
                        (Kind::RetireOnRemoval, Some(0.0)),
                    ],
                },
                Authored {
                    entry: 3,
                    curves: Vec::new(),
                    settings: vec![
                        (Kind::RetirementDelay, Some(1.5)),
                        (Kind::StrengthLoss, Some(0.25)),
                        (Kind::DisruptionGracePeriod, Some(1.25)),
                    ],
                },
                Authored {
                    entry: 10,
                    curves: Vec::new(),
                    settings: vec![
                        (Kind::TriggeredDuration, Some(2.5)),
                        (Kind::ExpirationUpdates, Some(3.0)),
                        (Kind::SourceFilter, Some(0.0)),
                    ],
                },
            ],
        ),
        (
            base("Sunbreaker"),
            vec![Authored {
                entry: 7,
                curves: Vec::new(),
                settings: vec![
                    (Kind::FastThrowTracking, Some(500.0)),
                    (Kind::TurnRateOffset, Some(300.0)),
                ],
            }],
        ),
    ];
    let source = open_manager(view.path()).unwrap();
    // Each program and lane setting sits on one of these stock abilities: Strafe Glide, Blink,
    // Axion Bolt, Nova Bomb, the melees Entropic Pull, Devour and Atomic Breach reach, Shadowshot,
    // Marksman's and Gambler's Dodge for invisibility, Fusion Grenade for its turn-rate program,
    // and Arcbolt Grenade, whose chain depth adds a constant to its input. Finding every one
    // proves each place.
    let census = [
        ("Voidwalker", &[4, 5, 9, 10, 11, 15, 21][..]),
        ("Nightstalker", &[2, 3, 10][..]),
        ("Sunbreaker", &[7][..]),
        ("Arcstrider", &[7][..]),
    ]
    .into_iter()
    .flat_map(|(name, entries)| {
        let base = base(name);
        entries
            .iter()
            .filter_map(|entry| base.entry_entities.get(entry).copied())
            .collect::<Vec<_>>()
    })
    .collect::<Vec<_>>();
    assert_offered(&source, &census, &PROGRAM_KINDS);
    assert_offered(&source, &census, &LANE_KINDS);
    // Burning Maul's part 80BC8832 holds a tracking definition, owned by 80BC41B1, whose
    // proximity event row names no receiver, so its range and timers do nothing. The tracking is
    // still read, and only those three are withheld.
    let maul = found(&source, base("Sunbreaker").entry_entities[&20]).1;
    let offers = |kinds: &[Kind]| {
        maul.iter()
            .any(|setting| setting.owner_tag == 0x80BC_41B1 && kinds.contains(&setting.kind))
    };
    assert!(
        offers(&[Kind::TargetLead]),
        "Burning Maul reads its tracking"
    );
    assert!(
        !offers(&[
            Kind::ProximityRange,
            Kind::MinimumProximityTime,
            Kind::MaximumProximityTime,
        ]),
        "Burning Maul offers no proximity settings without a receiver"
    );
    let mut recipes = Vec::new();
    let mut stock = Vec::new();
    let mut taken = BTreeMap::<(u32, u8), Vec<(Kind, f32)>>::new();
    let mut movement_banks = Vec::new();
    let mut typed_swap = None;
    for (base, authored) in &plans {
        let mut recipe = crate::WeaponRecipe::new_unbound_kind(crate::ItemKind::Subclass).unwrap();
        recipe.set_donor(base.hash, base.name.clone());
        recipe
            .rename_authored_item(format!("Settings {}", base.name))
            .unwrap();
        let mut abilities = SubclassAbilities::default();
        for each in authored {
            let place = Place::Ability(each.entry);
            let entity = base.entry_entities[&each.entry];
            stock.push((entity, found(&source, entity)));
            let mut edits = abilities.edits(base.hash, place);
            let (values, settings) = author(&source, entity, each);
            edits.ability_values = values;
            taken.insert((base.hash, each.entry), settings);
            abilities.set_edits(base.hash, place, edits);
        }
        // Voidwalker's third grenade fires Fusion Grenade's projectile in place of its own,
        // dealing a damage type of its own that neither grenade deals.
        if base.name == "Voidwalker" {
            // An active melee moved into a normally passive position keeps its own controls.
            // A modifier from another node must resolve the moved destination's private row.
            use crate::subclass::{
                AbilityModifier, AttunementPath, ModifierEffect, SubclassPathNode,
            };
            let mut moved = SubclassPathNode::stock(1, base.hash, AttunementPath::Middle, 1);
            moved.edits.extra_charges = 1;
            moved.edits.set_recharge(Some(1.25));
            abilities.set_path_node(AttunementPath::Top, base.hash, moved);
            let mut trigger = SubclassPathNode::stock(2, base.hash, AttunementPath::Top, 2);
            trigger.edits.modifiers.push(AbilityModifier {
                target: AttunementPath::Top.entries()[1],
                effect: ModifierEffect::Charges { count: 1 },
            });
            abilities.set_path_node(AttunementPath::Top, base.hash, trigger);
            let entity = base.entry_entities[&8];
            let donor = plans
                .iter()
                .find(|(other, _)| other.name == "Sunbreaker")
                .unwrap()
                .0
                .entry_entities[&7];
            let mut swap = projectile_swap(&source, entity, donor);
            swap.damage_type = Some(undealt_damage_type(&source, &[entity, swap.replacement]));
            let place = Place::Ability(8);
            let mut edits = abilities.edits(base.hash, place);
            edits.set_swap(swap.graph, swap.replaced, Some(swap.replacement));
            edits.set_swap_damage(swap.graph, swap.replaced, swap.damage_type);
            abilities.set_edits(base.hash, place, edits);
            typed_swap = Some(swap);
        }
        recipe.overrides.subclass_abilities = Some(abilities);
        if base.name == "Sunbreaker" {
            use sundial::package_authoring::{ability_modifier, ability_movement};
            let entry = 4;
            let entity = base.entry_entities[&entry];
            let payload = source.read_tag(tiger_pkg::TagHash(entity)).unwrap();
            let bank_tag = ability_modifier::entity_bank(&payload).unwrap().unwrap();
            let bank = source.read_tag(tiger_pkg::TagHash(bank_tag)).unwrap();
            let row = base.entry_rows[&entry];
            let keys = base.entry_modifiers[&entry]
                .iter()
                .filter(|(_, target)| *target == row)
                .map(|(key, _)| *key)
                .collect::<Vec<_>>();
            let lanes = ability_movement::row_lanes(&bank, &keys).unwrap();
            let abilities = recipe.overrides.subclass_abilities.as_mut().unwrap();
            let mut edits = abilities.edits(base.hash, Place::Ability(entry));
            let mut checks = Vec::new();
            for (label, value) in [
                ("Impulse Height Limit", 7.5_f32),
                ("Active Energy Rate", 0.17_f32),
            ] {
                let lane = lanes
                    .iter()
                    .find(|lane| lane.label == label)
                    .unwrap_or_else(|| panic!("High Lift exposes its selected {label}"));
                edits.set_bank_value(lane.key, lane.row, lane.lane, Some(value.to_bits()));
                checks.push((lane.offset, lane.stock, value.to_bits(), label));
            }
            abilities.set_edits(base.hash, Place::Ability(entry), edits);
            movement_banks.push((base.name.clone(), entry, entity, checks));
        }
        let json = recipe.to_json_pretty().unwrap();
        let reloaded = crate::WeaponRecipe::from_json_str(&json).unwrap();
        assert_eq!(
            reloaded, recipe,
            "authored settings survive a persisted recipe reload"
        );
        crate::test_support::artifact(&format!("settings-{}.recipe.json", base.name), &reloaded);
        recipes.push(reloaded);
    }
    let curve_payloads = stock
        .iter()
        .flat_map(|(_, (curves, _))| curves.iter())
        .map(|curve| {
            (
                curve.owner_tag,
                source.read_tag(TagHash(curve.owner_tag)).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    drop(source);

    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: view.path().to_path_buf(),
        staging_root: temporary.path().join("staging"),
        ignore_installed_authored_overlays: false,
        recipes,
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let staged = open_manager(view.path()).unwrap();
    let catalog =
        InvestmentCatalog::load_with_cache_path(install, &cache_path, true, |_| {}).unwrap();
    let staged_subclasses = catalog.subclasses(|_| true);
    let mut receipt = Vec::new();
    let mut missed = Vec::new();
    for (base, authored) in &plans {
        let name = format!("Settings {}", base.name);
        let subclass = staged_subclasses
            .iter()
            .find(|subclass| subclass.name == name)
            .unwrap_or_else(|| panic!("{name} reads back"));
        for each in authored {
            let copy = subclass.entry_entities[&each.entry];
            assert_ne!(
                copy, base.entry_entities[&each.entry],
                "{name} entry {} is a copy",
                each.entry
            );
            let wanted = &taken[&(base.hash, each.entry)];
            missed.extend(read_back(&staged, copy, each, wanted, &name));
            if !each.curves.is_empty() {
                receipt.push(read_back_reversed_curve(&staged, copy));
            }
            receipt.push(serde_json::json!({
                "subclass": name,
                "entry": each.entry,
                "copy": format!("0x{copy:08X}"),
                "curves": each.curves.iter().map(|(kind, value)| (kind.label(), value)).collect::<Vec<_>>(),
                "settings": wanted.iter().map(|(kind, value)| (kind.label(), value)).collect::<Vec<_>>(),
            }));
        }
        if let Some(swap) = typed_swap.filter(|_| base.name == "Voidwalker") {
            let entries = crate::subclass::AttunementPath::Top.entries();
            let row = subclass.entry_rows[&entries[1]];
            let key = sundial::package_authoring::ability_modifier::charge_key(1);
            for entry in [entries[1], entries[2]] {
                assert!(
                    subclass.entry_modifiers[&entry].contains(&(key, row)),
                    "the moved ability and its modifier both reach its equipped row"
                );
            }
            receipt.push(serde_json::json!({"moved_ability_entry": entries[1], "row": row, "charge_key": key}));
            let copy = subclass.entry_entities[&8];
            assert_ne!(copy, base.entry_entities[&8], "{name} entry 8 is a copy");
            receipt.push(read_back_swap_damage(&staged, copy, &swap, &name));
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
    for (name, entry, entity, checks) in movement_banks {
        let subclass = staged_subclasses
            .iter()
            .find(|s| s.name == format!("Settings {name}"))
            .unwrap();
        let bank_of = |copy| {
            let tag = super::stock::copied_bank(&staged, entity, copy);
            staged.read_tag(tiger_pkg::TagHash(tag)).unwrap()
        };
        let stock = bank_of(entity);
        let private = bank_of(subclass.entry_entities[&entry]);
        for (offset, original, expected, label) in checks {
            assert_eq!(
                u32::from_le_bytes(stock[offset..offset + 4].try_into().unwrap()),
                original,
                "stock {label} is unchanged"
            );
            assert_eq!(
                u32::from_le_bytes(private[offset..offset + 4].try_into().unwrap()),
                expected,
                "the private movement bank holds {label}"
            );
            receipt.push(serde_json::json!({"movement": name, "property": label,
                "offset": offset, "original_bits": original, "emitted_bits": expected}));
        }
    }
    // The stock abilities keep every stock value.
    for (entity, (curves, settings)) in &stock {
        let (now_curves, now_settings) = found(&staged, *entity);
        assert_eq!(
            now_curves
                .iter()
                .map(|curve| (curve.kind, curve.original().to_bits()))
                .collect::<Vec<_>>(),
            curves
                .iter()
                .map(|curve| (curve.kind, curve.original().to_bits()))
                .collect::<Vec<_>>(),
            "stock 0x{entity:08X} keeps its curves"
        );
        assert_eq!(
            now_settings
                .iter()
                .map(|setting| (setting.kind, setting.stock().to_bits()))
                .collect::<Vec<_>>(),
            settings
                .iter()
                .map(|setting| (setting.kind, setting.stock().to_bits()))
                .collect::<Vec<_>>(),
            "stock 0x{entity:08X} keeps its settings"
        );
    }
    for (owner, original) in curve_payloads {
        assert_eq!(
            staged.read_tag(TagHash(owner)).unwrap(),
            original,
            "stock curve owner 0x{owner:08X} keeps every byte"
        );
    }
    crate::test_support::artifact("ability-settings.json", &receipt);
}

/// Each damage profile the graphs below `root` name: graph, profile and damage type.
fn damage_profiles(
    manager: &sundial::package_authoring::PackageManager,
    root: u32,
) -> Vec<(u32, u32, u8)> {
    use sundial::package_authoring::ability_damage::references;
    ability_graphs(manager, root, crate::subclass::SPAWN_DEPTH)
        .unwrap()
        .into_iter()
        .flat_map(|(graph, payload)| {
            references(manager, graph, &payload)
                .unwrap()
                .into_iter()
                .map(move |(place, profile)| (graph, place.graph, profile.mode))
        })
        .collect()
}

/// A damage type none of the profiles below `roots` deal, so every one is copied.
fn undealt_damage_type(
    manager: &sundial::package_authoring::PackageManager,
    roots: &[u32],
) -> crate::recipe::RecipeDamageType {
    use crate::recipe::RecipeDamageType;
    let dealt = roots
        .iter()
        .flat_map(|&root| damage_profiles(manager, root))
        .map(|(.., mode)| mode)
        .collect::<BTreeSet<_>>();
    assert!(
        !dealt.is_empty(),
        "the grenade's graphs name damage profiles"
    );
    [
        RecipeDamageType::Arc,
        RecipeDamageType::Solar,
        RecipeDamageType::Void,
        RecipeDamageType::Kinetic,
    ]
    .into_iter()
    .find(|damage| !dealt.contains(&crate::subclass::damage_mode(*damage)))
    .expect("a damage type neither the grenade nor the projectile swapped in deals")
}

/// The first projectile `entity` names, in place of which it fires the first projectile below
/// `donor` within three levels.
fn projectile_swap(
    manager: &sundial::package_authoring::PackageManager,
    entity: u32,
    donor: u32,
) -> crate::subclass::SpawnSwap {
    use sundial::package_authoring::ability_spawns::spawned_graphs;
    let is_projectile = |graph: u32| {
        manager
            .read_tag(TagHash(graph))
            .ok()
            .is_some_and(|payload| payload.get(0x96) == Some(&18))
    };
    let payload = manager.read_tag(TagHash(entity)).unwrap();
    let replaced = spawned_graphs(manager, entity, &payload)
        .unwrap()
        .into_iter()
        .find(|graph| is_projectile(*graph))
        .unwrap_or_else(|| panic!("ability entity 0x{entity:08X} names a projectile to swap"));
    let mut seen = BTreeSet::new();
    let mut queue = std::collections::VecDeque::from([(donor, 0usize)]);
    let replacement = loop {
        let Some((graph, depth)) = queue.pop_front() else {
            panic!("donor 0x{donor:08X} names no projectile within three levels");
        };
        if !seen.insert(graph) {
            continue;
        }
        if graph != replaced && is_projectile(graph) {
            break graph;
        }
        if depth < 3 {
            let payload = manager.read_tag(TagHash(graph)).unwrap();
            for child in spawned_graphs(manager, graph, &payload).unwrap() {
                queue.push_back((child, depth + 1));
            }
        }
    };
    crate::subclass::SpawnSwap {
        graph: entity,
        replaced,
        replacement,
        damage_type: None,
    }
}

/// The swap read back from the staged copy `copied` of its grenade: the copy fires one private
/// copy of the replacement where the stock grenade fired the replaced projectile, every damage
/// profile below that copy is a private one dealing the swap's type, the stock projectile keeps
/// its own, and so do the grenade's own graphs. Returns what the receipt records of it.
fn read_back_swap_damage(
    staged: &sundial::package_authoring::PackageManager,
    copied: u32,
    swap: &crate::subclass::SpawnSwap,
    name: &str,
) -> serde_json::Value {
    use sundial::package_authoring::ability_spawns::{self, spawns};
    let damage = swap.damage_type.expect("the swap names a damage type");
    let mode = crate::subclass::damage_mode(damage);
    let place =
        |spawn: &ability_spawns::Spawn| (spawn.binding_hash, spawn.resource_index, spawn.offset);
    let stock = staged.read_tag(TagHash(swap.graph)).unwrap();
    let places = spawns(staged, swap.graph, &stock)
        .unwrap()
        .iter()
        .filter(|spawn| spawn.graph == swap.replaced)
        .map(place)
        .collect::<BTreeSet<_>>();
    assert!(
        !places.is_empty(),
        "{name}: the stock grenade names the replaced projectile"
    );
    let payload = staged.read_tag(TagHash(copied)).unwrap();
    let named = spawns(staged, copied, &payload)
        .unwrap()
        .iter()
        .filter(|spawn| places.contains(&place(spawn)))
        .map(|spawn| spawn.graph)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        named.len(),
        1,
        "{name}: the copy names one projectile where the stock grenade named 0x{:08X}",
        swap.replaced
    );
    let private = *named.first().unwrap();
    assert!(
        private != swap.replacement && private != swap.replaced,
        "{name}: the copy fires a private copy of projectile 0x{:08X}",
        swap.replacement
    );
    let retyped = damage_profiles(staged, private);
    assert!(
        !retyped.is_empty(),
        "{name}: the private projectile names damage profiles"
    );
    assert!(
        retyped.iter().all(|(.., each)| *each == mode),
        "{name}: the private projectile deals {damage:?} through every damage profile it names"
    );
    let stock_profiles = damage_profiles(staged, swap.replacement);
    let stock_tags = stock_profiles
        .iter()
        .map(|(_, tag, _)| *tag)
        .collect::<BTreeSet<_>>();
    assert!(
        retyped.iter().all(|(_, tag, _)| !stock_tags.contains(tag)),
        "{name}: the private projectile names private copies of its damage profiles"
    );
    assert!(
        stock_profiles.iter().all(|(.., each)| *each != mode),
        "the stock projectile keeps its own damage type"
    );
    let own = damage_profiles(staged, copied)
        .into_iter()
        .filter(|(graph, ..)| !retyped.iter().any(|(retyped, ..)| retyped == graph))
        .collect::<Vec<_>>();
    assert!(
        own.iter().all(|(.., each)| *each != mode),
        "{name}: the grenade keeps its own damage type outside the swapped projectile"
    );
    serde_json::json!({
        "subclass": name,
        "entry": 8,
        "copy": format!("0x{copied:08X}"),
        "swap": {
            "replaced": format!("0x{:08X}", swap.replaced),
            "replacement": format!("0x{:08X}", swap.replacement),
            "private_copy": format!("0x{private:08X}"),
            "damage_type": format!("{damage:?}"),
            "profile_mode": mode,
            "retyped_profiles": retyped.iter().map(|(_, tag, _)| format!("0x{tag:08X}")).collect::<BTreeSet<_>>(),
        },
    })
}
