//! D1's evade flags select the reachable Shadowkeep evade profile of the same eligibility, or a
//! private copy of it that plays the source evade's own human actions. The airborne flag selects
//! the air evade. The grounded flag selects Shadowkeep's one grounded profile, `BE74F016`, which
//! Arc Staff's controller names and whose bank selector `E117216E` no stock action applies. Its
//! eligibility runs one energy check, consumer `80803BEB`. Rise of Iron evades spend no class
//! energy, so a private grounded copy leaves that check out.
use super::*;
use crate::d1_roi::ability::animation;
use sundial::package_authoring::{ability_bank::AbilityTarget, ability_movement::evade};

/// The source movement controller's definition class, whose evade variants (`80803166`, `0x30`
/// bytes) hold `+0x10` a graph, `+0x18` the eligibility mask and `+0x1C..+0x28` four human
/// actions. Mask 2 is the airborne variant, mask 1 the grounded one.
const SOURCE_CONTROLLER: u32 = 0x8080_212D;
const SOURCE_VARIANT: u32 = 0x8080_3166;
const GROUNDED: u32 = 1;
const AIRBORNE: u32 = 2;

/// The eligibility mask a source evade grant's body enables: `[grounded, airborne, 0, 0]`.
fn eligibility(source: &Payload, row: usize) -> Result<u32> {
    match source.bytes::<4>(source.pointer(row + 32)?)? {
        [1, 0, 0, 0] => Ok(GROUNDED),
        [0, 1, 0, 0] => Ok(AIRBORNE),
        _ => anyhow::bail!("Source evade grant enables another evade combination"),
    }
}

/// The native evade profile a source evade grant qualifies for: the movement
/// controller that holds it, its identity, and the bank key that enables it.
struct Qualified {
    controller: u32,
    profile: u32,
    key: u32,
    /// Whether the native profile checks eligibility conditions of its own.
    gated: bool,
}

/// The human actions an item's private evades play, converted once for the item, and the
/// private names that replace them in the shared native human tables.
pub(in crate::d1_roi::native) struct Evades {
    symbol: String,
    converted: Option<(BTreeSet<u32>, BTreeMap<u32, u32>, animation::Human)>,
}

impl Evades {
    pub(in crate::d1_roi::native) fn new(symbol: String) -> Self {
        Self {
            symbol,
            converted: None,
        }
    }

    pub(in crate::d1_roi::native) fn into_human(self) -> Option<animation::Human> {
        self.converted.map(|(_, _, human)| human)
    }

    fn private(
        &mut self,
        source: &Source,
        native: &mut Reader,
        actions: [u32; 4],
    ) -> Result<[u32; 4]> {
        let wanted = actions.iter().copied().collect::<BTreeSet<_>>();
        if self.converted.is_none() {
            let plan = animation::read(source, animation::SOURCE_HUMAN_LOOKUP, &wanted)?;
            let prepared = animation::prepare(
                source,
                native,
                &plan,
                &animation::HUMAN,
                &self.symbol,
                &BTreeMap::new(),
                &[],
            )?;
            let names = prepared.actions.clone();
            self.converted = Some((
                wanted.clone(),
                names,
                animation::Human::new(native, prepared)?,
            ));
        }
        let (converted, names, _) = self.converted.as_ref().context("Converted evade actions")?;
        ensure!(
            *converted == wanted,
            "An item's evades play different source actions"
        );
        let mut private = [0; 4];
        for (lane, action) in actions.iter().enumerate() {
            private[lane] = *names.get(action).with_context(|| {
                format!("Source evade action {action:08X} has no private route")
            })?;
        }
        Ok(private)
    }
}

pub(super) fn lower(
    native: &mut Reader,
    source: &Payload,
    row: usize,
    slot: AbilityTarget,
    bank: &Payload,
    graph: u32,
) -> Result<u32> {
    Ok(qualify(native, source, row, slot, bank, graph)?.key)
}

