//! Attachments whose entity holds stat modifiers, a status, or both.
//!
//! A modern modifier entity is one or more `80802D2B` components, each with rows that add to or
//! multiply one input of one component interface. Shadowkeep's modifier component `80803AFE`
//! holds the same rows. Nothing builds a Shadowkeep entity from nothing, so each source row is
//! written into a private copy of a shipped graph.
//!
//! Without a status, each row takes a shipped one-row graph and the attachment becomes one
//! Shadowkeep attachment for each row. The rows of one entity are independent, so attaching
//! them separately for the same lifetime changes the same inputs. An entity that also holds
//! the plain network component `80808EDD` takes the shipped graph that pairs one row with
//! Shadowkeep's network component, so each row stays replicated.
//!
//! With a plain status `8080B11F`, the entity takes the shipped graph of one status and as
//! many rows as it has, up to five, so one status times and names all of them. See
//! [`super::status`].
//!
//! A dynamic attachment `80803130` names the same entities as a plain one and adds a value
//! program, which its Shadowkeep node `80803E44` keeps.
//!
//! A row names its destination by component and input numbers, which differ between the
//! builds. `table/modifiers.json` holds the destinations twin perks agree on: the same perk's
//! modifier rows on both sides, paired by position.
use std::{collections::BTreeMap, sync::Arc, sync::OnceLock};

use anyhow::{Context, Result, anyhow, ensure};
use serde::Deserialize;
use sundial::package_authoring::{
    entity::weapon_component_bindings,
    fnv1,
    sandbox_perk::program::{
        Action, Asset, AttachmentTarget, HudStatus, ImportedAsset, NativeAssetPatch,
        NativeAssetResourcePatch, NativeNode,
    },
};

use super::{lower, status, table};
use crate::d2_mot::payload::Payload;

const EMPTY: u32 = 0x811C_9DC5;
/// The shipped effect graph whose one modifier row carries each translated row. Its row is
/// found through its binding and checked, never assumed.
pub const ENVELOPE: u32 = 0x8162_C91A;
/// The shipped graph of Shadowkeep's network component and one modifier row, for a source
/// entity that replicates its modifiers.
pub const NETWORKED: u32 = 0x80C1_9433;
/// Shipped graphs of one status and as many modifier rows as their index.
const TIMED: [u32; 6] = [
    0x80BC_31BC,
    0x80FF_05F0,
    0x8161_E50E,
    0x8161_E47B,
    0x8161_E480,
    0x80C1_9A4F,
];
const MODIFIER_BINDING: u32 = 0x7330_E39F;
/// The stock HUD status whose generic buff row and icon a named status shows.
pub(super) const BUFF_ICON: u32 = 0xC35D_D059;
/// The settings of the modern network component on every entity surveyed: no host state.
const PLAIN_NETWORK: [u8; 56] = {
    let mut bytes = [0; 56];
    let mut at = 8;
    while at < 16 {
        bytes[at] = 0xFF;
        bytes[at + 16] = 0xFF;
        at += 1;
    }
    bytes
};
/// A source entity holds at most this many rows, one Shadowkeep attachment each.
const ROW_LIMIT: usize = 24;

/// Tag access for both builds, so the same translation runs on packages and on dumped payloads.
pub trait Tags {
    /// The tag the modern wide reference at `field` of `p` names.
    fn reference(&mut self, p: &Payload, field: usize) -> Result<u32>;
    /// A modern payload of the given class.
    fn source(&mut self, tag: u32, class: u32) -> Result<Arc<Payload>>;
    /// A Shadowkeep payload.
    fn native(&mut self, tag: u32) -> Result<Arc<Payload>>;
    /// Convert the whole entity that the plain attachment or the spawn at `at` names into a
    /// private attachment group, for an entity the row forms here do not cover: one with
    /// presentation, a status with inputs or creation parameters. Only an import with both
    /// package sets and a folder for the group can.
    fn whole(&mut self, _p: &Payload, _at: usize) -> Result<Whole> {
        anyhow::bail!(
            "not tried, since this run has no package sets or import folder to convert it with"
        )
    }
    /// The icon the source HUD shows beside the status named `name`, as a base64 PNG. Only
    /// an import with the source packages can read it. Without one the status keeps the
    /// generic buff icon.
    fn status_image(&mut self, _name: u32) -> Option<String> {
        None
    }
}

