//! Bounded source action lowering. An unsupported node rejects the entire choice.
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use sundial::package_authoring::sandbox_perk::program::{
    AbilityCounterpart, Action, AmmunitionStore, AmmunitionTarget, DamageMode, ImportedAsset,
    NativeNode, Program, Trigger,
};
mod animation;
mod armor;
mod audio;
mod condition;
mod counterpart;
mod damage;
pub(crate) mod direct;
mod energy;
mod feedback;
mod filter;
mod grant;
mod hit;
pub(crate) mod hud;
mod incoming;
/// Object-label filters, shared with query components.
pub(in crate::d1_roi) mod label;
mod orb;
pub(crate) mod pattern;
mod policy;
mod scalar;
mod stat;
pub(crate) mod subclass;
mod super_gate;
mod timer;
mod transfer;
mod weighted;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Host {
    Weapon,
    Armor,
}

impl Host {
    fn validate_variant(self, step: &Value, record: &Payload) -> Result<()> {
        if self == Self::Armor {
            ensure!(
                step["appearance_choices"].as_u64() == Some(0),
                "Appearance choices use the separate ornament route"
            );
            ensure!(
                record.bytes::<40>(48)? == [0; 40],
                "Source armor progression or variant metadata requires translation"
            );
        }
        Ok(())
    }

    fn validate_action(self, source: &Source, tag: u32) -> Result<()> {
        self.validate_action_inner(source, tag, &mut Vec::new())
    }

    fn validate_action_inner(
        self,
        source: &Source,
        tag: u32,
        ancestors: &mut Vec<u32>,
    ) -> Result<()> {
        if self != Self::Armor {
            return Ok(());
        }
        ensure!(
            ancestors.len() < 8 && !ancestors.contains(&tag),
            "Source armor controllers form a cycle or exceed the depth limit"
        );
        ancestors.push(tag);
        let result = self.validate_action_body(source, tag, ancestors);
        ancestors.pop();
        result
    }

    fn validate_action_body(
        self,
        source: &Source,
        tag: u32,
        ancestors: &mut Vec<u32>,
    ) -> Result<()> {
        if self == Self::Armor {
            let data = source.read(tag)?;
            for row in data.array(0x38, 8, None)? {
                let at = data.pointer(row)?;
                match data.u8(at)? {
                    0 => pattern::armor_attachment_inner(source, &data, at, ancestors)?,
                    // The qualified direct health response resolves the owning player.
                    // Profile admission still checks the complete zero-damage envelope.
                    3 => ensure!(
                        data.u8(at + 2)? == 1,
                        "Armor direct effects require the player target"
                    ),
                    // Explicit player properties use the same target resolver on armor. A
                    // property on the item itself, target 0 without an ability mask, is admitted
                    // on the talent's own action only, where `item_property_witness` then needs a
                    // stock action writing the same key on its item. A selected ability needs a
                    // separate contract.
                    9 => ensure!(
                        data.u32(at + 4)? == 0
                            && (data.u8(at + 2)? == 1
                                || (data.u8(at + 2)? == 0 && ancestors.len() == 1)),
                        "Source armor property {:08X} requires an unmasked player target, not target {}",
                        data.u32(at + 8)?,
                        data.u8(at + 2)?
                    ),
                    // Ammo modifier callbacks resolve the action owner's player component,
                    // independently of an owning weapon or a triggering-weapon event.
                    // Named event publication and the movement record resolve the owner's
                    // player the same way.
                    4 | 7 | 10 | 11 | 12 | 19 | 27 | 28 | 31 | 37 => {}
                    // Source 71DFB0 resolves the owner's player and its ability component through
                    // 6C89C0, as the property grants of an armor talent do.
                    6 => {}
                    // Proportional refills and reserve transfers resolve the player's
                    // inventory. Armor cannot supply an owning weapon amount.
                    14 | 15 => ensure!(
                        data.u32(at + 8)? == 0,
                        "Source armor refill requires an explicit inventory slot"
                    ),
                    // Literal damage rows consume the event buffers directly. Without
                    // the owner gate they need no weapon supplied by the armor host.
                    34 => ensure!(
                        data.u8(at + 0x94)? == 0,
                        "Source armor damage modifier requires the owner gate disabled"
                    ),
                    kind => anyhow::bail!("Source effect {kind} needs an armor host contract"),
                }
            }
        }
        Ok(())
    }
}

/// Equipped stat contributions of one source choice on Shadowkeep stat definitions, as recipe
/// rows and report rows. A source row is a plain delta only when both scale factors are one
/// and its selector bytes are unset. On armor, a budgeted stat's rows follow [`stat`]: a
/// selected row is the item's own random roll, which the native base stats stand for, and a
/// fixed zero-valued row takes the `slot`'s budget share in Shadowkeep points.
fn adjustments(
    host: Host,
    slot: Option<u8>,
    record: &Payload,
    rows: &Value,
    definitions: &BTreeMap<u32, u16>,
    budgets: &BTreeMap<u32, stat::Budget>,
) -> Result<(Vec<Value>, Vec<Value>)> {
    let rows = rows.as_array().map(Vec::as_slice).unwrap_or_default();
    ensure!(
        rows.len() == usize::try_from(record.u64(16)?)?,
        "Source stat adjustments disagree with the variant record"
    );
    let mut stats = Vec::new();
    let mut report = Vec::new();
    let mut indices = BTreeSet::new();
    for row in rows {
        let hash = u32::try_from(row["hash"].as_u64().context("Source stat hash")?)?;
        let bytes = Payload(hex::decode(
            row["record"]
                .as_str()
                .context("Source stat adjustment bytes")?,
        )?);
        // A row with no amount and both scale factors zero contributes nothing whatever the
        // item's stat budget, as Illumination's `8E8B4E10` row shows.
        if bytes.0.len() == 16
            && bytes.bytes::<12>(0)? == [0; 12]
            && bytes.bytes::<3>(13)? == [0xFF, 0, 0]
        {
            report.push(json!({"source_hash":hash,"source_index":row["index"],"inert":true}));
            continue;
        }
        if host == Host::Armor
            && let Some(budget) = budgets.get(&hash)
            && bytes.0.len() == 16
            && bytes.bytes::<2>(14)? == [0, 0]
        {
            let (low, high, selector) = (bytes.f32(4)?, bytes.f32(8)?, bytes.u8(13)?);
            if selector != 0xFF {
                // The roll interpolates `[low, high]` by one of the instance's random bytes. The
                // native item's own base stats are its roll, so a second one would double them.
                report.push(json!({"source_hash":hash,"source_index":row["index"],
                    "item_roll":true,"selector":selector,"bounds":[low, high]}));
                continue;
            }
            if bytes.u32(0)? == 0 {
                let value = stat::armor_points(
                    budget,
                    slot.context("Source armor stat budget needs the item's slot")?,
                    low,
                )?;
                let index = *definitions.get(&hash).with_context(|| {
                    format!("Source stat {hash:08X} has no Shadowkeep stat definition")
                })?;
                ensure!(
                    u8::try_from(index).is_ok() && indices.insert(index),
                    "Source stat {hash:08X} exceeds plug capacity or is adjusted twice"
                );
                stats.push(json!({"definition_index":index,"value":value}));
                report.push(json!({"source_hash":hash,"source_index":row["index"],
                    "definition_index":index,"fraction":low,"slot":slot,
                    "unit":"source budget share, 60 source points to 10 native a tier",
                    "value":value}));
                continue;
            }
        }
        ensure!(
            bytes.0.len() == 16
                && bytes.u32(4)? == 0x3F80_0000
                && bytes.u32(8)? == 0x3F80_0000
                && bytes.bytes::<3>(13)? == [0xFF, 0, 0],
            "Source stat {hash:08X} adjustment is scaled or selected and requires curve translation"
        );
        let source_value = i32::from_le_bytes(bytes.bytes::<4>(0)?);
        // Armor/Resilience keeps its investment identity across these builds. The
        // plain Armor policy treats each source unit as one native tier, expressed
        // as ten points. It does not recreate D1's health curve or armor roll.
        let scale = if host == Host::Armor {
            ensure!(
                hash == 0x1769_266F,
                "Source armor stat {hash:08X} requires a native unit and scaling contract"
            );
            10
        } else {
            1
        };
        let value = source_value
            .checked_mul(scale)
            .context("Source stat contribution overflow")?;
        // Rise of Iron and Shadowkeep share these stat hashes, including Inventory Size, which
        // the modern stat group mapping treats as repurposed, so the lookup is direct.
        let index = *definitions
            .get(&hash)
            .with_context(|| format!("Source stat {hash:08X} has no Shadowkeep stat definition"))?;
        ensure!(
            u8::try_from(index).is_ok(),
            "Source stat {hash:08X} exceeds private plug stat capacity"
        );
        ensure!(
            indices.insert(index),
            "Source stat {hash:08X} is adjusted twice"
        );
        stats.push(json!({"definition_index":index,"value":value}));
        report.push(json!({"source_hash":hash,"source_index":row["index"],
            "definition_index":index,"source_value":source_value,"unit_scale":scale,"value":value}));
    }
    Ok((stats, report))
}

struct Lowered {
    programs: Vec<Value>,
    raw: Vec<Program>,
    tags: Vec<u32>,
    auxiliary: usize,
    stats: Vec<Value>,
    adjustments: Vec<Value>,
    /// Sandbox perks of the stock plugs that stand in for source perks, and those plugs.
    stock: Vec<u16>,
    stock_plugs: Vec<String>,
    grants: Vec<Value>,
    audio_cues: BTreeSet<u32>,
}

/// Source talent perks, by identity, that run a stock Shadowkeep weapon perk instead of their
/// own action. Each source action attaches a talent pattern that sets weapon component fields,
/// which have no established Shadowkeep field mapping, while Shadowkeep ships the same perk:
/// Quickdraw `4E9855BC` (`quick_draw.pattern.tft`) runs Quickdraw `2A1CBFD4` and Snapshot
/// `5D32FF95` (`snapshot.pattern.tft`) runs Snapshot Sights `39169B67`. Triple Tap `EFD82FBF`
/// (action `80A7F003`: a counter of three hits, each held two seconds, adding one round, beside a
/// HUD status whose host carries a state program) runs Shadowkeep's Triple Tap `CAB3DB58`, which
/// returns one round to the magazine for rapidly landed precision hits. Rise of Iron and
/// Shadowkeep share no sandbox perk identities, so the pairs are listed rather than looked up.
const STOCK_EQUIVALENTS: [(u32, u32); 3] = [
    (0x4E98_55BC, 0x2A1C_BFD4),
    (0x5D32_FF95, 0x3916_9B67),
    (0xEFD8_2FBF, 0xCAB3_DB58),
];

/// Keys stock actions write with a Named Property node on their own item: target 0, no ability
/// mask, replacing, removal policy one. Air Control `12DAFD65` and Fast Enter `A2DCA8DC` are
/// written this way by `80BBC81F` and `80BBC986`, with the source's value one, and read by stock
/// owners `80EF1439`, `80EF62F7` and `80C0C498`, as the Rise of Iron owner `80A746E2` reads them.
fn item_properties(native: &mut Reader) -> Result<BTreeSet<u32>> {
    use sundial::package_authoring::sandbox_perk::action::{FactValue, NAMED_PROPERTY_LABELS};
    let labels = &NAMED_PROPERTY_LABELS;
    let mut keys = BTreeSet::new();
    for tag in native.classes(0x808040B5) {
        let stock = native.tag(tag, Some(0x808040B5))?;
        let Ok(decoded) = sundial::package_authoring::sandbox_perk::action::decode(&stock.0) else {
            continue;
        };
        for effect in decoded.effects().filter(|effect| effect.kind == 10) {
            let fact = |label: &str| {
                effect
                    .facts
                    .iter()
                    .find(|fact| fact.label == label)
                    .map(|fact| &fact.value)
            };
            if let Some(FactValue::Key(key)) = fact(labels.key)
                && fact(labels.target) == Some(&FactValue::Selector(0))
                && matches!(fact(labels.ability_mask), None | Some(FactValue::Mask(0)))
                && fact(labels.removal) == Some(&FactValue::Selector(1))
            {
                keys.insert(*key);
            }
        }
    }
    Ok(keys)
}