/// A private air evade: a copy of the qualified native profile that plays the source airborne
/// variant's four human actions, converted into private actions, under the key of its own
/// selector row. The copy keeps the native profile's energy, readiness, effect pattern and the
/// preparation that moves the body.
#[allow(clippy::too_many_arguments)]
pub(super) fn private(
    source: &Source,
    native: &mut Reader,
    owners: &[u32],
    data: &Payload,
    row: usize,
    slot: AbilityTarget,
    (bank, payload): (u32, &Payload),
    graph: u32,
    evades: &mut Evades,
) -> Result<(AbilityRow, Value)> {
    let qualified = qualify(native, data, row, slot, payload, graph)?;
    let (owner, variant, actions) = source_variant(source, owners, eligibility(data, row)?)?;
    let private = evades.private(source, native, actions)?;
    let property = AbilityRow::new(
        bank,
        slot,
        255,
        AbilityRowModifier::Evade {
            template_key: qualified.key,
            controller: qualified.controller,
            template_profile: qualified.profile,
            actions: private,
            ungated: qualified.gated,
        },
    );
    property
        .check_bank(&payload.0)
        .map_err(anyhow::Error::msg)?;
    // The controller must take the profile before the build is asked to add it.
    let controller = native.tag(qualified.controller, Some(0x80809C36))?;
    evade::with_profile(
        &controller.0,
        evade::Profile {
            identity: property.key,
            template: qualified.profile,
            actions: private,
            ungated: qualified.gated,
        },
    )
    .map_err(anyhow::Error::msg)?;
    Ok((
        property,
        json!({"source_controller":owner,"source_variant_offset":variant,"source_actions":actions,
            "private_actions":private,"native_controller":qualified.controller,
            "template_profile":qualified.profile,"template_key":qualified.key,
            "native_conditions_left_out":qualified.gated}),
    ))
}

/// The source movement controller among `owners`, its evade variant of eligibility `mask` and that
/// variant's four human actions.
fn source_variant(source: &Source, owners: &[u32], mask: u32) -> Result<(u32, usize, [u32; 4])> {
    let mut found = Vec::new();
    for &owner in owners {
        let data = source.read(owner)?;
        let definition = data.pointer(24)?;
        if data.u32(definition - 4)? != SOURCE_CONTROLLER {
            continue;
        }
        // The variants are the one array of their class the definition region holds.
        let mut arrays = Vec::new();
        for at in (definition..data.0.len().saturating_sub(16)).step_by(8) {
            let count = data.u64(at)?;
            if !(1..=8).contains(&count) {
                continue;
            }
            let Ok(header) = data.pointer(at + 8) else {
                continue;
            };
            if header + 16 <= data.0.len()
                && data.u64(header)? == count
                && data.u32(header + 8)? == SOURCE_VARIANT
            {
                arrays.push(at);
            }
        }
        let [descriptor] = arrays.as_slice() else {
            anyhow::bail!("Source movement controller evade variants are missing or ambiguous");
        };
        for at in data.array(*descriptor, 0x30, Some(SOURCE_VARIANT))? {
            if data.u32(at + 0x18)? != mask {
                continue;
            }
            let mut actions = [0; 4];
            for (lane, action) in actions.iter_mut().enumerate() {
                *action = data.u32(at + 0x1C + lane * 4)?;
            }
            found.push((owner, at, actions));
        }
    }
    let [variant] = found.as_slice() else {
        anyhow::bail!("Source evade variant {mask} is missing or ambiguous");
    };
    Ok(*variant)
}