/// A whole converted entity: the attachment and the group it names. The caller sets the
/// group's action index once the action has its place in the program.
pub struct Whole {
    pub action: Action,
    pub asset: ImportedAsset,
}

#[derive(Deserialize)]
struct File {
    destinations: BTreeMap<String, (u8, u16, usize, usize)>,
    /// Inputs between two voted inputs that sit the same distance from their Shadowkeep ones.
    #[serde(default)]
    interpolated: BTreeMap<String, (u8, u16, u16, u16)>,
    /// Inputs between two voted inputs whose Shadowkeep inputs are adjacent.
    #[serde(default)]
    absent: BTreeMap<String, (u16, u16)>,
}

/// Whether a modern modifier destination is an input Shadowkeep's component does not have: one
/// between two voted inputs whose Shadowkeep inputs are adjacent, which leaves it no place. A
/// row that names it changes nothing Shadowkeep can express, so it is left out of its entity.
pub fn absent(component: u8, input: u16) -> Result<bool> {
    type Absent = std::result::Result<Vec<(u8, u16)>, String>;
    static ABSENT: OnceLock<Absent> = OnceLock::new();
    let absent = ABSENT
        .get_or_init(|| {
            let file: File = serde_json::from_str(include_str!("table/modifiers.json"))
                .map_err(|error| format!("modifier destination table is invalid: {error}"))?;
            file.absent
                .into_keys()
                .map(|key| {
                    let (component, input) = key
                        .split_once(',')
                        .ok_or("absent modifier key has no comma")?;
                    Ok((
                        component.parse().map_err(|_| "absent modifier component")?,
                        input.parse().map_err(|_| "absent modifier input")?,
                    ))
                })
                .collect::<std::result::Result<_, &str>>()
                .map_err(str::to_owned)
        })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))?;
    Ok(lower::modifier_input(component, input).is_err() && absent.contains(&(component, input)))
}

/// Whether the modifier row at `row` of a modern owner names an input Shadowkeep lacks.
pub fn absent_row(owner: &Payload, row: usize) -> Result<bool> {
    absent(owner.u8(row + 96)?, owner.u16(row + 90)?)
}

/// How many modifiers of the entity that the attachment at `at` names change an input Shadowkeep
/// lacks. Every route leaves them out. They are the rows of its modifier components and of the
/// bank on the first input of its status, whose four inputs hold one row for each modifier.
pub fn left_out(tags: &mut dyn Tags, p: &Payload, at: usize) -> Result<usize> {
    let tag = tags.reference(p, at + 16)?;
    let graph = tags.source(tag, 0x8080_9AD8)?;
    let mut count = 0;
    for entry in graph.array(8, 12, Some(0x8080_9ACD))? {
        let owner = tags.source(graph.u32(entry)?, 0x8080_9B06)?;
        let settings = owner.pointer(24)?;
        ensure!(settings >= 4, "source component has no class words");
        let bank = match owner.u32(settings - 4)? {
            0x8080_2D2B => settings + 0x48,
            0x8080_B11F if owner.u64(settings + 0x650)? != 0 => owner.pointer(settings + 0x650)?,
            _ => continue,
        };
        for row in owner.array(bank + 0x10, 112, Some(0x8080_2D33))? {
            count += usize::from(absent_row(&owner, row)?);
        }
    }
    Ok(count)
}