/// An armor action's item properties must each have a stock writer on an item, since a key no
/// Shadowkeep item carries reaches no reader.
fn item_property_witness(
    source: &Source,
    native: &mut Reader,
    witnessed: &mut Option<BTreeSet<u32>>,
    tag: u32,
) -> Result<()> {
    let data = source.read(tag)?;
    for row in data.array(0x38, 8, None)? {
        let at = data.pointer(row)?;
        if (data.u32(at - 4)?, data.u8(at)?, data.u8(at + 2)?) != (0x80802755, 9, 0) {
            continue;
        }
        if witnessed.is_none() {
            *witnessed = Some(item_properties(native)?);
        }
        let key = data.u32(at + 8)?;
        ensure!(
            witnessed.as_ref().is_some_and(|keys| keys.contains(&key)),
            "Source armor item property {key:08X} has no Shadowkeep item writer"
        );
    }
    Ok(())
}

fn conditions(
    source: &Source,
    data: &Payload,
    descriptor: usize,
    mut native: Option<&mut Reader>,
) -> Result<Vec<NativeNode>> {
    // Activation alternatives pass independently, so one that names a game event Shadowkeep lacks
    // can be left out while another remains. The talent then starts on fewer events than in the
    // source, never on more. Capture King lists three zone capture events, of which only
    // `34FD8483` has a Shadowkeep event. Removal and rearm alternatives keep every condition,
    // since leaving one out would lengthen the effect.
    let mut absent = None;
    let nodes = data
        .array(descriptor, 8, None)?
        .into_iter()
        .map(|at| {
            let node = data.pointer(at)?;
            let read = condition::read(source, data, node, native.as_deref_mut());
            if descriptor == 0x20
                && (data.u32(node - 4)?, data.u8(node + 5)?) == (0x808030D4, 12)
                && let Err(error) = &read
                && error.to_string() == condition::ABSENT_GAME_EVENT
            {
                absent = Some(read.err());
                return Ok(None);
            }
            if let Some(translated) = read? {
                return Ok(Some(translated));
            }
            let class = data.u32(node - 4)?;
            let kind = data.u8(node + 5)?;
            let size = match (class, kind) {
                (0x80803176, 0) => 8,
                (0x80802DEB, 1) => {
                    ensure!(data.f32(node + 8)? >= 0.0, "Negative source timer");
                    12
                }
                _ => anyhow::bail!("Condition {class:08X} ({kind}) requires runtime translation"),
            };
            let bytes = data
                .0
                .get(node..node + size)
                .context("Truncated source condition")?
                .to_vec();
            Ok(Some(NativeNode { kind, bytes }))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if let Some(Some(error)) = absent
        && nodes.is_empty()
    {
        return Err(error);
    }
    Ok(nodes)
}

fn effect(
    source: &Source,
    data: &Payload,
    at: usize,
    native: Option<&mut Reader>,
) -> Result<Action> {
    let class = data.u32(at - 4)?;
    let kind = data.u8(at)?;
    match (class, kind) {
        (0x80802A46, 4) => orb::read(source, data, at),
        // Applying an ability property key on the owner's ability slot while the action runs. The
        // record is the native Ability Property layout (kind 7): slot `+2`, key `+4`, operation
        // `+8`. The key is the source bank's, so the action stands in until the talent step lowers
        // it through the ability grant catalog ([`grant::Catalog::lower_key`]).
        (0x80802820, 6) => {
            ensure!(
                data.u8(at + 1)? == 1
                    && data.u8(at + 2)? <= 4
                    && data.u8(at + 3)? == 0
                    && data.bytes::<4>(at + 8)? == [0; 4],
                "Source ability property effect has another lifecycle, slot or operation"
            );
            Ok(Action::AbilityProperty {
                target: sundial::package_authoring::ability_bank::AbilityTarget::from_byte(
                    data.u8(at + 2)?,
                ),
                key: data.u32(at + 4)?,
                option: sundial::package_authoring::sandbox_perk::program::PropertyOperation::Apply,
            })
        }
        (0x808026E4, 28) => incoming::read(source, data, at, native),
        (0x808027D8, 10) | (0x80802188, 11) => scalar::ammo_modifiers(data, at, kind),
        (0x8080230E, 27) => timer::read(source, data, at, native),
        (0x8080231F, 7) => energy::read(data, at),
        (0x80802482, 12) => weighted::read(data, at),
        (0x808027F1, 13) => scalar::rounds(data, at),
        (0x8080321E, 15) => transfer::read(data, at),
        (0x8080314E, 19) => {
            ensure!(
                data.u8(at + 1)? == 0 && data.u8(at + 2)? <= 1,
                "Unsupported source radar-detail lifecycle or selector"
            );
            // Both callbacks pass +2 on activation and its Boolean inverse on removal.
            // The serialized record is three bytes, without the following alignment.
            Ok(Action::Native {
                node: NativeNode {
                    kind: 20,
                    bytes: vec![20, 0, data.u8(at + 2)?],
                },
            })
        }
        (0x808031C0, 34) => damage::read(source, data, at, native),
        (0x80802AC6, 5) => {
            ensure!(
                data.u8(at + 1)? == 1 && data.u8(at + 2)? <= 3,
                "Unsupported source damage selector"
            );
            Ok(Action::SetDamageType {
                mode: DamageMode::from_byte(data.u8(at + 2)?),
                keep_after_removal: false,
            })
        }
        (0x8080221F, 14) => scalar::fraction(data, at),
        (0x80802840, 24) | (0x80802C04, 25) => {
            let size = if kind == 24 { 24 } else { 16 };
            ensure!(
                data.u8(at + 1)? == 1,
                "Unsupported source full-auto lifecycle"
            );
            let mut bytes = data
                .0
                .get(at..at + size)
                .context("Truncated source full-auto effect")?
                .to_vec();
            if kind == 24 {
                ensure!(
                    bytes[4..].iter().all(|v| *v == 0),
                    "Unknown source ability flags"
                );
            } else {
                ensure!(bytes[2..4] == [0, 0], "Unknown source timing flags");
                for offset in [4, 8, 12] {
                    let _ = data.f32(at + offset)?;
                }
            }
            bytes[0] = kind + 4;
            Ok(Action::Native {
                node: NativeNode {
                    kind: kind + 4,
                    bytes,
                },
            })
        }
        (0x80802755, 9) => {
            // The serialized node is 48 bytes. Alignment before a following node
            // can add zero padding, which is absent for a terminal node. Its constant
            // occupies four lanes at +10 rather than a value program. A removal
            // policy of one restores zero when the program ends, as stock Cluster Bombs does
            // for its rocket channel.
            ensure!(
                data.u8(at + 1)? == 1 && data.u32(at + 12)? == 0,
                "Unknown source named property header"
            );
            let key = data.u32(at + 8)?;
            ensure!(
                ![0, u32::MAX, 0x811C_9DC5].contains(&key),
                "Source named property has no key"
            );
            let value_bits = data.u32(at + 16)?;
            ensure!(
                (1..4).all(|lane| data.u32(at + 16 + lane * 4).ok() == Some(value_bits))
                    && f32::from_bits(value_bits).is_finite()
                    && data.bytes::<16>(at + 32)? == [0; 16],
                "Source named property is not one constant"
            );
            Ok(Action::Property {
                key,
                target: data.u8(at + 2)?,
                operation_byte: 0,
                removal: 1,
                value_bits,
                restore_bits: 0,
                ability_mask: data.u32(at + 4)?,
                input: 0,
                flag: data.u8(at + 3)?,
            })
        }
        // Source activation `7217F0` and native kind 43 `108CA60` are the same routine: for a
        // nonempty key and an authoritative owning player, publish key `+04` through the
        // event-message machinery, whose publishers require a registered key. What receives
        // the message is unresolved in both builds.
        (0x808030D7, 37) => {
            ensure!(
                data.bytes::<3>(at + 1)? == [0; 3],
                "Source named event publication has retained state"
            );
            let key = data.u32(at + 4)?;
            ensure!(
                ![0, u32::MAX, 0x811C_9DC5].contains(&key),
                "Source named event publication has no key"
            );
            let mut bytes = vec![43, 0, 0, 0];
            bytes.extend_from_slice(&key.to_le_bytes());
            Ok(Action::Native {
                node: NativeNode { kind: 43, bytes },
            })
        }
        // Source kind 31 and native kind 36 register a typed movement record on the owning
        // player's movement component, entering its timed air hold. Activation `0071FD40` and
        // `01081B00`, and removal `0071FE40` and `010834D0`, match instruction for instruction.
        // Native record `8080692C` inserts two admission flags and two zero bytes after the key,
        // then holds the source's two eight-float groups byte for byte. Both flags set requests
        // the hold in either airborne case, as the source always does and stock `80BB7412`
        // stores. The source initializes the hold even when its mode request is refused, and
        // native only once the mode is active. That state is inert outside the mode, and a
        // repeated registration renews the hold in both builds.
        (0x808023AA, 31) => {
            ensure!(
                data.bytes::<7>(at + 1)? == [0; 7] && data.u64(at + 8)? != 0,
                "Source movement record effect has retained state or no record"
            );
            let record = data.pointer(at + 8)?;
            ensure!(
                record >= 4 && data.u32(record - 4)? == 0x80802AA6,
                "Source movement record is not the air-hold record"
            );
            let mut bytes = vec![0; 104];
            bytes[0] = 36;
            bytes[8..16].copy_from_slice(&24i64.to_le_bytes());
            bytes[28..32].copy_from_slice(&0x8080_692Cu32.to_le_bytes());
            bytes[32..36].copy_from_slice(&data.bytes::<4>(record)?);
            bytes[36..38].copy_from_slice(&[1, 1]);
            bytes[40..104].copy_from_slice(&data.bytes::<64>(record + 4)?);
            Ok(Action::Native {
                node: NativeNode { kind: 36, bytes },
            })
        }
        // Source kind 29 (`71B950`) and native kind 34 (`108AD80`) run the same activation: for
        // the owning player, resolve target selector `+2` and hand the target to its pickup
        // interface. Neither keeps state or has a removal callback. The three-byte record carries
        // with the kind renumbered. Shoot to Loot uses selector 3, the struck object.
        (0x80802AA9, 29) => {
            ensure!(
                data.u8(at + 1)? == 0 && data.u8(at + 2)? <= 3,
                "Source pickup effect has retained state or another target"
            );
            Ok(Action::Native {
                node: NativeNode {
                    kind: 34,
                    bytes: vec![34, 0, data.u8(at + 2)?],
                },
            })
        }
        // Source kind 26 holds native kind 30's retained and operation bytes.
        // Padding before the next object is not part of this three-byte record.
        // Tracking `[1A 01 01]` equals stock Tracking Module `[1E 01 01]`.
        (0x80802B88, 26) => {
            ensure!(data.u8(at + 1)? == 1, "Unknown source weapon count fields");
            Ok(Action::WeaponReferenceCount {
                selector: data.u8(at + 2)?,
            })
        }
        _ => anyhow::bail!("Effect {class:08X} ({kind}) requires runtime translation"),
    }
}

/// `folder` receives imported attachment groups, such as victim explosion graphs. Programs
/// lowered without one reject those attachments.
fn program(
    source: &Source,
    tag: u32,
    name: &str,
    native: Option<&mut Reader>,
    folder: Option<&Path>,
) -> Result<(Program, usize)> {
    program_inner(source, tag, name, native, folder, &mut Vec::new())
}

fn program_inner(
    source: &Source,
    tag: u32,
    name: &str,
    native: Option<&mut Reader>,
    folder: Option<&Path>,
    ancestors: &mut Vec<u32>,
) -> Result<(Program, usize)> {
    ensure!(
        ancestors.len() < 8 && !ancestors.contains(&tag),
        "Source nested perk controllers form a cycle or exceed the depth limit"
    );
    ancestors.push(tag);
    let result = action_program(source, tag, name, native, folder, ancestors);
    ancestors.pop();
    result
}

fn action_program(
    source: &Source,
    tag: u32,
    name: &str,
    mut native: Option<&mut Reader>,
    folder: Option<&Path>,
    ancestors: &mut Vec<u32>,
) -> Result<(Program, usize)> {
    ensure!(
        source.reference(tag)? == 0x80802711,
        "Source perk is not an action controller"
    );
    let data = source.read(tag)?;
    ensure!(
        data.u64(0x68)? == 0,
        "Additional source action groups require translation"
    );
    let policy = policy::read(source, &data, native.as_deref_mut())?;
    let Effects {
        mut actions,
        patches,
        damage_profiles,
        imported_assets,
        mut omitted_feedback,
    } = translate_effects(source, &data, native.as_deref_mut(), folder, ancestors)?;
    // An execution policy runs its own handlers, so a choice may hold only the policy, as stock
    // action `80BBCA51` does for its stacking policy.
    ensure!(
        !actions.is_empty() || policy.is_some(),
        "Source choice has no executable effects"
    );
    // Activation on equipping a super of one damage type, ending when it leaves, becomes a
    // subclass gate the build expands over the installed subclasses' supers.
    let gate = super_gate::read(source, &data)?;
    let (mut activation, mut removal) = if gate.is_some() {
        (Vec::new().into_iter(), Vec::new().into_iter())
    } else {
        (
            conditions(source, &data, 0x20, native.as_deref_mut())?.into_iter(),
            conditions(source, &data, 0x48, native.as_deref_mut())?.into_iter(),
        )
    };
    let mut rearm = conditions(source, &data, 0x58, native.as_deref_mut())?.into_iter();
    let native_trigger = activation.next();
    let alternative_triggers = activation.collect::<Vec<_>>();
    // Target 3 selects the event's other combatant at context-summary +14 in both builds. When
    // the event has none, Rise of Iron keeps the action's owner, while Shadowkeep keeps the
    // absent handle and writes nothing. Weapon draw and holster events have no other combatant,
    // so a source property write there reaches its owner, which target 0 selects natively.
    if native_trigger.is_some()
        && native_trigger
            .iter()
            .chain(&alternative_triggers)
            .all(|condition| matches!(condition.kind, 16 | 17))
    {
        for action in &mut actions {
            if let Action::Property { target, .. } = action
                && *target == 3
            {
                *target = 0;
            }
        }
    }
    let trigger = if native_trigger.is_some() || gate.is_some() {
        Trigger::Native
    } else {
        // Source 6CD410 and native EBCEA0 admit an empty primary activation list
        // on event zero when its compiled mask contains that event. Use the
        // native Always condition without broadening a source routing mask.
        ensure!(
            data.u64(0x80)? == 1,
            "Source empty activation list requires initialization-only routing"
        );
        Trigger::Always
    };
    let mut program = Program {
        name: name.to_owned(),
        trigger,
        duration_ms: 0,
        cooldown_ms: 0,
        native_trigger,
        alternative_triggers,
        native_removal: removal.next(),
        alternative_removals: removal.collect(),
        subclass_damage_type: gate.as_ref().map(|(damage, _)| *damage),
        native_rearm: rearm.next(),
        alternative_rearms: rearm.collect(),
        actions,
        native_asset_patches: patches,
        damage_profiles,
        imported_assets,
        policy,
        ..Program::default()
    };
    program.ability_counterparts = counterpart::named(&program);
    // A damage-type gate also accepts imported copies of the supers its source list names.
    for &pattern in gate.iter().flat_map(|(_, supers)| supers) {
        program.ability_counterparts.push(AbilityCounterpart {
            source: crate::d1_roi::ABILITY_SOURCE.to_owned(),
            identity: counterpart::identity(source, pattern)?,
            native: None,
        });
    }
    let (auxiliary, omitted) = feedback::read(source, native, &data, &program)?;
    program.auxiliary = auxiliary;
    omitted_feedback += omitted;
    program.validate_structure().map_err(anyhow::Error::msg)?;
    Ok((program, omitted_feedback))
}

/// Intrinsic plugs (`67FBA961`) set a native weapon's frame, a fixed perk Rise of Iron never had.
/// An exotic's trait is the one fixed perk its grid singles out, so only that takes the socket.
const INTRINSIC_CATEGORY: u64 = 0x67FB_A961;
/// The source item rarity of an exotic. A legendary's first column can also hold one fixed perk,
/// such as Burning Eye's Snapshot or Abyss Defiant's Hive Disruptor, which stays a perk column.
const EXOTIC_RARITY: u64 = 5;
/// The source effect class and kind that set a weapon's damage type.
const DAMAGE_EFFECT: (u32, u8) = (0x8080_2AC6, 5);
/// The display-only intrinsic a weapon without a signature perk shows, so the inspect screen
/// still has a frame row. It carries no stats or effects.
const LEGACY_FRAME: &str = "Legacy Frame";
const LEGACY_FRAME_DESCRIPTION: &str = "A relic of an older age, pulled through the timeline.";
/// The Destiny logo, a stock 150-pixel Shadowkeep texture the build fits to a perk glyph. Its icon
/// layer is `80B47ED7`, but a perk icon names the texture.
const LEGACY_FRAME_ICON: &str = "0x80B47ED5";
/// Weapon perk plugs, as opposed to shaders (14), masterworks and trackers (13) and mods (37).
const PERK_ITEM_KIND: u64 = 15;

/// Whether a base socket is an exotic catalyst socket, which Rise of Iron weapons never had. Only
/// catalyst sockets mix masterwork-kind (13) and mod-kind (37) plugs. Most default to an empty
/// mod-kind plug of category `2EA40B3E` and offer the catalysts, while some default to the empty
/// masterwork plug `v400.empty.exotic.masterwork` (`72334481`). Masterwork sockets hold masterwork
/// plugs only and weapon mod sockets mod plugs only.
fn catalyst_socket(socket: &Value) -> bool {
    let kinds = socket["choices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|choice| choice["item_kind"].as_u64())
        .collect::<BTreeSet<_>>();
    kinds.contains(&13) && kinds.contains(&37)
}

fn damage_name(mode: DamageMode) -> Option<&'static str> {
    match mode {
        DamageMode::Kinetic => Some("kinetic"),
        DamageMode::Solar => Some("solar"),
        DamageMode::Arc => Some("arc"),
        DamageMode::Void => Some("void"),
        DamageMode::Unknown(_) => None,
    }
}

/// The damage type a translated source choice sets, from the first of its programs that sets one.
fn damage_type(programs: &[Program]) -> Option<&'static str> {
    programs
        .iter()
        .flat_map(|program| &program.actions)
        .find_map(|action| match action {
            Action::SetDamageType { mode, .. } => damage_name(*mode),
            _ => None,
        })
}

/// Whether setting the damage type is all a translated choice does, so the recipe's damage type
/// can stand in for it.
fn only_sets_damage(lowered: &Lowered) -> bool {
    matches!(lowered.raw.as_slice(), [program] if program.actions.len() == 1)
        && lowered.stats.is_empty()
        && lowered.stock.is_empty()
        && lowered.grants.is_empty()
}

/// Every source step whose talent actions carry a damage effect, as its node, step and damage
/// type. The effects are read from the source actions, so a step counts even when its other
/// contributions keep it from translating. A selector outside the four types reads as `None`.
/// Only each action's own effect list is read, not nested controllers or additional groups.
fn source_damage(
    source: &Source,
    nodes: &[Value],
) -> Result<Vec<(u64, u64, Option<&'static str>)>> {
    let mut found = Vec::new();
    for node in nodes {
        let index = node["index"].as_u64().context("Source talent node index")?;
        for step in node["steps"].as_array().context("Source talent steps")? {
            let ordinal = step["index"].as_u64().context("Source talent step index")?;
            let perks = step["variants"]
                .as_array()
                .context("Source talent variants")?
                .iter()
                .flat_map(|variant| variant["perks"].as_array().into_iter().flatten());
            for perk in perks {
                let Some(tag) = perk["action"]
                    .as_u64()
                    .and_then(|tag| u32::try_from(tag).ok())
                else {
                    continue;
                };
                if source.reference(tag)? != 0x8080_2711 {
                    continue;
                }
                let data = source.read(tag)?;
                for at in data.array(0x38, 8, None)? {
                    let at = data.pointer(at)?;
                    if (data.u32(at - 4)?, data.u8(at)?) == DAMAGE_EFFECT {
                        let kind = if data.u8(at + 1)? == 1 {
                            damage_name(DamageMode::from_byte(data.u8(at + 2)?))
                        } else {
                            None
                        };
                        found.push((index, ordinal, kind));
                    }
                }
            }
        }
    }
    Ok(found)
}

/// The damage type the source effects give a weapon: the one type all of them select, or Kinetic
/// when no talent carries one, since Rise of Iron applies an element only through such an effect.
/// A mix of types or an unreadable selector gives none.
fn effect_damage(found: &[(u64, u64, Option<&'static str>)]) -> Option<&'static str> {
    let kinds = found
        .iter()
        .map(|&(_, _, kind)| kind)
        .collect::<BTreeSet<_>>();
    match kinds.into_iter().collect::<Vec<_>>().as_slice() {
        [] => Some("kinetic"),
        [kind] => *kind,
        _ => None,
    }
}

/// A source weapon whose effects make it Kinetic needs a donor that can carry Kinetic damage.
/// Damage alternatives mix types, so a weapon with selectable damage keeps any donor. Unsupported
/// choices count, since the weapon's damage does not depend on their translation.
pub(super) fn requires_kinetic(source: &Source, weapon: &Value) -> Result<bool> {
    let nodes = weapon["talents"]["nodes"]
        .as_array()
        .context("Source talent nodes")?;
    Ok(effect_damage(&source_damage(source, nodes)?) == Some("kinetic"))
}

/// A native perk socket: its index, socket type and perk plugs, a default first when present.
struct PerkSocket<'a> {
    index: usize,
    socket_type: &'a Value,
    plugs: Vec<u32>,
}

/// The base's intrinsic socket and its other perk sockets, in socket order.
fn perk_sockets(
    sockets: &[Value],
    host: Host,
) -> Result<(Option<PerkSocket<'_>>, Vec<PerkSocket<'_>>)> {
    let mut intrinsic = None;
    let mut perks = Vec::new();
    for (index, socket) in sockets.iter().enumerate() {
        if socket["socket_type"] == u16::MAX {
            continue;
        }
        let Some(choices) = socket["choices"].as_array() else {
            continue;
        };
        // Randomized trait sockets can omit a default entirely. Their homogeneous
        // perk pool still supplies a socket layout for private source choices.
        // Mixed or cosmetic pools cannot establish that role without a default.
        let default = choices.iter().find(|choice| choice["default"] == true);
        let fallback = || {
            choices
                .iter()
                .all(|choice| choice["item_kind"].as_u64() == Some(PERK_ITEM_KIND))
                .then(|| {
                    choices
                        .iter()
                        .find(|choice| choice["hash"] == socket["curated_fallback"])
                        .or_else(|| choices.first())
                })
                .flatten()
        };
        let Some(default) = default.or_else(fallback) else {
            continue;
        };
        if default["item_kind"].as_u64() != Some(PERK_ITEM_KIND) {
            continue;
        }
        let mut plugs = Vec::new();
        for choice in std::iter::once(default).chain(choices.iter().filter(|choice| {
            choice["default"] != true && choice["item_kind"].as_u64() == Some(PERK_ITEM_KIND)
        })) {
            let hash = u32::try_from(choice["hash"].as_u64().context("Native plug identity")?)?;
            if !plugs.contains(&hash) {
                plugs.push(hash);
            }
        }
        let socket = PerkSocket {
            index,
            socket_type: &socket["socket_type"],
            plugs,
        };
        if host == Host::Armor || default["category"].as_u64() != Some(INTRINSIC_CATEGORY) {
            perks.push(socket);
        } else if intrinsic.is_none() {
            intrinsic = Some(socket);
        }
    }
    Ok((intrinsic, perks))
}

/// A translated source choice and where it sits in the source grid.
struct Choice {
    column: i64,
    row: i64,
    /// The source node, one of the column's exclusive rows. Rise of Iron rolled one of its steps.
    node: u64,
    name: String,
    description: Value,
    icon: Option<String>,
    lowered: Lowered,
    report: Value,
}

/// The finished perks that carry translated programs, and the plug that lent the first. A donor
/// choice supplies only a finished-perk layout. Empty sight and magazine plugs are valid
/// choices, so search the remaining default and alternate choices before refusing it. A private
/// plug clones each finished perk once, so a talent step with several source perks needs a
/// different one for each, and the search continues until it has `needed` of them.
fn templates(
    assembly: &mut Assembly<'_>,
    sockets: &[Value],
    needed: usize,
) -> Result<(u32, Vec<u16>)> {
    let mut visited = BTreeSet::new();
    let mut lender = None;
    let mut templates = Vec::new();
    for default in [true, false] {
        for choice in sockets
            .iter()
            .flat_map(|socket| socket["choices"].as_array().into_iter().flatten())
        {
            if templates.len() >= needed {
                break;
            }
            if choice["item_kind"].as_u64() != Some(PERK_ITEM_KIND)
                || (choice["default"] == true) != default
            {
                continue;
            }
            let hash = u32::try_from(choice["hash"].as_u64().context("Native plug identity")?)?;
            if !visited.insert(hash) {
                continue;
            }
            let tag = crate::tiger::shadowkeep::item_tag(&mut assembly.native, hash)?;
            let item = assembly.native.tag(tag, Some(0x80807BEA))?;
            let perks = sundial::package_authoring::native_weapon::base_sandbox_perks(&item.0)
                .map_err(anyhow::Error::msg)?;
            for perk in perks {
                if templates.len() < needed && !templates.contains(&perk) {
                    lender.get_or_insert(hash);
                    templates.push(perk);
                }
            }
        }
    }
    let lender = lender.context("Native gameplay plug choices have no finished perk template")?;
    Ok((lender, templates))
}

/// The most choices a perk column offers freely, as few as a Shadowkeep random-roll column's
/// curated perks.
const REUSABLE_CHOICES: usize = 4;
/// The most choices a column rolls from: the rows one 64-bit ownership mask covers, also the
/// largest stock randomized set.
const ROLL_POOL: usize = 64;

/// Source choices in row and node order, retaining step order within each node.
fn members(choices: &[Choice], column: i64) -> Vec<usize> {
    let mut members = choices
        .iter()
        .enumerate()
        .filter_map(|(index, choice)| (choice.column == column).then_some(index))
        .collect::<Vec<_>>();
    members.sort_by_key(|&index| (choices[index].row, choices[index].node));
    members
}

/// A column's members with each translated plug once, the first in row and step order. Steps of
/// different nodes can translate to the same plug: each Rise of Iron armor stat node offers
/// Discipline, Intellect and Strength, and the tier-preserving point conversion gives rolls of one
/// stat on two nodes the same native points. A socket offers a plug once, so an item that rolled
/// the same stat on two nodes rolls it once.
fn distinct(
    choices: &[Choice],
    members: Vec<usize>,
    unsupported: &mut Vec<Value>,
) -> Result<Vec<usize>> {
    let mut seen = BTreeSet::new();
    let mut kept = Vec::new();
    for index in members {
        let choice = &choices[index];
        let definition = serde_json::to_string(&json!([
            choice.name,
            choice.description,
            choice.icon,
            choice.lowered.stats,
            choice.lowered.stock,
            choice.lowered.programs
        ]))?;
        if seen.insert(definition) {
            kept.push(index);
        } else {
            unsupported.push(json!({"node":choice.node,"step":choice.report["step"],
                "name":choice.name,"reason":"Another choice of the column translates to the same plug"}));
        }
    }
    Ok(kept)
}

/// A column's choices in socket order, how many lead it as freely swappable, and how many steps an
/// item of the column offered. A source column's nodes are exclusive rows, and Rise of Iron rolled
/// one step of each. The first supported step of each of the first four nodes stays swappable, and
/// a column with fewer nodes fills its four swappable choices from the next steps in row and step
/// order. Every supported step joins the roll pool, the swappable ones first and then the rest in
/// row and step order.
fn roll(
    choices: &[Choice],
    members: Vec<usize>,
    unsupported: &mut Vec<Value>,
) -> (Vec<usize>, usize, usize) {
    let mut nodes = BTreeSet::new();
    let (mut ordered, mut rolled): (Vec<_>, Vec<_>) = members
        .into_iter()
        .partition(|&index| nodes.insert(choices[index].node) && nodes.len() <= REUSABLE_CHOICES);
    let offered = ordered.len();
    let filled = REUSABLE_CHOICES.saturating_sub(offered).min(rolled.len());
    ordered.extend(rolled.drain(..filled));
    let reusable = ordered.len();
    for index in rolled {
        if ordered.len() < ROLL_POOL {
            ordered.push(index);
        } else {
            let choice = &choices[index];
            unsupported.push(json!({"node":choice.node,"step":choice.report["step"],
                "name":choice.name,"reason":"The column's roll pool is full"}));
        }
    }
    (ordered, reusable, offered)
}

/// Whether a source node carries a talent: perks, stat rows or ability grants. Infusion, Ascend,
/// damage upgrade and ornament nodes carry none.
fn carries_talent(node: &Value) -> bool {
    node["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|step| step["variants"].as_array().into_iter().flatten())
        .any(|variant| {
            ["perks", "stat_adjustments", "ability_grants"]
                .into_iter()
                .any(|field| {
                    variant[field]
                        .as_array()
                        .is_some_and(|rows| !rows.is_empty())
                })
        })
}

/// The source column of an exotic's signature perk, decided by the grid's structure and the
/// source's other weapons rather than by which talents translate. A candidate column holds one
/// talent node with one step whose name no non-exotic weapon's grid shows, so generic perks an
/// exotic shares with legendaries, such as Gjallarhorn's Tracking or The Jade Rabbit's Zen Moment,
/// are never its signature. A candidate named after the weapon is its signature, such as Sleeper
/// Simulant's or Ice Breaker's. Otherwise an always-active first-column candidate that only this
/// weapon shows is, such as Outbreak Prime's The Corruption Spreads. Otherwise the signature is the
/// last candidate, the grid's final exotic unlock, such as Tlaloc's Overflow over Grenadier, a
/// first-column perk legendaries also roll. When that last talent runs every action
/// of an earlier single talent, it upgrades that talent, as Hawkmoon's Holding Aces adds bullets to
/// Luck in the Chamber, and the earlier talent is the signature. Nodes that set the damage type
/// are never the signature.
fn trait_column(
    nodes: &[Value],
    damage_nodes: &BTreeSet<u64>,
    exotic_talents: &BTreeMap<String, BTreeSet<String>>,
    weapon: &str,
) -> Result<Option<i64>> {
    // Names compare without case, a leading article or punctuation, so The Fate of All Fools
    // names the weapon Fate of All Fools.
    let plain = |name: &str| {
        let name = name.trim().to_lowercase();
        let name = name.strip_prefix("the ").unwrap_or(&name).to_owned();
        name.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let weapon = plain(weapon);
    let mut talents = BTreeMap::<i64, Vec<&Value>>::new();
    for node in nodes {
        let index = node["index"].as_u64().context("Source talent node index")?;
        let column = node["column"].as_i64().context("Source talent column")?;
        if carries_talent(node) && !damage_nodes.contains(&index) && column >= 0 {
            talents.entry(column).or_default().push(node);
        }
    }
    let perks = |node: &Value| -> Vec<Value> {
        node["steps"][0]["variants"][0]["perks"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    // Every column of one single-step talent, with its actions and whether only exotics show it.
    let mut singles = Vec::new();
    for (&column, members) in &talents {
        let [node] = members.as_slice() else {
            continue;
        };
        let rows = perks(node);
        if node["steps"].as_array().map_or(0, Vec::len) != 1 || rows.is_empty() {
            continue;
        }
        let name = node["steps"][0]["name"].as_str().unwrap_or_default();
        // The weapons that show the talent, when only exotics do.
        let shown = exotic_talents.get(name.trim());
        let actions = rows
            .iter()
            .filter_map(|perk| perk["action"].as_u64())
            .collect::<BTreeSet<_>>();
        singles.push((column, actions, shown, plain(name) == weapon));
    }
    if let Some((column, _, _, _)) = singles
        .iter()
        .rev()
        .find(|(_, _, shown, named)| shown.is_some() && *named)
    {
        return Ok(Some(*column));
    }
    if singles
        .iter()
        .any(|(column, _, shown, _)| *column == 0 && shown.is_some_and(|w| w.len() == 1))
    {
        return Ok(Some(0));
    }
    let Some(at) = singles.iter().rposition(|(_, _, shown, _)| shown.is_some()) else {
        return Ok(None);
    };
    let (last, upgrade, _, _) = &singles[at];
    // The upgraded talent can be one legendaries also roll, as Luck in the Chamber is in Rise of
    // Iron, so any earlier single talent qualifies.
    let base = singles[..at]
        .iter()
        .rev()
        .find(|(_, actions, _, _)| !actions.is_empty() && actions.is_subset(upgrade));
    Ok(Some(base.map_or(*last, |(column, _, _, _)| *column)))
}

/// Translate the source talent grid onto the base's perk sockets. On an exotic, the trait that
/// [`trait_column`] finds takes the intrinsic socket. Otherwise the intrinsic holds Legacy Frame,
/// since a native frame would describe a perk the source never had.
/// Each other source column becomes one socket, taking the base's perk sockets after its
/// intrinsic in order, so the sights column lands where a native weapon's barrel or sight does.
/// A column offers one step of each of its first four nodes, in row order, and rolls from every
/// supported step, as a Shadowkeep random-roll column keeps a few curated perks beside its
/// randomized pool. Each choice clones a plug from
/// its native socket's pool as its layout template, distinct while the pool lasts. A choice whose
/// only effect sets a fixed damage type sets the recipe's damage type. Alternative damage steps
/// share a separate socket so their private effects follow the equipped choice, and all of them
/// stay selectable rather than rolled. A fixed damage choice that does more keeps the rest on that
/// socket, as its one choice. Without a translated damage choice, the source effects set the
/// recipe's damage type, Kinetic when no talent sets an element.
/// Unused perk sockets are disabled so the base's own perks do not stand in for the source's, and
/// so is a base exotic's catalyst socket.
/// Damage alternatives keep their source talent art, the element marks Rise of Iron drew.
#[allow(clippy::cognitive_complexity)]
/// What lowering one talent step needs besides the step: the source and native packages, the
/// folder for imported attachment groups, the native stat definitions, the stock plugs that
/// stand in for source perks, the finished perks that carry programs and the ability catalog
/// grants read on first use.
struct StepContext<'a> {
    source: &'a Source,
    native: &'a mut Reader,
    folder: &'a Path,
    host: Host,
    definitions: &'a BTreeMap<u32, u16>,
    equivalents: &'a BTreeMap<u32, (u32, Vec<u16>)>,
    templates: &'a [u16],
    grant_catalog: &'a mut Option<grant::Catalog>,
    /// The symbol an item's private evade actions convert under, when its graph carries them.
    evades: Option<&'a str>,
    /// The armor item's source slot, which a budgeted stat row's share depends on.
    slot: Option<u8>,
    budgets: &'a BTreeMap<u32, stat::Budget>,
    /// Keys stock actions write on their own item, read on first use.
    item_properties: &'a mut Option<BTreeSet<u32>>,
}

/// The step's ability grant catalog, read once per item.
fn grant_catalog(context: &mut StepContext<'_>) -> Result<()> {
    if context.grant_catalog.is_none() {
        let catalog = grant::Catalog::read(context.source, context.native)?;
        *context.grant_catalog = Some(match context.evades {
            Some(symbol) => catalog.with_evades(symbol.to_owned()),
            None => catalog,
        });
    }
    Ok(())
}

/// Lower the source ability property keys a perk action's own effects apply (source effect 6)
/// through the grant catalog, in place, keeping the action's conditions. Returns their grant
/// reports.
fn ability_properties(
    context: &mut StepContext<'_>,
    program: &mut Program,
    name: &str,
) -> Result<Vec<Value>> {
    let applies = |program: &Program| {
        program
            .actions
            .iter()
            .any(|action| matches!(action, Action::AbilityProperty { .. }))
    };
    ensure!(
        !program.owned_programs().skip(1).any(applies),
        "A nested controller applies a source ability property"
    );
    if !applies(program) {
        return Ok(Vec::new());
    }
    grant_catalog(context)?;
    let catalog = context
        .grant_catalog
        .as_ref()
        .context("Source ability catalog")?;
    let mut reports = Vec::new();
    let mut actions = Vec::new();
    for action in std::mem::take(&mut program.actions) {
        let Action::AbilityProperty { target, key, .. } = action else {
            actions.push(action);
            continue;
        };
        let (lowered, report) =
            catalog.lower_key(context.source, context.native, key, target, name)?;
        actions.extend(lowered.actions);
        program.ability_rows.extend(lowered.ability_rows);
        program.ability_tunings.extend(lowered.ability_tunings);
        program.ability_inputs.extend(lowered.ability_inputs);
        reports.extend(report);
    }
    program.actions = actions;
    program.validate_structure().map_err(anyhow::Error::msg)?;
    Ok(reports)
}

fn lower_step(context: &mut StepContext<'_>, step: &Value, name: &str) -> Result<Lowered> {
    let host = context.host;
    let variants = step["variants"]
        .as_array()
        .context("Source talent variants")?;
    ensure!(
        variants.len() == 1,
        "Talent step has multiple runtime variants"
    );
    let record = Payload(hex::decode(
        variants[0]["record"]
            .as_str()
            .context("Source variant bytes")?,
    )?);
    host.validate_variant(step, &record)?;
    let (stats, adjusted) = adjustments(
        host,
        context.slot,
        &record,
        &variants[0]["stat_adjustments"],
        context.definitions,
        context.budgets,
    )?;
    let perks = variants[0]["perks"]
        .as_array()
        .context("Source talent perks")?;
    let grants = variants[0]["ability_grants"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    ensure!(
        grants.len() == usize::try_from(record.u64(0)?)?,
        "Source ability grants disagree with the variant record"
    );
    // A choice with only stat rows becomes a stats-only private plug.
    ensure!(
        !perks.is_empty() || !stats.is_empty() || !grants.is_empty(),
        "Talent step has no standalone runtime perk"
    );
    let mut programs = Vec::new();
    let mut raw = Vec::new();
    let mut tags = Vec::new();
    let mut auxiliary = 0;
    let mut stock = Vec::new();
    let mut stock_plugs = Vec::new();
    let mut property_reports = Vec::new();
    for perk in perks {
        let Some(tag) = perk["action"].as_u64() else {
            // A grant-backed talent can have a display perk without its own action.
            // The complete grant array must lower successfully below.
            ensure!(
                !grants.is_empty() && perk["runtime_key"].as_u64() == Some(0x811C9DC5),
                "Source perk has no resolved standalone action"
            );
            continue;
        };
        let tag = u32::try_from(tag)?;
        tags.push(tag);
        host.validate_action(context.source, tag)?;
        if host == Host::Armor {
            item_property_witness(context.source, context.native, context.item_properties, tag)?;
        }
        let identity = u32::try_from(perk["hash"].as_u64().context("Source perk identity")?)?;
        if let Some((plug, indices)) = context.equivalents.get(&identity) {
            stock.extend(indices.iter().copied());
            stock_plugs.push(format!("{plug:08X}"));
            continue;
        }
        let (mut program, omitted) = program(
            context.source,
            tag,
            name,
            Some(&mut *context.native),
            Some(context.folder),
        )?;
        property_reports.extend(ability_properties(context, &mut program, name)?);
        auxiliary += omitted;
        let carrier = context
            .templates
            .get(programs.len())
            .context("The base has no further finished perk to carry this source perk")?;
        programs.push(json!({"source_perk_index":carrier,"program":program,"runtime_values":[],"action_float_values":[]}));
        raw.push(program);
    }
    let mut granted = property_reports;
    if !grants.is_empty() {
        grant_catalog(context)?;
        let (program, report) = context
            .grant_catalog
            .as_ref()
            .context("Source ability catalog")?
            .lower(
                context.source,
                context.native,
                &variants[0],
                name,
                &BTreeMap::new(),
                false,
            )?;
        granted.extend(report);
        if !program.actions.is_empty() {
            let carrier = context
                .templates
                .get(programs.len())
                .context("The base has no further finished perk for source ability properties")?;
            programs.push(json!({"source_perk_index":carrier,"program":program,"runtime_values":[],"action_float_values":[]}));
            raw.push(program);
        }
    }
    ensure!(
        !programs.is_empty() || !stats.is_empty() || !stock.is_empty(),
        "Talent step has no executable action, property or stat contribution"
    );
    let audio_cues = audio::program_cues(&raw)?;
    Ok(Lowered {
        programs,
        raw,
        tags,
        auxiliary,
        stats,
        adjustments: adjusted,
        stock,
        stock_plugs,
        grants: granted,
        audio_cues,
    })
}

pub(super) fn translate(
    assembly: &mut Assembly<'_>,
    source: &Value,
    donor: u32,
    host: Host,
) -> Result<(Value, Value, Value, Option<&'static str>)> {
    let native = crate::tiger::shadowkeep::sockets(&mut assembly.native, donor)?;
    let definitions = crate::tiger::shadowkeep::stat_definitions(&mut assembly.native)?;
    let sockets = native["sockets"].as_array().context("Native sockets")?;
    let nodes = source["talents"]["nodes"]
        .as_array()
        .context("Source talent nodes")?;
    // A talent step carries each of its source perks on a finished perk of its own.
    let needed = nodes
        .iter()
        .flat_map(|node| node["steps"].as_array().into_iter().flatten())
        .flat_map(|step| step["variants"].as_array().into_iter().flatten())
        .map(|variant| {
            variant["perks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|perk| perk["action"].as_u64().is_some())
                .count()
                + usize::from(
                    variant["ability_grants"]
                        .as_array()
                        .is_some_and(|g| !g.is_empty()),
                )
        })
        .max()
        .unwrap_or_default()
        .max(1);
    // An exotic armor signature's socket type, borrowed by its intrinsic socket below.
    let signature_socket_type;
    let (mut intrinsic, mut perk_sockets) = perk_sockets(sockets, host)?;
    // Armor has no damage type, so only a weapon's damage effects are read.
    let found = if host == Host::Weapon {
        source_damage(assembly.source, nodes)?
    } else {
        Vec::new()
    };
    let armor_lender = if host == Host::Armor && perk_sockets.is_empty() {
        Some(armor::layout(&mut assembly.native, donor)?)
    } else {
        None
    };
    let template_sockets = armor_lender
        .as_ref()
        .map(|lender| {
            lender["sockets"]
                .as_array()
                .context("Armor perk layout sockets")
        })
        .transpose()?
        .unwrap_or(sockets);
    let (_, extra_layouts) = self::perk_sockets(template_sockets, host)?;
    let (template_hash, templates) = templates(assembly, template_sockets, needed)?;
    let equivalents = stock_equivalents(&mut assembly.native, host)?;
    let mut columns = clear_talent_sockets(sockets, intrinsic.as_ref(), &perk_sockets, host);
    let LoweredTalents {
        mut choices,
        mut unsupported,
        grant_catalog,
    } = lower_talents(
        assembly,
        source,
        nodes,
        host,
        (&definitions, &equivalents, &templates),
    )?;
    audio::admit(assembly, &mut choices, &mut unsupported)?;
    assign_damage_choices(&mut choices)?;
    let (damage, damage_origin) = primary_damage(&choices, &found, host);
    let mut source_columns = choices
        .iter()
        .map(|choice| choice.column)
        .filter(|&column| column != i64::MIN)
        .collect::<Vec<_>>();
    source_columns.sort_unstable();
    source_columns.dedup();
    let mut variants = Vec::new();
    let mut translated = Vec::new();
    for choice in choices.iter().filter(|choice| choice.column == i64::MIN) {
        let mut report = choice.report.clone();
        report["damage_type"] = json!(damage);
        translated.push(report);
    }
    let damage_nodes = found
        .iter()
        .map(|&(node, _, _)| node)
        .collect::<BTreeSet<_>>();
    let trait_at = signature_column(
        assembly.source,
        source,
        nodes,
        &damage_nodes,
        host,
        intrinsic.is_some(),
    )?;
    // A trait that does not translate leaves Legacy Frame in the intrinsic socket rather than
    // promoting another column's perk.
    let mut signature = trait_at.filter(|&column| {
        choices
            .iter()
            .filter(|choice| choice.column == column)
            .count()
            == 1
    });
    // A translated armor signature takes the intrinsic socket a Year 1 exotic carries its perk in,
    // so the inspect screen shows it first, labeled Intrinsic, and the tooltip lists it. It takes
    // the donor's first socket when that socket is disabled or holds only intrinsic-category
    // plugs before any perk socket, as a year-two armor base's random stat intrinsic does. That
    // stat plug moves to an added socket of the type a Year 1 exotic keeps its own stat intrinsic
    // in, outside the perk row, so the item keeps its stats. Otherwise the signature takes the
    // first perk socket and the other columns follow it. A bucket with no such exotic, such as
    // class items, keeps the signature as an ordinary perk column.
    let mut stat_intrinsic = None;
    let layout = if host == Host::Armor && signature.is_some() {
        armor::intrinsic(&mut assembly.native, donor)?
    } else {
        None
    };
    if host == Host::Armor && layout.is_none() {
        signature = None;
    }
    if let Some(layout) = layout {
        signature_socket_type = json!(layout.socket_type);
        let (index, displaced) = signature_slot(
            sockets,
            &mut perk_sockets,
            &mut columns,
            layout.stat_socket_type,
        );
        stat_intrinsic = displaced;
        intrinsic = Some(PerkSocket {
            index,
            socket_type: &signature_socket_type,
            plugs: vec![layout.plug],
        });
    }
    emit_talent_columns(
        &assembly.projectile_names,
        (&choices, &source_columns, &mut unsupported),
        (signature, intrinsic.as_ref()),
        (&perk_sockets, &extra_layouts),
        (&mut columns, &mut variants, &mut translated),
    )?;
    let (legacy, stat_intrinsic) = finish_intrinsic(
        &mut assembly.native,
        signature,
        intrinsic.as_ref(),
        stat_intrinsic,
        (&mut columns, &mut variants),
    )?;
    let intrinsic_socket = intrinsic
        .as_ref()
        .filter(|_| signature.is_some() || legacy)
        .map(|socket| socket.index);
    let intrinsic = if signature.is_some() {
        "signature"
    } else if legacy {
        "legacy_frame"
    } else {
        "none"
    };
    let adaptation = if host == Host::Armor {
        "Supported source armor talents become private equipped socket choices. Source activation, removal, rearm and explicit player or triggering-weapon targets are retained through the bounded translators. Supported charge and script-parameter grants use private properties restricted to matching ability banks. Every action and grant must translate for a choice to be admitted. An exotic's signature talent, found among talents only exotic armor shows, takes a Year 1 exotic intrinsic socket at the front of the perk row when it translates, and a donor stat intrinsic it displaces moves to a socket outside the perk row. Donor perk columns and direct perks are cleared. Native base stats remain the baseline. Source stat tiers, progression and unqualified effects remain unsupported. Ornaments keep their separate appearance sockets."
    } else {
        "Source talent columns become private socket choices, with additional perk sockets when needed. A column offers the first supported step of each of its first four nodes, in row order, and every supported step joins a private randomized plug set the socket rolls from, since Rise of Iron rolled one step of each node. Offered talents are available immediately, with plain stat adjustments as equipped stat contributions. A fixed damage talent sets the weapon's damage type. One that also carries other effects keeps them on a separate damage socket. Alternative damage steps share that socket, which offers every one of them, with the first source step as the initial damage type. A weapon whose talents set no element is Kinetic. On an exotic, the last single talent whose name only exotic weapons show takes the intrinsic socket, or the earlier talent it upgrades, and its other exotic talents stay perk columns. Supported attached status companions remain part of their talent. Source progression and unsupported talents are not migrated. Unsupported talents are reported individually and do not block weapon import."
    };
    // Authored columns make the build curate the sockets the base keeps, such as a year-two
    // armor base's random stat intrinsic, so each needs a default.
    let mut columns = json!(columns);
    let mut native_defaults = Vec::new();
    crate::tiger::shadowkeep::fill_native_defaults(&native, &mut columns, &mut native_defaults)?;
    assembly.human = grant_catalog.as_ref().and_then(grant::Catalog::take_human);
    Ok((
        columns,
        json!(variants),
        json!({"translated":translated,"unsupported":unsupported,"damage_type":damage,
        "damage_origin":damage_origin,
        "native_defaults":native_defaults,
        "status":if translated.is_empty() { "unsupported" } else if unsupported.is_empty() { "translated" } else { "partial" },
        "intrinsic":intrinsic,
        "intrinsic_socket":intrinsic_socket,
        "stat_intrinsic":stat_intrinsic,
        "trait_column":trait_at,
        "empty_source_grid":nodes.is_empty(),
        "finished_perk_templates":{"plug":template_hash,"indices":templates},
        "socket_layout_lender":armor_lender.as_ref().map(|lender| &lender["weapon"]),
        "host":if host == Host::Armor { "armor" } else { "weapon" },
        "adaptation":adaptation,
        "gameplay_verified":false}),
        damage,
    ))
}

/// Lower every distinct talent step of every source weapon and armor piece once, with the host
/// its item gives, and count which translate, by distinct step and across items. Steps repeat
/// across items, so a step is keyed by host, name and variant content. Carriers are not limited
/// by a base here, so a step that needs more finished perks than one base holds still counts.
/// A lowered talent's ability counterparts, with the conditions a stand-in imported copy of each
/// source ability adds to clones of its programs, as the build adds them for real copies.
fn ability_gates(programs: &[Program]) -> Option<Value> {
    const STAND_IN: u32 = 0x81FF_FFF0;
    let counterparts = programs
        .iter()
        .flat_map(Program::owned_programs)
        .flat_map(|program| &program.ability_counterparts)
        .collect::<BTreeSet<_>>();
    if counterparts.is_empty() {
        return None;
    }
    let imported = counterparts
        .iter()
        .map(|counterpart| {
            (
                (counterpart.source.clone(), counterpart.identity),
                BTreeSet::from([STAND_IN]),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let added = programs
        .iter()
        .map(|program| program.clone().accept_imported_abilities(&imported))
        .sum::<usize>();
    Some(json!({
        "counterparts": counterparts
            .iter()
            .map(|counterpart| json!({
                "source": counterpart.source,
                "identity": format!("{:08X}", counterpart.identity),
                "native": counterpart.native.map(|native| format!("{native:08X}")),
            }))
            .collect::<Vec<_>>(),
        "conditions_added_for_one_imported_copy_each": added,
    }))
}

pub(crate) fn census(source: &Source, native: &mut Reader, folder: &Path) -> Result<Value> {
    let definitions = crate::tiger::shadowkeep::stat_definitions(native)?;
    let mut equivalents = BTreeMap::new();
    for (identity, plug) in STOCK_EQUIVALENTS {
        let tag = crate::tiger::shadowkeep::item_tag(native, plug)
            .with_context(|| format!("Stock perk plug {plug:08X}"))?;
        let data = native.tag(tag, Some(0x80807BEA))?;
        let perks = sundial::package_authoring::native_weapon::base_sandbox_perks(&data.0)
            .map_err(anyhow::Error::msg)?;
        equivalents.insert(identity, (plug, perks));
    }
    let none = BTreeMap::new();
    let templates = vec![0u16; 64];
    let budgets = stat::budgets(source)?;
    let mut grant_catalog = None;
    let mut item_properties = None;
    let mut results = BTreeMap::<String, Result<(), String>>::new();
    let mut report = serde_json::Map::new();
    let items = [
        (
            Host::Weapon,
            "weapons",
            source
                .weapons()?
                .into_iter()
                .map(|weapon| (weapon.definition, None))
                .collect::<Vec<_>>(),
        ),
        (
            Host::Armor,
            "armor",
            source
                .armor()?
                .into_iter()
                .map(|armor| (armor.definition, Some(armor.slot)))
                .collect::<Vec<_>>(),
        ),
        // Exotic armor again on its own, since its signature perks carry each item.
        (
            Host::Armor,
            "exotic_armor",
            source
                .armor()?
                .into_iter()
                .filter(|armor| u64::from(armor.rarity) == EXOTIC_RARITY)
                .map(|armor| (armor.definition, Some(armor.slot)))
                .collect::<Vec<_>>(),
        ),
    ];
    for (host, label, definitions_of_items) in items {
        let mut counted = 0usize;
        let mut steps = (0usize, 0usize);
        let mut distinct = BTreeMap::<String, bool>::new();
        let mut names = BTreeMap::<String, (bool, String)>::new();
        let mut reasons =
            BTreeMap::<String, (usize, BTreeMap<String, usize>, BTreeMap<String, Value>)>::new();
        // Each talent's ability counterparts, and how many conditions a stand-in imported copy of
        // each source ability adds when the build widens its programs.
        let mut gates = BTreeMap::<String, Value>::new();
        let mut grant_routes = BTreeMap::<String, Value>::new();
        for (definition, slot) in definitions_of_items {
            let Ok(talents) = source.item_talents(definition, &mut BTreeMap::new()) else {
                continue;
            };
            counted += 1;
            for node in talents["nodes"].as_array().into_iter().flatten() {
                for step in node["steps"].as_array().into_iter().flatten() {
                    let name = step["name"].as_str().unwrap_or_default().trim().to_owned();
                    let host_name = if host == Host::Weapon {
                        "weapon"
                    } else {
                        "armor"
                    };
                    // Budgeted stat rows give each armor slot its own points, so only steps with
                    // them are distinguished by slot.
                    let budgeted = step["variants"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .flat_map(|variant| {
                            variant["stat_adjustments"].as_array().into_iter().flatten()
                        })
                        .any(|row| {
                            row["hash"]
                                .as_u64()
                                .and_then(|hash| u32::try_from(hash).ok())
                                .is_some_and(|hash| budgets.contains_key(&hash))
                        });
                    let key = if budgeted {
                        format!("{host_name}|{slot:?}|{name}|{}", step["variants"])
                    } else {
                        format!("{host_name}|{name}|{}", step["variants"])
                    };
                    let result = match results.get(&key) {
                        Some(result) => result.clone(),
                        None => {
                            let result = lower_step(
                                &mut StepContext {
                                    source,
                                    native: &mut *native,
                                    folder,
                                    host,
                                    definitions: &definitions,
                                    equivalents: if host == Host::Weapon {
                                        &equivalents
                                    } else {
                                        &none
                                    },
                                    templates: &templates,
                                    grant_catalog: &mut grant_catalog,
                                    evades: None,
                                    slot,
                                    budgets: &budgets,
                                    item_properties: &mut item_properties,
                                },
                                step,
                                &name,
                            )
                            .map(|lowered| {
                                if let Some(gate) = ability_gates(&lowered.raw) {
                                    gates.insert(name.clone(), gate);
                                }
                                // The routes of grants that stand in for source rows, or leave
                                // some out, rather than carrying them privately.
                                let routes = lowered
                                    .grants
                                    .iter()
                                    .filter(|grant| {
                                        matches!(
                                            grant["route"].as_str(),
                                            Some("native_port_key" | "left_out")
                                        ) || grant["evidence"]["route"].is_string()
                                            || grant["evade"].is_object()
                                            || grant["route"] == "native_air_evade"
                                    })
                                    .cloned()
                                    .collect::<Vec<_>>();
                                if !routes.is_empty() {
                                    grant_routes.insert(name.clone(), Value::Array(routes));
                                }
                            })
                            .map_err(|error| format!("{error:#}"));
                            results.insert(key.clone(), result.clone());
                            result
                        }
                    };
                    // Infuse, empty grid steps and appearance choices are not perks.
                    if let Err(reason) = &result
                        && (reason == "Talent step has no standalone runtime perk"
                            || reason == "Appearance choices use the separate ornament route")
                    {
                        continue;
                    }
                    steps.1 += 1;
                    if let Err(reason) = &result {
                        let entry = reasons.entry(reason.clone()).or_default();
                        entry.0 += 1;
                        *entry.1.entry(name.clone()).or_default() += 1;
                        // Each talent's first refused step variants, enough to locate its records.
                        entry
                            .2
                            .entry(name.clone())
                            .or_insert_with(|| step["variants"].clone());
                    }
                    steps.0 += usize::from(result.is_ok());
                    distinct.insert(key, result.is_ok());
                    let entry = names
                        .entry(name)
                        .or_insert((false, result.clone().err().unwrap_or_default()));
                    entry.0 |= result.is_ok();
                }
            }
        }
        let never = names
            .iter()
            .filter(|(_, (ok, _))| !ok)
            .map(|(name, (_, reason))| json!({"name":name,"reason":reason}))
            .collect::<Vec<_>>();
        report.insert(
            label.into(),
            json!({"items":counted,"steps":steps.1,"steps_translated":steps.0,
                "distinct_steps":distinct.len(),"distinct_translated":distinct.values().filter(|ok| **ok).count(),
                "talents":names.len(),"talents_translated":names.values().filter(|(ok, _)| *ok).count(),
                "never_translated":never,"ability_gates":gates,"grant_routes":grant_routes,
                "refusals":reasons.iter().map(|(reason, (count, names, sample))| json!({"reason":reason,"steps":count,"talents":names,"sample":sample})).collect::<Vec<_>>()}),
        );
    }
    // Each exotic's signature talent, so the intrinsic rule can be read against every exotic.
    // Armor has no damage nodes and reads its own exotic-only names.
    let mut intrinsics = serde_json::Map::new();
    let weapons = source.weapons()?.into_iter().map(|weapon| {
        (
            Host::Weapon,
            weapon.hash,
            weapon.definition,
            weapon.rarity,
            weapon.name,
        )
    });
    let armor = source.armor()?.into_iter().map(|armor| {
        (
            Host::Armor,
            armor.hash,
            armor.definition,
            armor.rarity,
            armor.name,
        )
    });
    for (host, hash, definition, _, name) in weapons
        .chain(armor)
        .filter(|&(_, _, _, rarity, _)| u64::from(rarity) == EXOTIC_RARITY)
    {
        let Ok(talents) = source.item_talents(definition, &mut BTreeMap::new()) else {
            continue;
        };
        let nodes = talents["nodes"].as_array().cloned().unwrap_or_default();
        let (damage_nodes, exotic_talents) = match host {
            Host::Weapon => (
                source_damage(source, &nodes)?
                    .into_iter()
                    .map(|(node, _, _)| node)
                    .collect::<BTreeSet<_>>(),
                source.exotic_talents()?,
            ),
            Host::Armor => (BTreeSet::new(), source.exotic_armor_talents()?),
        };
        let column = trait_column(&nodes, &damage_nodes, exotic_talents, &name)?;
        let talent = column.and_then(|column| {
            nodes
                .iter()
                .find(|node| {
                    node["column"].as_i64() == Some(column)
                        && carries_talent(node)
                        && !damage_nodes.contains(&node["index"].as_u64().unwrap_or(u64::MAX))
                })
                .and_then(|node| node["steps"][0]["name"].as_str())
        });
        let first = nodes
            .iter()
            .filter(|node| node["column"].as_i64() == Some(0) && carries_talent(node))
            .filter_map(|node| node["steps"][0]["name"].as_str())
            .map(|talent| {
                let shown = exotic_talents.get(talent.trim());
                json!({"talent":talent,"exotic_only":shown.is_some(),"items":shown})
            })
            .collect::<Vec<_>>();
        intrinsics.insert(
            format!("{name} {hash:08X}"),
            json!({"host":if host == Host::Armor { "armor" } else { "weapon" },
                "column":column,"talent":talent,"first_column":first}),
        );
    }
    report.insert("exotic_intrinsics".into(), Value::Object(intrinsics));
    Ok(Value::Object(report))
}

struct Effects {
    actions: Vec<Action>,
    patches: Vec<sundial::package_authoring::sandbox_perk::program::NativeAssetPatch>,
    damage_profiles: Vec<sundial::package_authoring::sandbox_perk::program::FixedDamageProfile>,
    imported_assets: Vec<ImportedAsset>,
    omitted_feedback: usize,
}

fn translate_effects(
    source: &Source,
    data: &Payload,
    mut native: Option<&mut Reader>,
    folder: Option<&Path>,
    ancestors: &mut Vec<u32>,
) -> Result<Effects> {
    let mut omitted_feedback = 0;
    let mut patches = Vec::new();
    let mut damage_profiles = Vec::new();
    let mut imported_assets = Vec::new();
    let mut actions = Vec::new();
    for at in data
        .array(0x38, 8, None)?
        .into_iter()
        // Both engines activate stored effects from last to first. Program actions
        // are execution ordered, and the native compiler reverses them for storage.
        .rev()
    {
        let at = data.pointer(at)?;
        if pattern::closure_form(source, data, at)? {
            let (action, group, bank) = pattern::closure(
                source,
                native
                    .as_deref_mut()
                    .context("Attachment translation needs native templates")?,
                data,
                at,
                folder.context("Translated attachments need an import folder")?,
            )?
            .context("Source attachment has no translation")?;
            imported_assets.push(ImportedAsset {
                action_index: actions.len(),
                directory: group.directory,
                sha256: group.sha256,
                symbol: group.symbol,
                graph: None,
            });
            actions.push(action);
            if let Some((action, mut patch)) = bank {
                patch.action_index = actions.len();
                patches.push(patch);
                actions.push(action);
            }
            continue;
        }
        if matches!(
            (data.u32(at - 4)?, data.u8(at)?),
            (0x80802953, 0) | (0x80802B8D, 1)
        ) {
            let translated = pattern::read(
                source,
                native
                    .as_deref_mut()
                    .context("Pattern translation needs native component layouts")?,
                data,
                at,
                ancestors,
                &mut omitted_feedback,
                folder,
            )?;
            for (action, mut patch) in translated {
                patch.action_index = actions.len();
                patches.push(patch);
                actions.push(action);
            }
        } else if data.u32(at - 4)? == 0x808025A9 && data.u8(at)? == 3 {
            let native = native
                .as_deref_mut()
                .context("Direct effect translation needs native profiles")?;
            match direct::read(source, native, data, at) {
                Ok((action, mut profile)) => {
                    profile.action_index = actions.len();
                    damage_profiles.push(profile);
                    actions.push(action);
                }
                // A profile beyond the plain fixed form translates whole, as area profiles do.
                Err(plain) => {
                    let folder = folder.context("Translated profiles need an import folder")?;
                    let (action, mut imported) = direct::imported(source, native, data, at, folder)
                        .map_err(|error| {
                            anyhow::anyhow!("{plain:#}. Translated profile: {error:#}")
                        })?;
                    imported.action_index = actions.len();
                    imported_assets.push(imported);
                    actions.push(action);
                }
            }
        } else if (data.u32(at - 4)?, data.u8(at)?) == (0x808020EB, 30) {
            actions.extend(animation::read(data, at)?);
        } else {
            actions.push(effect(source, data, at, native.as_deref_mut())?);
        }
    }
    Ok(Effects {
        actions,
        patches,
        damage_profiles,
        imported_assets,
        omitted_feedback,
    })
}

fn assign_damage_choices(choices: &mut [Choice]) -> Result<()> {
    let damage_kinds = choices
        .iter()
        .filter(|c| c.column == i64::MIN)
        .filter_map(|c| damage_type(&c.lowered.raw))
        .collect::<BTreeSet<_>>();
    if damage_kinds.len() > 1 {
        let damage_nodes = choices
            .iter()
            .filter(|c| c.column == i64::MIN)
            .map(|c| c.report["node"].as_u64().context("Damage talent node"))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            damage_nodes.len() == 1,
            "Damage alternatives belong to different source nodes"
        );
        let alternatives = damage_nodes
            .first()
            .copied()
            .context("Damage talent node")?;
        for choice in &mut *choices {
            if choice.column == i64::MIN {
                choice.column = i64::MAX;
            } else if choice.column == i64::MAX && choice.node != alternatives {
                // The damage socket offers the alternatives. A choice from another node that
                // also does more than set the damage type keeps its own column.
                choice.column = choice.report["column"]
                    .as_i64()
                    .context("Source talent column")?;
            }
        }
    }
    for choice in choices.iter_mut().filter(|c| c.column == i64::MAX) {
        choice.report["damage_choice"] = json!(damage_type(&choice.lowered.raw));
    }
    Ok(())
}

type TalentChoices<'a> = (&'a [Choice], &'a [i64], &'a mut Vec<Value>);
type TalentOutput<'a> = (&'a mut Vec<Value>, &'a mut Vec<Value>, &'a mut Vec<Value>);

fn emit_talent_columns(
    projectile_names: &BTreeSet<u32>,
    (choices, source_columns, unsupported): TalentChoices<'_>,
    (signature, intrinsic): (Option<i64>, Option<&PerkSocket<'_>>),
    (perk_sockets, extra_layouts): (&[PerkSocket<'_>], &[PerkSocket<'_>]),
    (columns, variants, translated): TalentOutput<'_>,
) -> Result<()> {
    let mut ordinal = 0;
    for &column in source_columns {
        let members = distinct(choices, members(choices, column), unsupported)?;
        // Damage alternatives are steps of one node, which a roll would cut to its first step.
        // Every damage type stays selectable instead.
        let (members, reusable, offered) = if column == i64::MAX {
            let count = members.len();
            (members, count, count)
        } else {
            roll(choices, members, unsupported)
        };
        // The runtime base supplies socket layouts, not a limit on source talent columns.
        // Additional columns use a normal perk layout and still receive private plug identities.
        let extra;
        let socket = if signature == Some(column)
            && let Some(socket) = intrinsic
        {
            socket
        } else if let Some(socket) = perk_sockets.get(ordinal) {
            ordinal += 1;
            socket
        } else {
            let layout = perk_sockets
                .last()
                .or_else(|| extra_layouts.last())
                .context("Native base has no perk socket layout")?;
            extra = PerkSocket {
                index: columns.len(),
                socket_type: layout.socket_type,
                plugs: layout.plugs.clone(),
            };
            columns.push(Value::Null);
            &extra
        };
        let mut plugs = Vec::new();
        for (choice_index, index) in members.into_iter().enumerate() {
            let choice = &choices[index];
            // The template only lends its layout, and each choice still becomes its own private
            // item. Every choice takes the socket's first plug, so one talent on weapons with the
            // same base builds the same plug wherever its column places it, and the build shares it.
            let plug = socket.plugs[0];
            plugs.push(format!("0x{plug:08X}"));
            let mut variant = json!({"socket_index":socket.index,"choice_index":choice_index,
                "source_plug_hash":format!("0x{plug:08X}"),"name":choice.name,
                "description":choice.description,"replace_effects":true,
                "investment_stats":choice.lowered.stats,"additional_sandbox_perks":choice.lowered.stock,
                "sandbox_perks":choice.lowered.programs});
            // A bundled projectile replaces the fired one while its plug is equipped. A retained
            // Rise of Iron projectile whose bank already declares the dependency's channel reads
            // the property itself, as Gjallarhorn's rocket on carrier `80BBD121` reads Wolfpack
            // Rounds `456E31D3`, so it keeps firing.
            if let Some(dependency) = choice
                .lowered
                .raw
                .iter()
                .find_map(|program| program.projectile_dependency())
                .filter(|dependency| !projectile_names.contains(&dependency.key()))
            {
                variant["projectile"] = serde_json::to_value(dependency)?;
            }
            if let Some(icon) = &choice.icon {
                variant["icon"] =
                    json!({"source":"image","name":choice.name,"image":{"png_base64":icon}});
            }
            variants.push(variant);
            let mut report = choice.report.clone();
            report["socket"] = json!(socket.index);
            report["choice"] = json!(choice_index);
            report["reusable"] = json!(choice_index < reusable);
            report["template_plug"] = json!(format!("{plug:08X}"));
            translated.push(report);
        }
        if !plugs.is_empty() {
            let count = plugs.len();
            columns[socket.index] = json!({"choices":plugs,"socket_type":socket.socket_type});
            // Choices past the reusable ones exist only in the roll pool. A socket rolls as many
            // of them as the column had nodes it offered, one step per node as Rise of Iron did.
            if reusable < count {
                columns[socket.index]["reusable_choices"] = json!(reusable);
                columns[socket.index]["randomized_selection_program"] =
                    json!([{"opcode":11,"operand":offered}]);
            }
        }
    }
    Ok(())
}

type TalentTemplates<'a> = (
    &'a BTreeMap<u32, u16>,
    &'a BTreeMap<u32, (u32, Vec<u16>)>,
    &'a [u16],
);

struct LoweredTalents {
    choices: Vec<Choice>,
    unsupported: Vec<Value>,
    grant_catalog: Option<grant::Catalog>,
}

fn lower_talents(
    assembly: &mut Assembly<'_>,
    source: &Value,
    nodes: &[Value],
    host: Host,
    (definitions, equivalents, templates): TalentTemplates<'_>,
) -> Result<LoweredTalents> {
    let mut choices = Vec::new();
    let mut unsupported = Vec::new();
    let mut grant_catalog = None;
    let mut item_properties = None;
    let budgets = stat::budgets(assembly.source)?;
    // The armor import's item record carries its source slot.
    let slot = if host == Host::Armor {
        Some(u8::try_from(
            source["item"]["slot"]
                .as_u64()
                .context("Source armor slot")?,
        )?)
    } else {
        None
    };
    // Private evade actions join the shared human tables under names of this item's own. The
    // build reads them from armor graphs only, so weapons keep the native evade profile.
    let evades = format!(
        "evade-{:08x}",
        source["item"]["hash"]
            .as_u64()
            .context("Source item hash")?
    );
    for node in nodes {
        let column = node["column"].as_i64().context("Source talent column")?;
        let row = node["row"].as_i64().context("Source talent row")?;
        let node_index = node["index"].as_u64().context("Source talent node index")?;
        for step in node["steps"].as_array().context("Source talent steps")? {
            let name = step["name"].as_str().context("Source talent name")?;
            let lowered = lower_step(
                &mut StepContext {
                    source: assembly.source,
                    native: &mut assembly.native,
                    folder: &assembly.folder,
                    host,
                    definitions,
                    equivalents,
                    templates,
                    grant_catalog: &mut grant_catalog,
                    evades: (host == Host::Armor).then_some(evades.as_str()),
                    slot,
                    budgets: &budgets,
                    item_properties: &mut item_properties,
                },
                step,
                name,
            );
            let lowered = match lowered {
                Ok(lowered) => lowered,
                Err(error) => {
                    unsupported.push(json!({"node":node["index"],"step":step["index"],"name":name,"reason":format!("{error:#}")}));
                    continue;
                }
            };
            let report = json!({"node":node["index"],"step":step["index"],"name":name,
                "column":column,"row":row,"source_actions":lowered.tags,
                "stat_adjustments":lowered.adjustments,"stock_perk_plugs":lowered.stock_plugs,
                "ability_grants":lowered.grants,
                "source_audio_cues":lowered.audio_cues,
                "omitted_feedback_records":lowered.auxiliary});
            let container = u32::try_from(step["icon"].as_u64().unwrap_or(0))?;
            let icon = assembly
                .talent_icon(container)
                .with_context(|| format!("Source talent {name} icon"))?;
            // A choice that sets the weapon's damage type leaves its perk column. When that is
            // all it does, the recipe's damage type stands in for it. A choice that does more
            // keeps the rest on the damage socket, after the perk columns.
            let column = match damage_type(&lowered.raw) {
                Some(_) if only_sets_damage(&lowered) => i64::MIN,
                Some(_) => i64::MAX,
                None => column,
            };
            choices.push(Choice {
                column,
                row,
                node: node_index,
                name: name.to_owned(),
                description: step["description"].clone(),
                icon,
                lowered,
                report,
            });
        }
    }
    Ok(LoweredTalents {
        choices,
        unsupported,
        grant_catalog,
    })
}

fn signature_slot(
    sockets: &[Value],
    perk_sockets: &mut Vec<PerkSocket<'_>>,
    columns: &mut Vec<Value>,
    stat_socket_type: u16,
) -> (usize, Option<(u64, u16)>) {
    let mut stat_intrinsic = None;
    let first = sockets.first().filter(|first| {
        let disabled = first["socket_type"] == u16::MAX;
        let stat = first["choices"].as_array().is_some_and(|choices| {
            !choices.is_empty()
                && choices
                    .iter()
                    .all(|choice| choice["category"].as_u64() == Some(INTRINSIC_CATEGORY))
        });
        perk_sockets.first().is_none_or(|perk| perk.index > 0) && (disabled || stat)
    });
    let index = if let Some(first) = first {
        if first["socket_type"] != u16::MAX {
            let plug = first["choices"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|choice| choice["default"] == true)
                .map(|choice| &choice["hash"])
                .unwrap_or(&first["curated_fallback"])
                .as_u64();
            if let Some(plug) = plug {
                stat_intrinsic = Some((plug, stat_socket_type));
            }
        }
        0
    } else if perk_sockets.is_empty() {
        columns.push(Value::Null);
        columns.len() - 1
    } else {
        perk_sockets.remove(0).index
    };
    (index, stat_intrinsic)
}

type StockEquivalents = BTreeMap<u32, (u32, Vec<u16>)>;

fn stock_equivalents(native: &mut Reader, host: Host) -> Result<StockEquivalents> {
    let mut equivalents = BTreeMap::new();
    for (source, plug) in STOCK_EQUIVALENTS
        .into_iter()
        .filter(|_| host == Host::Weapon)
    {
        let tag = crate::tiger::shadowkeep::item_tag(native, plug)
            .with_context(|| format!("Stock perk plug {plug:08X}"))?;
        let data = native.tag(tag, Some(0x80807BEA))?;
        let perks = sundial::package_authoring::native_weapon::base_sandbox_perks(&data.0)
            .map_err(anyhow::Error::msg)?;
        ensure!(
            !perks.is_empty(),
            "Stock perk plug {plug:08X} has no sandbox perk"
        );
        equivalents.insert(source, (plug, perks));
    }
    Ok(equivalents)
}

fn signature_column(
    reader: &Source,
    source: &Value,
    nodes: &[Value],
    damage_nodes: &BTreeSet<u64>,
    host: Host,
    has_intrinsic: bool,
) -> Result<Option<i64>> {
    // Armor finds its signature among names only exotic armor shows, and a weapon among names only
    // exotic weapons show. A weapon base without an intrinsic socket has nowhere to put one.
    let exotic = source["item"]["rarity"].as_u64() == Some(EXOTIC_RARITY);
    let trait_at = if exotic && (has_intrinsic || host == Host::Armor) {
        let talents = match host {
            Host::Weapon => reader.exotic_talents()?,
            Host::Armor => reader.exotic_armor_talents()?,
        };
        trait_column(
            nodes,
            damage_nodes,
            talents,
            source["item"]["name"].as_str().unwrap_or_default(),
        )?
    } else {
        None
    };
    Ok(trait_at)
}

fn finish_intrinsic(
    native: &mut Reader,
    signature: Option<i64>,
    intrinsic: Option<&PerkSocket<'_>>,
    stat_intrinsic: Option<(u64, u16)>,
    (columns, variants): (&mut Vec<Value>, &mut Vec<Value>),
) -> Result<(bool, Option<Value>)> {
    // Talent failures stay in the choice report. A weapon with no translated talents
    // still keeps its appearance and base behavior, with unused donor perk lanes disabled.
    let mut legacy = false;
    if signature.is_none()
        && let Some(socket) = intrinsic
        && let Some(&plug) = socket.plugs.first()
    {
        columns[socket.index] =
            json!({"choices":[format!("0x{plug:08X}")],"socket_type":socket.socket_type});
        variants.push(json!({"socket_index":socket.index,"choice_index":0,
            "source_plug_hash":format!("0x{plug:08X}"),"name":LEGACY_FRAME,
            "description":LEGACY_FRAME_DESCRIPTION,"replace_effects":true,
            "investment_stats":[],"additional_sandbox_perks":[],"sandbox_perks":[],
            "icon":{"source":"texture","tag":LEGACY_FRAME_ICON}}));
        legacy = true;
    }
    // The moved stat intrinsic follows every perk column, so added perk sockets stay in order.
    // The item tooltip lists each visible finished perk a plug carries. A stock Year 1 exotic's stat
    // intrinsic carries investment stats and no finished perk, so it gets no tooltip row. A year-two
    // stat plug such as Heavy Titan Armor also carries a presentation perk, so the socket takes a
    // private copy with the plug's stats and no finished perks.
    let stat_intrinsic = if let Some((plug, socket_type)) = stat_intrinsic {
        let socket = columns.len();
        columns.push(json!({"choices":[format!("0x{plug:08X}")],"socket_type":socket_type}));
        let stats = crate::tiger::shadowkeep::investment_stats(native, u32::try_from(plug)?)?
            .into_iter()
            .map(|(index, value)| json!({"definition_index":index,"value":value}))
            .collect::<Vec<_>>();
        variants.push(json!({"socket_index":socket,"choice_index":0,
            "source_plug_hash":format!("0x{plug:08X}"),"replace_effects":true,
            "investment_stats":stats,"sandbox_perks":[]}));
        Some(json!({"socket":socket,"plug":format!("{plug:08X}"),"socket_type":socket_type}))
    } else {
        None
    };
    Ok((legacy, stat_intrinsic))
}

fn clear_talent_sockets(
    sockets: &[Value],
    intrinsic: Option<&PerkSocket<'_>>,
    perks: &[PerkSocket<'_>],
    host: Host,
) -> Vec<Value> {
    let mut columns = sockets.iter().map(|_| Value::Null).collect::<Vec<_>>();
    for socket in intrinsic.into_iter().chain(perks) {
        columns[socket.index] = json!({"choices":[],"socket_type":u16::MAX});
    }
    // The base's catalyst socket would offer upgrades no source weapon could take.
    if host == Host::Weapon {
        for (index, socket) in sockets.iter().enumerate() {
            if catalyst_socket(socket) {
                columns[index] = json!({"choices":[],"socket_type":u16::MAX});
            }
        }
    }
    columns
}

type SourceDamage = (u64, u64, Option<&'static str>);

fn primary_damage(
    choices: &[Choice],
    found: &[SourceDamage],
    host: Host,
) -> (Option<&'static str>, Option<&'static str>) {
    // The first damage choice in source order gives the weapon its damage type, the initial one
    // among alternatives. Without one, the source effects decide it.
    let chosen = choices
        .iter()
        .filter(|c| matches!(c.column, i64::MIN | i64::MAX))
        .find_map(|c| damage_type(&c.lowered.raw));
    let damage = chosen.or_else(|| {
        (host == Host::Weapon)
            .then(|| effect_damage(found))
            .flatten()
    });
    let damage_origin = match (chosen, damage) {
        (Some(_), _) => Some("talent"),
        (None, Some(_)) if found.is_empty() => Some("no_source_element"),
        (None, Some(_)) => Some("source_effect"),
        (None, None) => None,
    };
    (damage, damage_origin)
}