fn qualify(
    native: &mut Reader,
    source: &Payload,
    row: usize,
    slot: AbilityTarget,
    bank: &Payload,
    graph: u32,
) -> Result<Qualified> {
    let mask = eligibility(source, row)?;
    ensure!(
        slot == AbilityTarget::Movement
            && source.u32(source.pointer(16)? - 4)? == 0x808027BE
            && source.u32(source.pointer(24)? - 4)? == 0x808029B5
            && source.u32(row + 12)? == 10
            && source.u32(row + 16)? == 255,
        "Only an ungated source evade has a qualified native profile"
    );
    ensure!(
        bank.u32(bank.pointer(16)? - 4)? == 0x80804112
            && bank.u32(bank.pointer(24)? - 4)? == 0x80804113,
        "Native movement bank pair differs"
    );

    // A retained source key can name a removed profile. Resolve the current controller's
    // profile first, then the bank property that enables it. No shared bytes are changed.
    let entity = native.tag(graph, Some(0x80809C0F))?;
    let mut profiles = BTreeSet::new();
    let mut controllers = BTreeSet::new();
    let mut gated = false;
    for component in entity.array(16, 12, None)? {
        let owner = entity.u32(component)?;
        let data = native.tag(owner, Some(0x80809C36))?;
        let settings = data.pointer(24)?;
        if data.u32(settings - 4)? != 0x808044E0 {
            continue;
        }
        ensure!(
            data.u32(data.pointer(16)? - 4)? == 0x808043AE,
            "Movement controller pair differs"
        );
        let config = data.pointer(settings + 0x800)?;
        ensure!(
            data.u32(config - 4)? == 0x8080410F,
            "Movement profile configuration differs"
        );
        let candidates = data.array(config + 16, 128, Some(0x80804106))?;
        ensure!(
            candidates.len() <= 32,
            "Movement profile mask exceeds its native capacity"
        );
        for at in candidates {
            // Original request admission uses mask 0x02 for airborne, 0x01 for grounded.
            // The qualified air evade answers input 476D0367 with no extra gate. The one
            // grounded profile answers 6B3F0812 and holds one energy check.
            let (energy, gates) = match mask {
                AIRBORNE
                    if data.u32(at + 0x58)? == AIRBORNE
                        && data.u32(at + 0x70)? == 0x476D0367
                        && data.u64(at + 0x30)? == 0
                        && data.u64(at + 0x38)? == 0 =>
                {
                    ([0.5f32, 5.0, 100.0, 0.5, 1.0], 0)
                }
                GROUNDED
                    if data.u32(at + 0x58)? == GROUNDED && data.u32(at + 0x70)? == 0x6B3F0812 =>
                {
                    ([0.0, 0.0, 0.0, 0.0, 1.0], 1)
                }
                _ => continue,
            };
            ensure!(
                [0x14, 0x18, 0x1C, 0x40, 0x44]
                    .iter()
                    .zip(energy)
                    .all(|(&offset, value)| data.u32(at + offset).ok() == Some(value.to_bits()))
                    && data.u64(at + 0x30)? == gates,
                "Evade energy profile differs from the qualified native contract"
            );
            gated |= gates != 0;
            native.tag(data.u32(at + 0x50)?, Some(0x80809C0F))?;
            ensure!(
                profiles.insert(data.u32(at + 16)?),
                "Duplicate air-evade profile identity"
            );
            controllers.insert(owner);
        }
    }
    ensure!(
        profiles.len() == 1,
        "Native evade profile {mask} is missing or ambiguous"
    );
    let profile = *profiles.first().context("Native evade profile")?;
    let mut selected = Vec::new();
    let rows = bank.array(bank.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    for &at in &rows {
        let modifier = bank.pointer(at + 48)?;
        if bank.u32(modifier + 4)? != 0x80804121 || bank.u32(modifier + 16)? != profile {
            continue;
        }
        ensure!(
            bank.u32(at + 24)? == 255
                && bank.u32(at + 32)? == 255
                && bank.u32(at + 28)? == 11
                && bank.u32(at + 36)? == 0
                && bank.u64(at + 40)? == 0,
            "Air-evade property has additional gates or flags"
        );
        selected.push(bank.u32(at + 16)?);
    }
    ensure!(
        selected.len() == 1,
        "Native air-evade property is missing or ambiguous"
    );
    let key = selected[0];
    ensure!(
        rows.iter()
            .filter(|&&at| bank.u32(at + 16).ok() == Some(key))
            .count()
            == 1,
        "Native air-evade key also grants an unqualified companion"
    );
    let [controller] = controllers.into_iter().collect::<Vec<_>>()[..] else {
        anyhow::bail!("Native air-evade controller is ambiguous");
    };
    Ok(Qualified {
        controller,
        profile,
        key,
        gated,
    })
}