/// The Shadowkeep component and input of a modern modifier destination: the destinations with a
/// traced Shadowkeep consumer first, then the ones twin perks agree on, then an input between
/// two voted inputs of its component that keep one distance from their Shadowkeep inputs. No
/// twin shows such an input, so its place is an inference from the order of its neighbors.
pub fn destination(component: u8, input: u16) -> Result<(u8, u16)> {
    /// Modern (component, input) to Shadowkeep (component, input), or why the table is invalid.
    type Voted = std::result::Result<BTreeMap<(u8, u16), (u8, u16)>, String>;
    static VOTED: OnceLock<Voted> = OnceLock::new();
    if let Ok(traced) = lower::modifier_input(component, input) {
        return Ok(traced);
    }
    let voted = VOTED
        .get_or_init(|| {
            let file: File = serde_json::from_str(include_str!("table/modifiers.json"))
                .map_err(|error| format!("modifier destination table is invalid: {error}"))?;
            // A vote replaces an interpolated entry of the same key.
            file.interpolated
                .into_iter()
                .map(|(key, (component, input, _, _))| (key, (component, input)))
                .chain(
                    file.destinations
                        .into_iter()
                        .map(|(key, (component, input, _, _))| (key, (component, input))),
                )
                .map(|(key, (native_component, native_input))| {
                    let (component, input) = key
                        .split_once(',')
                        .ok_or("modifier destination key has no comma")?;
                    Ok((
                        (
                            component.parse().map_err(|_| "modifier component")?,
                            input.parse().map_err(|_| "modifier input")?,
                        ),
                        (native_component, native_input),
                    ))
                })
                .collect::<std::result::Result<_, &str>>()
                .map_err(str::to_owned)
        })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))?;
    voted.get(&(component, input)).copied().with_context(|| {
        format!("source modifier component {component} input {input} has no Shadowkeep destination among twin perks")
    })
}

/// One modifier row of a shipped graph: where it is and how an edit addresses it.
struct Row {
    at: usize,
    resource_index: u16,
    /// The row's offset within its component resource.
    relative: u32,
}

/// A shipped graph a private copy starts from.
struct Stock {
    graph: Arc<Payload>,
    /// The modifier owner and its bound rows, in settings order.
    modifiers: Option<(Arc<Payload>, Vec<Row>)>,
    status: Option<(u32, Arc<Payload>)>,
    /// Owners a private copy leaves out.
    remove: Vec<u32>,
}

/// Read the shipped graph `tag`. Its rows are found through their bindings and checked,
/// never assumed. `timed` keeps its status for the source's own.
fn stock(tags: &mut dyn Tags, tag: u32, timed: bool) -> Result<Stock> {
    let graph = tags.native(tag)?;
    let bindings =
        weapon_component_bindings(&graph.0, MODIFIER_BINDING).map_err(anyhow::Error::msg)?;
    let mut stock = Stock {
        graph: graph.clone(),
        modifiers: None,
        status: None,
        remove: Vec::new(),
    };
    for entry in graph.array(16, 12, Some(0x8080_9C04))? {
        let owner_tag = graph.u32(entry)?;
        let owner = tags.native(owner_tag)?;
        let instance = owner.pointer(16)?;
        let settings = owner.pointer(24)?;
        ensure!(
            instance >= 4 && settings >= 4,
            "Shadowkeep component has no class words"
        );
        match owner.u32(settings - 4)? {
            0x8080_3AFE if stock.modifiers.is_none() => {
                ensure!(
                    owner.u32(instance - 4)? == 0x8080_3AFD
                        && owner.u32(instance + 4)? == 0x8080_3AFE
                        && owner.u32(settings + 4)? == 0x8080_3AFD,
                    "the Shadowkeep modifier owner has other instance or settings classes"
                );
                let mut rows = Vec::new();
                for at in owner.array(settings + 88, 88, Some(0x8080_3B06))? {
                    // A row's runtime record is the resource a binding selects, and names
                    // the row back. A shipped graph can hold rows no binding selects.
                    let resource = owner.u64(at + 8)?;
                    let Some(binding) = bindings.iter().find(|binding| {
                        binding.owner_tag == owner_tag
                            && binding.concrete_class == 0x8080_3B05
                            && binding.resource_count == 1
                            && binding.resource_offset == resource
                    }) else {
                        continue;
                    };
                    let resource = usize::try_from(resource)?;
                    ensure!(
                        owner.u32(at)? == owner_tag
                            && owner.u32(at + 4)? == 0x8080_3B05
                            && owner.u32(resource)? == owner_tag
                            && owner.u32(resource + 4)? == 0x8080_3B06
                            && owner.u64(resource + 8)? == at as u64,
                        "a Shadowkeep modifier resource and its row disagree"
                    );
                    rows.push(Row {
                        at,
                        resource_index: u16::try_from(binding.resource_index)?,
                        relative: at
                            .checked_sub(resource)
                            .and_then(|offset| u32::try_from(offset).ok())
                            .context("a Shadowkeep modifier row lies before its resource")?,
                    });
                }
                stock.modifiers = Some((owner, rows));
            }
            0x8080_4205 if timed && stock.status.is_none() => {
                stock.status = Some((owner_tag, owner));
            }
            // The one-row graph's status gives its row a lifetime no translated row has.
            0x8080_4205 if tag == ENVELOPE => stock.remove.push(owner_tag),
            // The networked graph's network component is what its row is there for.
            0x8080_90E8 if tag == NETWORKED => {}
            class => anyhow::bail!(
                "Shadowkeep graph {tag:08X} holds an unexpected component {class:08X}"
            ),
        }
    }
    ensure!(
        stock.modifiers.as_ref().map_or(0, |(_, rows)| rows.len()) == bindings.len()
            && (!timed || stock.status.is_some()),
        "Shadowkeep graph {tag:08X} does not have the rows and status its place needs"
    );
    Ok(stock)
}

