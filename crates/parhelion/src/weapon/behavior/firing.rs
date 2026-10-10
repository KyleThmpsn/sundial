//! Borrowed plugs' firing and magazine changes, fitted to the weapon as private plugs.
use super::*;

/// Whose firing pattern a weapon uses when a borrowed plug changes how many rounds it fires.
///
/// A stock plug's burst change is an addition written for its own weapon type. Graviton Lance's
/// Black Hole always attaches an entity that takes one round from the barrel's Rounds per Burst,
/// which turns a pulse rifle's three into Graviton's two. An auto rifle fires one, so the same plug
/// left Arc Lance, Graviton on Arc Logic, with none and it could not fire at all. Every weapon type
/// that fires one round per pull failed the same way, and Bastion's Saint's Fists takes four.
///
/// Either way, on a weapon of another type the borrowed perks leave out the cuts sized for the
/// source weapon's own numbers: a lower fire rate or shorter time between shots given as an
/// amount, and any smaller magazine or reserve. Saint's Fists takes 25 shots a second, which a
/// fusion rifle's bolts absorb and an auto rifle's ten a second cannot, and its 0.35 magazine
/// leaves a one-round launcher with none. Increases stay, since they cannot empty anything.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BehaviorFiring {
    /// Fire the burst the behavior's source weapon fires. An auto rifle wearing Graviton Lance's
    /// behavior fires Graviton's two-round burst, so its second-shot perk still has a second shot.
    #[default]
    Behavior,
    /// Keep the weapon's own burst and shot timing. The borrowed plugs keep every other effect.
    Weapon,
}

/// Rounds one pull of the trigger fires before any perk changes it, for a weapon of this type.
///
/// The packages keep this in a channel the weapon's content programs write, which Parhelion does
/// not evaluate, so the figures come from the stock perks that change it, read against the burst
/// their own descriptions give:
///
/// - Pulse rifles fire three. Harsh Truths adds two to Vigilance Wing for its 5-round burst, and
///   Black Hole takes one from Graviton Lance for its two.
/// - Fusion rifles fire seven. Saint's Fists takes four from Bastion to fire 3 spreads, and
///   Jötunn's own row takes six to fire one projectile.
/// - Every other type fires one. Banned Weapon adds two to Crimson for a three-round burst, and
///   Twintails adds one to Two-Tailed Fox for its double rockets.
///
/// Lord of Wolves may be an exception: it shares the pulse rifles' stat translation group, so it
/// can start from three rather than a shotgun's one.
fn base_rounds_per_burst(type_name: &str) -> f32 {
    match type_name.trim() {
        "Pulse Rifle" => 3.0,
        "Fusion Rifle" => 7.0,
        _ => 1.0,
    }
}

/// One stored change to a barrel firing input or a magazine input, inside an entity a stock perk
/// attaches.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FiringRecord {
    /// Locates the record's amount inside the graph the perk attaches.
    pub(crate) amount: sundial::package_authoring::runtime::WeaponRuntimeFieldLocator,
    /// The component interface the record changes, the barrel or the magazine.
    pub(crate) component: i64,
    /// The input the record changes within that component.
    pub(crate) input: i64,
    /// Whether the amount adds or multiplies.
    pub(crate) operation: i64,
    pub(crate) value: f32,
    /// Whether the perk attaches the record's entity as soon as the plug is equipped and keeps
    /// it attached. Only such a record can carry a change that holds between activations.
    pub(crate) always: bool,
}

impl FiringRecord {
    fn adds(&self) -> bool {
        self.operation == sundial::package_authoring::runtime::modifiers::OPERATION_ADD
    }

    fn multiplies(&self) -> bool {
        self.operation == sundial::package_authoring::runtime::modifiers::OPERATION_MULTIPLY
    }

    /// The amount that leaves its input as it was.
    fn neutral(&self) -> f32 {
        if self.multiplies() { 1.0 } else { 0.0 }
    }

    /// Whether the record changes this barrel burst lane.
    pub(crate) fn in_burst_lane(&self, input: i64) -> bool {
        use sundial::package_authoring::runtime::modifiers;
        self.component == modifiers::BARREL_COMPONENT
            && self.input == input
            && modifiers::BARREL_ROUNDS_PER_BURST.contains(&input)
    }