/// One Shadowkeep attachment: its action and the private edit of the graph it names. The
/// caller sets the edit's action index once the action has its place in the program.
pub struct Attached {
    pub action: Action,
    pub patch: NativeAssetPatch,
}

/// How the source attaches its entity: a plain attachment to one target, or a dynamic one,
/// whose node is written again for each Shadowkeep graph.
enum Carrier {
    /// The target, and whether the source keeps the entity when its parent is not the host.
    Plain(AttachmentTarget, bool),
    Dynamic,
}

impl Carrier {
    fn read(p: &Payload, at: usize) -> Result<Self> {
        ensure!(at >= 4, "source effect has no class word");
        match p.u32(at - 4)? {
            0x8080_B7C6 => {
                let head = p.bytes::<8>(at)?;
                ensure!(
                    head[0] == 1 && head[1] == 1 && head[3..] == [0; 5],
                    "unsupported source attachment header"
                );
                let mode = match head[2] {
                    0 => AttachmentTarget::ThisItem,
                    1 => AttachmentTarget::Player,
                    2 => AttachmentTarget::TriggeringWeapon,
                    3 => AttachmentTarget::OtherCombatant,
                    target => {
                        anyhow::bail!(
                            "source attachment target {target} has no Shadowkeep selector"
                        )
                    }
                };
                // A removal key, a stacks key or creation parameters need the entity to read
                // them, and these entities read none. The record ends at `+40`.
                ensure!(
                    p.u32(at + 0x20)? <= 1
                        && p.u32(at + 0x24)? == EMPTY
                        && p.u32(at + 0x28)? == 0
                        && p.u32(at + 0x2C)? == EMPTY
                        && p.u64(at + 0x30)? == 0
                        && p.u64(at + 0x38)? == 0,
                    "source attachment has a removal key, a stacks key or creation parameters"
                );
                Ok(Self::Plain(mode, p.u32(at + 0x20)? == 1))
            }
            0x8080_3130 => {
                // The node's own form is checked before any entity is read.
                lower::dynamic_entity(p, at, ENVELOPE)?;
                Ok(Self::Dynamic)
            }
            class => anyhow::bail!("source effect {class:08X} is not an attachment"),
        }
    }

    /// The Shadowkeep action naming `graph`. `lifetime` is the key that keeps the attachment
    /// past its action, and `hud` the status it shows.
    fn action(
        &self,
        p: &Payload,
        at: usize,
        graph: u32,
        lifetime: u32,
        hud: Option<HudStatus>,
    ) -> Result<Action> {
        Ok(match self {
            Self::Plain(mode, _) => Action::Attach {
                asset: Asset {
                    graph,
                    hud_status: hud,
                    ..Asset::default()
                },
                mode: *mode,
                keys: [lifetime, EMPTY],
                float_bits: [0; 4],
            },
            Self::Dynamic => {
                let node = dynamic(p, at, graph)?;
                Action::Native {
                    node: NativeNode {
                        kind: node.kind,
                        bytes: node.bytes,
                    },
                }
            }
        })
    }
}

/// The Shadowkeep node of the modern dynamic attachment at `at`, naming the entity `graph`.
pub fn dynamic(p: &Payload, at: usize, graph: u32) -> Result<lower::Node> {
    let mut node = lower::dynamic_entity(p, at, graph)?;
    let code = p.array_range(at + 40, 1, Some(0x8080_0009))?;
    widen_constants(&mut node.bytes, &table::pushed(&p.0[code]))?;
    Ok(node)
}

/// Give the constants of a dynamic attachment node that its source code pushes the lanes
/// Shadowkeep stores.
fn widen_constants(bytes: &mut [u8], pushed: &[usize]) -> Result<()> {
    let field = |at: usize| -> Result<[u8; 8]> {
        Ok(bytes
            .get(at..at + 8)
            .context("dynamic attachment node is truncated")?
            .try_into()?)
    };
    let count = usize::try_from(u64::from_le_bytes(field(48)?))?;
    if count == 0 {
        return Ok(());
    }
    let rows = usize::try_from(56 + i64::from_le_bytes(field(56)?))? + 16;
    for index in (0..count).filter(|index| pushed.contains(index)) {
        let at = rows + index * 16;
        let lanes: &mut [u8; 16] = bytes
            .get_mut(at..at + 16)
            .context("dynamic attachment constants are truncated")?
            .try_into()?;
        table::widen(lanes);
    }
    Ok(())
}

/// What a source entity holds, when it is one of the forms this route writes.
struct Entity {
    tag: u32,
    /// Each modifier row with its owner, in component and settings order.
    rows: Vec<(Arc<Payload>, usize)>,
    networked: bool,
    status: Option<status::Status>,
}

fn entity(tags: &mut dyn Tags, p: &Payload, at: usize) -> Result<Entity> {
    let tag = tags.reference(p, at + 16)?;
    let graph = tags.source(tag, 0x8080_9AD8)?;
    ensure!(
        graph.u64(0)? == graph.0.len() as u64 && graph.bytes::<32>(24)? == [0; 32],
        "source entity has external input connections"
    );
    let owners = graph.array(8, 12, Some(0x8080_9ACD))?;
    ensure!(!owners.is_empty(), "source entity has no component");
    let mut entity = Entity {
        tag,
        rows: Vec::new(),
        networked: false,
        status: None,
    };
    for entry in owners {
        let owner = tags.source(graph.u32(entry)?, 0x8080_9B06)?;
        let instance = owner.pointer(16)?;
        let settings = owner.pointer(24)?;
        ensure!(
            instance >= 4 && settings >= 4,
            "source component has no class words"
        );
        match (owner.u32(settings - 4)?, owner.u32(instance - 4)?) {
            (0x8080_2D2B, 0x8080_2D2A) => {
                // The weight every modifier twin entity holds: constant and unit, with no
                // driver.
                ensure!(
                    owner.f32(instance + 96)? == 1.0
                        && owner.f32(instance + 100)? == 1.0
                        && owner.bytes::<4>(instance + 104)? == [0, 0, 1, 1],
                    "source modifier component does not have a constant unit weight"
                );
                for row in owner.array(settings + 88, 112, Some(0x8080_2D33))? {
                    if !absent_row(&owner, row)? {
                        entity.rows.push((owner.clone(), row));
                    }
                }
            }
            (0x8080_8EDD, 0x8080_8EDB) => {
                ensure!(
                    !entity.networked && owner.bytes::<56>(settings + 16)? == PLAIN_NETWORK,
                    "source entity has a second network component or one with host state"
                );
                entity.networked = true;
            }
            (0x8080_B11F, _) if entity.status.is_none() => {
                entity.status = Some(status::read(&owner)?);
            }
            (class, _) => anyhow::bail!(
                "source entity holds component {class:08X} beside its stat modifiers and status"
            ),
        }
    }
    ensure!(
        entity.rows.len() <= ROW_LIMIT,
        "source entity holds {} modifier rows",
        entity.rows.len()
    );
    Ok(entity)
}