    /// Whether the record changes how quickly the barrel fires.
    fn times_fire(&self) -> bool {
        use sundial::package_authoring::runtime::modifiers;
        self.component == modifiers::BARREL_COMPONENT
            && modifiers::BARREL_FIRE_TIMING.contains(&self.input)
    }

    /// Whether the record changes the magazine or reserves.
    fn holds_ammo(&self) -> bool {
        self.component == sundial::package_authoring::runtime::modifiers::MAGAZINE_COMPONENT
    }

    /// Whether the record's amount was sized for the source weapon's own numbers in a way that
    /// can empty a weapon of another type.
    ///
    /// A rate or interval given as an amount can cross zero: 25 fewer shots a second takes an
    /// auto rifle's ten below nothing, and 0.1 seconds less between shots leaves one firing ten a
    /// second with no interval at all. A factor cannot, so it stays. Magazine and reserve counts are
    /// whole rounds, so a factor below one can round a small magazine down to nothing as well.
    fn cuts_for_its_own_type(&self) -> bool {
        let fire_rate_cut = self.times_fire() && self.adds() && self.value < 0.0;
        let ammo_cut = self.holds_ammo()
            && ((self.adds() && self.value < 0.0) || (self.multiplies() && self.value < 1.0));
        fire_rate_cut || ammo_cut
    }
}

/// The barrel firing and magazine changes an entity graph stores, in payload order.
///
/// A record is found by its settings schema and read by field offset, the same way the Perk
/// Workbench shows it, so a record whose fields do not all decode is left out rather than guessed.
pub(crate) fn firing_records(
    manager: &PackageManager,
    graph: u32,
) -> AuthoringResult<Vec<FiringRecord>> {
    use sundial::package_authoring::runtime::load_weapon_runtime_graph_for_entity;
    let payload = manager
        .read_tag(TagHash(graph))
        .map_err(|error| invalid(format!("Attached entity 0x{graph:08X}: {error}")))?;
    let loaded = load_weapon_runtime_graph_for_entity(manager, 0, 0, graph, &payload)
        .map_err(|error| invalid(format!("Attached entity 0x{graph:08X}: {error}")))?;
    Ok(modifier_fields(&loaded)
        .values()
        .filter_map(|fields| firing_record(graph, fields))
        .collect())
}

/// The amount, operation, input and component fields of one stored modifier record.
type ModifierFields<'a> = [Option<&'a sundial::package_authoring::runtime::WeaponRuntimeField>; 4];

/// Every modifier record's fields, keyed by the owner and offset the record starts at.
fn modifier_fields(
    loaded: &sundial::package_authoring::runtime::WeaponRuntimeGraph,
) -> std::collections::BTreeMap<(u32, u32), ModifierFields<'_>> {
    use sundial::package_authoring::runtime::modifiers;
    let mut records = std::collections::BTreeMap::<(u32, u32), ModifierFields<'_>>::new();
    for resource in &loaded.resources {
        let fields = std::iter::once(&resource.instance)
            .chain(resource.definition.as_ref())
            .flat_map(|root| &root.fields)
            .filter(|field| field.locator.type_handle.get() == modifiers::SETTINGS_SCHEMA);
        for field in fields {
            let slot = match field.locator.value_offset {
                modifiers::AMOUNT_OFFSET => 0,
                modifiers::OPERATION_OFFSET => 1,
                modifiers::INPUT_OFFSET => 2,
                modifiers::COMPONENT_OFFSET => 3,
                _ => continue,
            };
            if let Some(start) = field.owner_offset.checked_sub(field.locator.value_offset) {
                records.entry((resource.owner_tag, start)).or_default()[slot] = Some(field);
            }
        }
    }
    records
}

/// One record as a barrel firing or magazine change, when it is one and every field decodes.
fn firing_record(graph: u32, fields: &ModifierFields<'_>) -> Option<FiringRecord> {
    use sundial::package_authoring::runtime::{WeaponRuntimeValue, modifiers};
    let number = |value: &WeaponRuntimeValue| match value {
        WeaponRuntimeValue::Signed(value) => Some(*value),
        WeaponRuntimeValue::Unsigned(value) => i64::try_from(*value).ok(),
        _ => None,
    };
    let [Some(amount), Some(operation), Some(input), Some(component)] = *fields else {
        return None;
    };
    let input = number(&input.value)?;
    let component = number(&component.value)?;
    let fires = component == modifiers::BARREL_COMPONENT
        && (modifiers::BARREL_ROUNDS_PER_BURST.contains(&input)
            || modifiers::BARREL_FIRE_TIMING.contains(&input));
    if !fires && component != modifiers::MAGAZINE_COMPONENT {
        return None;
    }
    let WeaponRuntimeValue::Float32Bits(bits) = amount.value else {
        return None;
    };
    let mut locator = amount.locator.clone();
    locator.graph_tag = Some(graph.into());
    Some(FiringRecord {
        amount: locator,
        component,
        input,
        operation: number(&operation.value)?,
        value: f32::from_bits(bits),
        always: false,
    })
}