/// The edit that writes one source row over one shipped row.
fn row_patch(
    source: &Payload,
    row: usize,
    owner: &Payload,
    slot: &Row,
) -> Result<NativeAssetResourcePatch> {
    let bytes = lower::modifier_settings_with(source, row, owner, slot.at, destination)?;
    Ok(NativeAssetResourcePatch {
        binding_hash: MODIFIER_BINDING,
        resource_index: slot.resource_index,
        offset: slot.relative,
        expected: owner.bytes::<88>(slot.at)?.to_vec(),
        bytes: bytes.to_vec(),
        imported_particle: None,
    })
}

fn edit(
    graph: u32,
    patches: Vec<NativeAssetResourcePatch>,
    remove_owners: Vec<u32>,
) -> NativeAssetPatch {
    NativeAssetPatch {
        action_index: 0,
        source_graph: graph,
        patches,
        appends: Vec::new(),
        remove_owners,
        components: Vec::new(),
        programs: Vec::new(),
    }
}

/// Translate the modern plain or dynamic attachment at `at` when its entity holds stat
/// modifiers with constant unit weight, a plain status, or both, and no external input.
/// `name` is the perk's, which a named status shows on the HUD.
pub fn attach(tags: &mut dyn Tags, p: &Payload, at: usize, name: &str) -> Result<Vec<Attached>> {
    let carrier = Carrier::read(p, at)?;
    let entity = entity(tags, p, at)?;
    let Some(status) = &entity.status else {
        ensure!(
            !entity.rows.is_empty(),
            "source entity holds no modifier row and no status"
        );
        let target = if entity.networked {
            NETWORKED
        } else {
            ENVELOPE
        };
        let stock = stock(tags, target, false)?;
        let Some((owner, [slot])) = stock
            .modifiers
            .as_ref()
            .map(|(owner, rows)| (owner, rows.as_slice()))
        else {
            anyhow::bail!("Shadowkeep graph {target:08X} does not bind one modifier row");
        };
        return entity
            .rows
            .iter()
            .map(|(source, row)| {
                Ok(Attached {
                    action: carrier.action(p, at, target, EMPTY, None)?,
                    patch: edit(
                        target,
                        vec![row_patch(source, *row, owner, slot)?],
                        stock.remove.clone(),
                    ),
                })
            })
            .collect();
    };
    ensure!(
        !entity.networked,
        "source entity replicates a status, which has no Shadowkeep graph on this route"
    );
    let Carrier::Plain(_, kept) = carrier else {
        anyhow::bail!("a dynamic attachment of a status has no Shadowkeep graph on this route");
    };
    let target = *TIMED
        .get(entity.rows.len())
        .context("source status times more modifier rows than any Shadowkeep status graph")?;
    let stock = stock(tags, target, true)?;
    let (status_tag, status_owner) = stock
        .status
        .as_ref()
        .context("Shadowkeep status graph has no status")?;
    let mut patches = status::patches(&stock.graph, *status_tag, status_owner, status)?;
    if let Some((owner, slots)) = &stock.modifiers {
        ensure!(
            slots.len() == entity.rows.len(),
            "Shadowkeep status graph {target:08X} binds {} modifier rows",
            slots.len()
        );
        for ((source, row), slot) in entity.rows.iter().zip(slots) {
            patches.push(row_patch(source, *row, owner, slot)?);
        }
    } else {
        ensure!(
            entity.rows.is_empty(),
            "Shadowkeep status graph {target:08X} has no modifier component"
        );
    }
    ensure!(
        !patches.is_empty(),
        "source status equals the Shadowkeep graph it would edit"
    );
    // Twin attachments of a status hold a lifetime key exactly when the source keeps the
    // entity unless its parent is the host: 81 of 82 against none of 172 without. The key
    // only has to be the attachment's own, so it is named after the source entity.
    let lifetime = if kept {
        let key = fnv1(&format!("d2_mot/attachment/{:08X}/lifetime", entity.tag));
        ensure!(
            !matches!(key, 0 | u32::MAX | EMPTY),
            "the attachment lifetime key is reserved"
        );
        key
    } else {
        EMPTY
    };
    let hud = status.named().then(|| HudStatus {
        name: name.to_owned(),
        icon: Some(BUFF_ICON),
        image: tags.status_image(status.name()),
    });
    Ok(vec![Attached {
        action: carrier.action(p, at, target, lifetime, hud)?,
        patch: edit(target, patches, Vec::new()),
    }])
}