/// One stock perk on a borrowed plug and the barrel firing and magazine changes of what it
/// attaches.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PerkFiring {
    pub(crate) perk: u16,
    pub(crate) records: Vec<FiringRecord>,
}

/// The firing and magazine changes a stock perk makes, or `None` when it makes none.
pub(crate) fn perk_firing(
    manager: &PackageManager,
    globals: &[u8],
    perk: u16,
) -> AuthoringResult<Option<PerkFiring>> {
    use sundial::package_authoring::sandbox_perk::{action, load_sandbox_perk_runtime_action};
    let runtime = match load_sandbox_perk_runtime_action(manager, globals, usize::from(perk)) {
        Ok(runtime) => runtime,
        // A declaration-only row has no runtime action, so it attaches nothing. Stock ships them
        // on real plugs, Hard Light's Volatile Light among them.
        Err(error)
            if error.contains("is not assigned") || error.contains("no standalone runtime") =>
        {
            return Ok(None);
        }
        Err(error) => return Err(invalid(format!("Perk {perk}: {error}"))),
    };
    let mut records = Vec::new();
    for graph in &runtime.graphs {
        // Most perks attach projectiles or effects rather than barrel changes. One whose graph
        // does not decode as a runtime graph holds no record this could edit, and reading it must
        // not fail a graft that builds today.
        let Ok(mut found) = firing_records(manager, graph.tag.0) else {
            continue;
        };
        if found.is_empty() {
            continue;
        }
        // An action that does not decode is treated as conditional, which never shifts a burst.
        let always = action::decode(&runtime.action_payload)
            .is_ok_and(|decoded| attached_from_the_start(&decoded, graph.tag.0));
        for record in &mut found {
            record.always = always;
        }
        records.extend(found);
    }
    Ok((!records.is_empty()).then_some(PerkFiring { perk, records }))
}

/// Whether an action attaches this graph for as long as the weapon is in use.
///
/// Three shapes do, the same three the program decompiler reads as having no ending: an
/// unconditional start with no removal, being equipped until unequipped, and being drawn until
/// stowed. An empty activation list is routed like an unconditional check. Harsh Truths, Twintails
/// and Saint's Fists hold their burst from equip to unequip, and Black Hole from the start.
fn attached_from_the_start(
    action: &sundial::package_authoring::sandbox_perk::action::DecodedAction,
    graph: u32,
) -> bool {
    use sundial::package_authoring::sandbox_perk::action::{DecodedCondition, Probability};
    /// The condition kind that always passes.
    const UNCONDITIONAL: u8 = 0;
    /// Condition kinds that start and end a hold: equipped and unequipped, drawn and stowed.
    const HOLDS: [(u8, u8); 2] = [(14, 15), (16, 17)];
    let plain = |condition: &DecodedCondition, kind: u8| {
        condition.kind == kind
            && condition.probability == Probability::Always
            && condition.children.is_empty()
            && condition.subgroups.is_empty()
    };
    action.groups.iter().any(|group| {
        let held = match (group.activation.as_slice(), group.removal.as_slice()) {
            ([], []) => true,
            ([start], []) => plain(start, UNCONDITIONAL),
            ([start], [end]) => HOLDS
                .iter()
                .any(|(on, off)| plain(start, *on) && plain(end, *off)),
            _ => false,
        };
        held && group
            .effects
            .iter()
            .any(|effect| effect.referenced_tag == Some(graph))
    })
}

/// How a borrowed plug's changes are fitted to the weapon it lands on.
#[derive(Clone, Copy, Debug)]
struct Landing {
    firing: BehaviorFiring,
    /// The host type's starting burst, and the source type's.
    host_base: f32,
    source_base: f32,
    /// Whether the weapon is a different type from the behavior's source weapon.
    other_type: bool,
}

/// The amounts a borrowed plug's firing and magazine changes take on this weapon, as edits of its
/// own perks.
///
/// Choosing the behavior's firing adds the difference between the two types' starting bursts once
/// per burst lane, through the first change the plug applies from the start, so a conditional
/// change on top keeps its own size. A lane with no such change scales its multipliers instead,
/// and no lane is ever left under one round. Choosing the weapon's firing returns every firing
/// change to the value that leaves its input alone. On a weapon of another type, cuts sized for
/// the source weapon's own numbers are returned to that value either way.
fn firing_edits(
    perks: &[PerkFiring],
    landing: Landing,
) -> Vec<(
    u16,
    sundial::package_authoring::runtime::WeaponRuntimeValueOverride,
)> {
    use sundial::package_authoring::runtime::{
        WeaponRuntimeValue, WeaponRuntimeValueOverride, modifiers::BARREL_ROUNDS_PER_BURST,
    };
    let records = perks
        .iter()
        .flat_map(|perk| perk.records.iter().map(move |record| (perk.perk, record)))
        .collect::<Vec<_>>();
    let mut values = records
        .iter()
        .map(|(_, record)| record.value)
        .collect::<Vec<_>>();
    for (value, (_, record)) in values.iter_mut().zip(&records) {
        let weapon_firing = landing.firing == BehaviorFiring::Weapon && !record.holds_ammo();
        if weapon_firing || (landing.other_type && record.cuts_for_its_own_type()) {
            *value = record.neutral();
        }
    }
    if landing.firing == BehaviorFiring::Behavior {
        for input in BARREL_ROUNDS_PER_BURST {
            let lane = (0..records.len())
                .filter(|index| records[*index].1.in_burst_lane(input))
                .collect::<Vec<_>>();
            shift_lane(
                &records,
                &mut values,
                &lane,
                landing.host_base,
                landing.source_base,
            );
            keep_one_round(&records, &mut values, &lane, landing.host_base);
        }
    }
    records
        .iter()
        .zip(values)
        .filter(|((_, record), value)| value.to_bits() != record.value.to_bits())
        .map(|((perk, record), value)| {
            (
                *perk,
                WeaponRuntimeValueOverride {
                    locator: record.amount.clone(),
                    value: WeaponRuntimeValue::Float32Bits(value.to_bits()),
                },
            )
        })
        .collect()
}

/// Moves one burst lane from the host type's starting burst to the source type's.
fn shift_lane(
    records: &[(u16, &FiringRecord)],
    values: &mut [f32],
    lane: &[usize],
    host_base: f32,
    source_base: f32,
) {
    if host_base.to_bits() == source_base.to_bits() {
        return;
    }
    if let Some(&index) = lane
        .iter()
        .find(|index| records[**index].1.adds() && records[**index].1.always)
    {
        values[index] += source_base - host_base;
        return;
    }
    for &index in lane.iter().filter(|index| records[**index].1.multiplies()) {
        values[index] *= source_base / host_base;
    }
}

/// Raises a burst lane that could otherwise fire no round at all.
///
/// A change applied from the start always counts, and a conditional one counts only when it takes
/// rounds away. The fewest the lane can fire is the host's own burst with all of those.
fn keep_one_round(
    records: &[(u16, &FiringRecord)],
    values: &mut [f32],
    lane: &[usize],
    host_base: f32,
) {
    let adds = || {
        lane.iter()
            .copied()
            .filter(|index| records[*index].1.adds())
    };
    let fewest = host_base
        + adds()
            .map(|index| {
                if records[index].1.always {
                    values[index]
                } else {
                    values[index].min(0.0)
                }
            })
            .sum::<f32>();
    if fewest >= 1.0 {
        return;
    }
    if let Some(index) = adds().min_by(|a, b| values[*a].total_cmp(&values[*b])) {
        values[index] += 1.0 - fewest;
    }
}

/// Makes each borrowed plug that changes the weapon's firing or magazine its own private plug,
/// carrying the firing pattern the recipe chose and fitted to the weapon's type.
///
/// The stock plug stays untouched, and so do its other perks: only the perks with a changed
/// amount are cloned, and only those amounts differ. A slot the author already holds a private
/// plug in is left to them. `plug_firing` reads a plug's perks from the packages, and the types
/// name the host weapon and each behavior's source weapon.
pub(crate) fn firing_variants(
    overrides: &crate::item::WeaponCloneOverrides,
    host_type: Option<&str>,
    source_type: &dyn Fn(&Behavior) -> Option<String>,
    plug_firing: &mut dyn FnMut(u32) -> AuthoringResult<Vec<PerkFiring>>,
) -> AuthoringResult<crate::item::WeaponCloneOverrides> {
    let mut expanded = overrides.clone();
    if overrides.skip_behavior_perks {
        return Ok(expanded);
    }
    let mut seen = BTreeSet::new();
    for entry in overrides
        .additional_behaviors
        .iter()
        .filter_map(|id| behavior(id))
    {
        // The behavior's firing needs both starting bursts. A type that cannot be read keeps the
        // stock amounts, which is what every graft did before this choice existed, and a weapon
        // only counts as another type when both types are known.
        let source = source_type(entry);
        let types = host_type
            .map(str::trim)
            .zip(source.as_deref().map(str::trim));
        let bases = match overrides.behavior_firing {
            BehaviorFiring::Behavior => types
                .map(|(host, source)| (base_rounds_per_burst(host), base_rounds_per_burst(source))),
            BehaviorFiring::Weapon => Some((1.0, 1.0)),
        };
        let Some((host_base, source_base)) = bases else {
            continue;
        };
        let landing = Landing {
            firing: overrides.behavior_firing,
            host_base,
            source_base,
            other_type: types.is_some_and(|(host, source)| host != source),
        };
        for plug in [entry.intrinsic_plug, entry.trait_plug]
            .into_iter()
            .flatten()
        {
            if !seen.insert(plug) {
                continue;
            }
            let perks = plug_firing(plug)?;
            let edits = firing_edits(&perks, landing);
            if !edits.is_empty() {
                place_private_plug(&mut expanded, plug, &edited_perks(&perks, &edits));
            }
        }
    }
    Ok(expanded)
}

/// The plug's perks that carry a firing edit, each as a private clone with its new amounts.
fn edited_perks(
    perks: &[PerkFiring],
    edits: &[(
        u16,
        sundial::package_authoring::runtime::WeaponRuntimeValueOverride,
    )],
) -> Vec<crate::item::WeaponSandboxPerkRuntimeOverride> {
    perks
        .iter()
        .filter_map(|perk| {
            let runtime_values = edits
                .iter()
                .filter(|(index, _)| *index == perk.perk)
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>();
            (!runtime_values.is_empty()).then(|| crate::item::WeaponSandboxPerkRuntimeOverride {
                program: None,
                projectiles: Vec::new(),
                source_perk_index: perk.perk,
                activation: None,
                runtime_values,
                action_float_values: Vec::new(),
            })
        })
        .collect()
}

/// Puts a private copy of the plug wherever the weapon offers it, except in a slot the author
/// already gave a private plug of their own.
fn place_private_plug(
    overrides: &mut crate::item::WeaponCloneOverrides,
    plug: u32,
    sandbox_perks: &[crate::item::WeaponSandboxPerkRuntimeOverride],
) {
    let slots = overrides
        .socket_columns
        .iter()
        .enumerate()
        .filter_map(|(lane, column)| Some((lane, column.as_ref()?)))
        .flat_map(|(lane, column)| {
            column
                .choices
                .iter()
                .enumerate()
                .filter(move |(_, choice)| **choice == plug)
                .map(move |(choice, _)| (lane, choice))
        })
        .filter_map(|(lane, choice)| Some((u16::try_from(lane).ok()?, u16::try_from(choice).ok()?)))
        .collect::<Vec<_>>();
    for (socket_index, choice_index) in slots {
        if overrides.socket_plug_variants.iter().any(|variant| {
            variant.socket_index == socket_index && variant.choice_index == choice_index
        }) {
            continue;
        }
        overrides
            .socket_plug_variants
            .push(crate::item::WeaponSocketPlugVariantOverride {
                offer_everywhere: false,
                replace_effects: false,
                investment_stats: Vec::new(),
                socket_index,
                choice_index,
                source_plug_hash: plug,
                name: None,
                classification_donor_hash: None,
                icon: None,
                description: None,
                additional_sandbox_perks: Vec::new(),
                sandbox_perks: sandbox_perks.to_vec(),
            });
    }
}
