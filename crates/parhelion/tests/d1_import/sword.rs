//! Compare a source sword owner's reachable attack rows with the staged private sword owner.
//! Both are decoded here from their packages. The converter's report supplies only the reason a
//! sword stayed native.
use super::*;
use sundial::package_authoring::{entity::weapon_component_bindings, runtime};

const BINDING: u32 = 0xCD2B_CEAC;
const EMPTY: u32 = 0x811C_9DC5;

#[derive(Clone, Debug, PartialEq)]
enum Contact {
    /// Capsule geometry and whether the contact names a damage profile.
    Melee([u32; 4], bool),
    Ranged(u32),
}

#[derive(Clone, Debug)]
struct Combo {
    kind: u8,
    windows: [u32; 3],
    rows: Vec<Row>,
}

/// One attack row's mechanics, in the fields both games store.
#[derive(Clone, Debug)]
struct Row {
    action: u32,
    key: u32,
    /// Minimum ammunition and the amount spent.
    ammo: Option<[u32; 2]>,
    /// Whether a condition other than ammunition gates the row.
    gated: bool,
    aim: [u32; 14],
    scalars: [Option<u32>; 3],
    flags: [u32; 8],
    /// Mode, distance, time, travel and two limits per block.
    trajectories: Vec<[u32; 6]>,
    strike: u32,
    mode: u32,
    lunge: u32,
    contact: Contact,
    /// The entity the attack reports with and the entity it spawns.
    entities: [u32; 2],
    combos: Vec<Combo>,
}

fn scalar(p: &Payload, code: usize, constants: usize) -> Result<Option<u32>> {
    let code = p
        .array(code, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| p.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let constants = p.array(constants, 16, Some(0x80800090))?;
    Ok(if code == [0x34, 0, 0x3E, 0] && constants.len() == 1 {
        Some(p.u32(constants[0])?)
    } else {
        None
    })
}

fn source_row(p: &Payload, r: usize, depth: usize) -> Result<Row> {
    ensure!(depth < 6, "Source sword combos nest too deeply");
    let ammo = if p.u64(r + 0x100)? == 0 {
        None
    } else {
        let t = p.pointer(r + 0x100)?;
        ensure!(p.u32(t - 4)? == 0x80803195, "Source sword condition class");
        Some([p.u32(t)?, p.u32(t + 4)?])
    };
    let mut aim = [0; 14];
    for (index, value) in aim.iter_mut().enumerate() {
        *value = p.u32(r + 0x0C + index * 4)?;
    }
    let mut flags = [0; 8];
    for (value, offset) in flags
        .iter_mut()
        .zip([0x124, 0x12C, 0x130, 0x134, 0x138, 0x13C, 0x140, 0x144])
    {
        *value = p.u32(r + offset)?;
    }
    let mut trajectories = Vec::new();
    for entry in p.array(r + 0x108, 8, Some(0x808028AF))? {
        let b = p.pointer(entry)?;
        ensure!(p.u32(b - 4)? == 0x80802975, "Source sword trajectory class");
        trajectories.push([
            p.u32(b)?,
            p.u32(b + 4)?,
            p.u32(b + 8)?,
            p.u32(b + 0x10)?,
            p.u32(b + 0x14)?,
            p.u32(b + 0x18)?,
        ]);
    }
    let c = p.pointer(r + 0x170)?;
    let contact = match p.u32(c - 4)? {
        0x808031A4 => Contact::Melee(
            [p.u32(c)?, p.u32(c + 4)?, p.u32(c + 8)?, p.u32(c + 0x18)?],
            p.u32(c + 0x28)? != u32::MAX,
        ),
        0x8080307A => Contact::Ranged(p.u32(c + 0x18)?),
        other => anyhow::bail!("Source sword contact class {other:08X}"),
    };
    let mut combos = Vec::new();
    for c in p.array(r + 0x180, 0x20, Some(0x808026AC))? {
        combos.push(Combo {
            kind: p.u8(c)?,
            windows: [p.u32(c + 4)?, p.u32(c + 8)?, p.u32(c + 12)?],
            rows: p
                .array(c + 0x10, 0x198, Some(0x80802A15))?
                .into_iter()
                .map(|row| source_row(p, row, depth + 1))
                .collect::<Result<_>>()?,
        });
    }
    Ok(Row {
        action: p.u32(r)?,
        key: p.u32(r + 8)?,
        ammo,
        gated: false,
        aim,
        scalars: [
            scalar(p, r + 0x58, r + 0x68)?,
            scalar(p, r + 0x90, r + 0xA0)?,
            scalar(p, r + 0xC8, r + 0xD8)?,
        ],
        flags,
        trajectories,
        strike: p.u32(r + 0x168)?,
        mode: p.u32(r + 0x178)?,
        lunge: p.u32(r + 0x194)?,
        contact,
        entities: [p.u32(r + 0x150)?, p.u32(r + 0x160)?],
        combos,
    })
}

fn native_row(p: &Payload, r: usize, depth: usize) -> Result<Row> {
    ensure!(depth < 6, "Staged sword combos nest too deeply");
    let mut ammo = None;
    let mut gated = false;
    for entry in p.array(r + 0x18, 8, Some(0x80802D6E))? {
        let t = p.pointer(entry)?;
        match p.u32(t - 4)? {
            0x80802D70 => {
                ensure!(ammo.is_none(), "Staged sword row has two ammunition gates");
                ammo = Some([p.u32(t)?, p.u32(t + 4)?]);
            }
            _ => gated = true,
        }
    }
    let mut aim = [0; 14];
    for (index, value) in aim.iter_mut().enumerate() {
        *value = p.u32(r + 0x28 + index * 4)?;
    }
    let mut flags = [0; 8];
    for (value, offset) in flags
        .iter_mut()
        .zip([0x14C, 0x154, 0x158, 0x15C, 0x160, 0x164, 0x174, 0x178])
    {
        *value = p.u32(r + offset)?;
    }
    let mut trajectories = Vec::new();
    for entry in p.array(r + 0x118, 8, Some(0x80804446))? {
        let b = p.pointer(entry)?;
        ensure!(p.u32(b - 4)? == 0x80804444, "Staged sword trajectory class");
        trajectories.push([
            p.u32(b)?,
            p.u32(b + 0x20)?,
            p.u32(b + 0x30)?,
            p.u32(b + 0x3C)?,
            p.u32(b + 0x44)?,
            p.u32(b + 0x48)?,
        ]);
    }
    let c = p.pointer(r + 0x1A8)?;
    let contact = match p.u32(c - 4)? {
        0x80804455 => Contact::Melee(
            [p.u32(c)?, p.u32(c + 4)?, p.u32(c + 8)?, p.u32(c + 0x18)?],
            p.u32(c + 0x28)? != u32::MAX,
        ),
        0x80804454 => Contact::Ranged(p.u32(c + 0x18)?),
        other => anyhow::bail!("Staged sword contact class {other:08X}"),
    };
    let mut combos = Vec::new();
    for c in p.array(r + 0x1F0, 0xE8, Some(0x80802D6D))? {
        let window = |at: usize| -> Result<u32> {
            scalar(p, c + at, c + at + 0x10)?.context("Staged sword combo window is not a constant")
        };
        combos.push(Combo {
            kind: p.u8(c)?,
            windows: [window(0x10)?, window(0x48)?, window(0x80)?],
            rows: p
                .array(c + 0xD8, 8, Some(0x80802EAE))?
                .into_iter()
                .map(|entry| native_row(p, p.pointer(entry)?, depth + 1))
                .collect::<Result<_>>()?,
        });
    }
    Ok(Row {
        action: p.u32(r + 4)?,
        key: p.u32(r + 0xC)?,
        ammo,
        gated,
        aim,
        scalars: [
            scalar(p, r + 0x70, r + 0x80)?,
            scalar(p, r + 0xA8, r + 0xB8)?,
            scalar(p, r + 0xE0, r + 0xF0)?,
        ],
        flags,
        trajectories,
        strike: p.u32(r + 0x1A0)?,
        mode: p.u32(r + 0x1B0)?,
        lunge: p.u32(r + 0x204)?,
        contact,
        entities: [p.u32(r + 0x188)?, p.u32(r + 0x198)?],
        combos,
    })
}

/// The rows selection can reach. Both games take the first row whose key is empty or held and
/// whose ammunition gate passes, so a later row another row always precedes is never taken.
fn reachable(rows: &[Row]) -> Vec<&Row> {
    let mut kept: Vec<&Row> = Vec::new();
    for row in rows {
        let shadowed = kept.iter().any(|earlier| {
            (earlier.key == EMPTY || earlier.key == row.key)
                && match (earlier.ammo, row.ammo) {
                    (None, _) => true,
                    (Some(a), Some(b)) => f32::from_bits(a[0]) <= f32::from_bits(b[0]),
                    (Some(_), None) => false,
                }
        });
        if !shadowed {
            kept.push(row);
        }
    }
    kept
}

struct Scope<'a> {
    required_keys: &'a BTreeSet<u32>,
    /// Source attacks whose base row lacks one of their combo kinds, so the continuations of
    /// that kind stay the base's.
    native_combos: &'a BTreeSet<u32>,
    paired: usize,
    keyed: BTreeMap<u32, usize>,
    /// Keyed rows that play an action of another name than the source's, source row first.
    specials: Vec<(Row, Row)>,
}

/// Holds the staged row to the source row's mechanics. Delivery stays the native row's, so the
/// contact geometry is compared only where both rows deliver damage the same way, and the
/// trajectory only where both rows hold the same blocks.
fn agrees(source: &Row, staged: &Row, place: &str, private: bool) -> Result<()> {
    ensure!(
        private || source.action == staged.action,
        "{place}: staged action differs"
    );
    ensure!(
        source.ammo == staged.ammo,
        "{place}: ammunition gate {:?} is not the source's {:?}",
        staged.ammo,
        source.ammo
    );
    ensure!(source.aim == staged.aim, "{place}: aim limits differ");
    ensure!(
        source.flags == staged.flags,
        "{place}: movement flags differ"
    );
    for (index, (from, to)) in source.scalars.iter().zip(&staged.scalars).enumerate() {
        if from.is_some() && to.is_some() {
            ensure!(from == to, "{place}: constant {index} differs");
        }
    }
    ensure!(
        source.strike == staged.strike,
        "{place}: strike time differs"
    );
    ensure!(source.mode == staged.mode, "{place}: contact mode differs");
    ensure!(source.lunge == staged.lunge, "{place}: lunge differs");
    if source.trajectories.len() == staged.trajectories.len()
        && source
            .trajectories
            .iter()
            .zip(&staged.trajectories)
            .all(|(a, b)| a[0] == b[0])
    {
        ensure!(
            source.trajectories == staged.trajectories,
            "{place}: trajectory differs"
        );
    }
    match (&source.contact, &staged.contact) {
        (Contact::Melee(a, from), Contact::Melee(b, to)) if from == to => {
            ensure!(a == b, "{place}: contact geometry differs");
        }
        (Contact::Ranged(a), Contact::Ranged(b)) => {
            ensure!(a == b, "{place}: ranged contact differs");
        }
        _ => {}
    }
    Ok(())
}

/// The staged set selects the source's translated rows, in the source's order, and no others.
/// A keyed source row may be absent, when Shadowkeep has no row that plays its action.
fn compare(source: &[Row], staged: &[Row], place: &str, scope: &mut Scope<'_>) -> Result<()> {
    let mut next = 0;
    for row in reachable(source) {
        let place = format!("{place}/{:08X}", row.action);
        if row.key != EMPTY {
            let Some(candidate) = staged.get(next).filter(|s| s.key == row.key) else {
                continue;
            };
            ensure!(
                !candidate.gated,
                "{place}: keyed row keeps a gate the source row lacks"
            );
            // A keyed row under another action name plays a private action, held to the
            // source's action after the lists.
            let private = candidate.action != row.action;
            agrees(row, candidate, &place, private)?;
            if private {
                scope.specials.push((row.clone(), candidate.clone()));
            }
            *scope.keyed.entry(row.key).or_default() += 1;
            next += 1;
            continue;
        }
        let partner = staged
            .get(next)
            .with_context(|| format!("{place}: no staged row carries this source attack"))?;
        ensure!(
            partner.key == EMPTY && partner.ammo.is_some() == row.ammo.is_some(),
            "{place}: staged row {:08X} stands where the source attack belongs",
            partner.action
        );
        agrees(row, partner, &place, false)?;
        scope.paired += 1;
        next += 1;
        for combo in &row.combos {
            let Some(continued) = partner.combos.iter().find(|c| c.kind == combo.kind) else {
                ensure!(
                    scope.native_combos.contains(&row.action),
                    "{place}: staged row lacks combo kind {}",
                    combo.kind
                );
                continue;
            };
            ensure!(
                combo.windows == continued.windows,
                "{place}: combo {} windows differ",
                combo.kind
            );
            compare(
                &combo.rows,
                &continued.rows,
                &format!("{place}:{}", combo.kind),
                scope,
            )?;
        }
        for combo in &partner.combos {
            ensure!(
                row.combos.iter().any(|c| c.kind == combo.kind) || combo.rows.is_empty(),
                "{place}: staged combo kind {} continues into rows the source lacks",
                combo.kind
            );
        }
    }
    ensure!(
        next == staged.len(),
        "{place}: staged set selects {} rows the source lacks",
        staged.len() - next
    );
    Ok(())
}
/// The Rise of Iron human bank and action table every player action resolves in, with the pose
/// owner naming their layer and direction tables, and the native lookup and pose owners.
const SOURCE_HUMAN: [u32; 3] = [0x80A7_E33C, 0x80A7_4754, 0x80A7_E157];
const NATIVE_HUMAN: [u32; 2] = [0x80EF_62E5, 0x8161_D5F7];

/// A descriptor's supplemental route edges per layer family, each an option and a layer name,
/// then its aim and look aliases.
type Pose = ([Vec<(u16, u32)>; 2], [u16; 2]);

/// One game's human tables: bank, action table, pose layers and directions.
struct Human {
    bank: Payload,
    states: Payload,
    layers: Payload,
    directions: Payload,
    native: bool,
}

impl Human {
    fn source(source: &Source) -> Result<Self> {
        let pose = source.read(SOURCE_HUMAN[2])?;
        let at = pose.pointer(24)?;
        Ok(Self {
            bank: source.read(SOURCE_HUMAN[0])?,
            states: source.read(SOURCE_HUMAN[1])?,
            layers: source.read(pose.u32(at + 0x118)?)?,
            directions: source.read(pose.u32(at + 0x110)?)?,
            native: false,
        })
    }

    fn native(reader: &mut Reader) -> Result<Self> {
        let lookup = reader.tag(NATIVE_HUMAN[0], Some(0x80809C36))?;
        let pose = reader.tag(NATIVE_HUMAN[1], Some(0x80809C36))?;
        let (lp, pp) = (lookup.pointer(24)?, pose.pointer(24)?);
        Ok(Self {
            bank: (*reader.tag(lookup.u32(lp + 0x90)?, None)?).clone(),
            states: (*reader.tag(lookup.u32(lp + 0x9C)?, None)?).clone(),
            layers: (*reader.tag(pose.u32(pp + 0x11C)?, None)?).clone(),
            directions: (*reader.tag(pose.u32(pp + 0x114)?, None)?).clone(),
            native: true,
        })
    }

    /// Every action name with its node.
    fn names(&self) -> Result<BTreeMap<u32, usize>> {
        let nodes = self.states.array(24, 16, None)?;
        self.states
            .array(8, 8, None)?
            .into_iter()
            .map(|row| {
                Ok((
                    self.states.u32(row)?,
                    *nodes
                        .get(self.states.u32(row + 4)? as usize)
                        .context("Human action node")?,
                ))
            })
            .collect()
    }

    /// The descriptors an unconditional action chooses among, or none for a conditional one.
    fn choices(&self, node: usize) -> Result<Option<Vec<usize>>> {
        if self.states.u64(node)? != 1 {
            return Ok(None);
        }
        let list = self.states.pointer(node + 8)?;
        self.states
            .array(list, 8, None)?
            .into_iter()
            .map(|row| Ok(self.states.u32(row)? as usize))
            .collect::<Result<Vec<_>>>()
            .map(Some)
    }

    fn descriptor(&self, index: usize) -> Result<usize> {
        self.bank
            .array(0x68, 32, None)?
            .get(index)
            .copied()
            .context("Human bank descriptor")
    }

    fn clip(&self, descriptor: usize) -> Result<u32> {
        let slot = usize::from(self.bank.u16(self.descriptor(descriptor)? + 24)?);
        self.bank.u32(
            *self
                .bank
                .array(8, 4, None)?
                .get(slot)
                .context("Human bank clip")?,
        )
    }

    /// A descriptor's supplemental pose, as each route edge's option and layer name per layer
    /// family, and its aim and look aliases.
    fn pose(&self, descriptor: usize) -> Result<Pose> {
        let alias = |payload: &Payload, field: usize| -> Result<u16> {
            payload.u16(
                *payload
                    .array(field, 2, Some(0x80800006))?
                    .get(descriptor)
                    .context("Pose dispatch descriptor")?,
            )
        };
        let mut edges = [Vec::new(), Vec::new()];
        let route = alias(&self.layers, 40)?;
        if route != u16::MAX {
            let row = *self
                .layers
                .array(56, 32, None)?
                .get(usize::from(route))
                .context("Pose route")?;
            for (family, list) in edges.iter_mut().enumerate() {
                let layers = self.layers.array(8 + family * 16, 24, None)?;
                for edge in self.layers.array(row + family * 16, 4, None)? {
                    let layer = *layers
                        .get(usize::from(self.layers.u16(edge + 2)?))
                        .context("Pose route layer")?;
                    list.push((self.layers.u16(edge)?, self.layers.u32(layer + 16)?));
                }
            }
        }
        Ok((
            edges,
            [alias(&self.directions, 8)?, alias(&self.directions, 0x38)?],
        ))
    }

    fn frames(&self, clip: &Payload) -> Result<u16> {
        clip.u16(if self.native { 0x13C } else { 0x128 })
    }
}

/// A keyed attack with no Shadowkeep action of its name plays a private human action and
/// spawns the source's own entities. The private action exists only in the staged human tables,
/// chooses as many clips as the source action, each a private clip of the source clip's length
/// under the source descriptor's playback policy. Each clip poses as the source's does: its
/// supplemental route selects the same options of the same named layers, and its aim and look
/// aliases are the ones Shadowkeep gives an action both games hold whose Rise of Iron aliases
/// are this action's. Each entity the source row names is a private graph held to the source
/// graph, and a field the source leaves empty stays empty.
fn special(
    source: &Source,
    stock: &mut Reader,
    staged: &mut Reader,
    from: &Row,
    to: &Row,
) -> Result<Value> {
    let place = format!("Private attack {:08X}", from.action);
    let original = Human::source(source)?;
    let native = Human::native(stock)?;
    let built = Human::native(staged)?;
    ensure!(
        !native.names()?.contains_key(&to.action) && !native.names()?.contains_key(&from.action),
        "{place}: Shadowkeep already holds the action"
    );
    let wanted = original
        .choices(
            *original
                .names()?
                .get(&from.action)
                .with_context(|| format!("{place}: source action is absent"))?,
        )?
        .with_context(|| format!("{place}: source action is conditional"))?;
    let chosen = built
        .choices(
            *built
                .names()?
                .get(&to.action)
                .with_context(|| format!("{place}: staged tables lack the private action"))?,
        )?
        .with_context(|| format!("{place}: private action is conditional"))?;
    ensure!(
        wanted.len() == chosen.len(),
        "{place}: private action chooses among {} clips, the source among {}",
        chosen.len(),
        wanted.len()
    );
    // Aliases are shared indices, so the staged pair must be one a shared action witnesses.
    let mut witnessed = BTreeSet::new();
    let stock_names = native.names()?;
    for (name, &node) in &original.names()? {
        let Some(&other) = stock_names.get(name) else {
            continue;
        };
        let (Some(a), Some(b)) = (original.choices(node)?, native.choices(other)?) else {
            continue;
        };
        if a.len() == b.len() {
            for (a, b) in a.into_iter().zip(b) {
                witnessed.insert((original.pose(a)?.1, native.pose(b)?.1));
            }
        }
    }
    let stock_descriptors = native.bank.array(0x68, 32, None)?.len();
    let mut clips = Vec::new();
    for (&a, &b) in wanted.iter().zip(&chosen) {
        ensure!(
            b >= stock_descriptors,
            "{place}: private action plays a stock descriptor"
        );
        ensure!(
            original.bank.bytes::<8>(original.descriptor(a)? + 16)?
                == built.bank.bytes::<8>(built.descriptor(b)? + 16)?,
            "{place}: playback policy or duration differs"
        );
        let clip = built.clip(b)?;
        ensure!(
            stock.tag(clip, None).is_err(),
            "{place}: private action plays a stock clip"
        );
        ensure!(
            original.frames(&source.read(original.clip(a)?)?)?
                == built.frames(&*staged.tag(clip, Some(0x80808F49))?)?,
            "{place}: clip length differs"
        );
        let (edges, aliases) = original.pose(a)?;
        let (staged_edges, staged_aliases) = built.pose(b)?;
        ensure!(
            edges == staged_edges,
            "{place}: supplemental pose {staged_edges:X?} is not the source's {edges:X?}"
        );
        ensure!(
            (aliases == [u16::MAX; 2] && staged_aliases == [u16::MAX; 2])
                || witnessed.contains(&(aliases, staged_aliases)),
            "{place}: aim and look aliases {staged_aliases:?} have no shared action as witness for the source's {aliases:?}"
        );
        clips.push(json!({"source_descriptor":a,"staged_descriptor":b,"clip":clip,
            "pose":edges.iter().flatten().map(|(option, layer)| json!([option, layer])).collect::<Vec<_>>(),
            "aliases":staged_aliases}));
    }
    let mut entities = Vec::new();
    for (&tag, &built) in from.entities.iter().zip(&to.entities) {
        if tag == u32::MAX {
            ensure!(
                built == u32::MAX,
                "{place}: row names an entity the source row lacks"
            );
            continue;
        }
        ensure!(
            built != u32::MAX,
            "{place}: source entity {tag:08X} is not carried"
        );
        perk::translated_graph(source, stock, staged, &source.read(tag)?, built)
            .with_context(|| format!("{place}: entity {tag:08X}"))?;
        entities.push(json!({"source":tag,"staged":built}));
    }
    Ok(
        json!({"action":from.action,"key":from.key,"private_action":to.action,
        "clips":clips,"entities":entities}),
    )
}

/// The staged guard owners against the source's guards, each decoded from its packages.
///
/// Both games scale the damage a guarding entity takes row by row: a row that matches the
/// damage multiplies it by the row's share and adds the row's ammunition cost. A staged guard
/// carrying a source name therefore holds the source guard's exclusions and one row per source
/// row with the same damage type masks and bypass flags, the same share as a constant, the same
/// ammunition cost and no sword energy cost. A source row without labels skips both label
/// tests. Which labels a row with labels keeps is the label registry translation's contract.
fn guards(
    source: &Source,
    entity: &Payload,
    stock: &mut Reader,
    staged: &mut Reader,
    (runtime, donor): (&[u8], &[u8]),
    required: &BTreeSet<u32>,
) -> Result<Value> {
    const GUARD: u32 = 0x5211_1D67;
    let names = |p: &Payload, at: usize| -> Result<Vec<u32>> {
        p.array(at, 4, Some(0x80800070))?
            .into_iter()
            .map(|row| p.u32(row))
            .collect()
    };
    for binding in weapon_component_bindings(donor, GUARD).map_err(anyhow::Error::msg)? {
        ensure!(
            staged.tag(binding.owner_tag, Some(0x80809C36))?.0
                == stock.tag(binding.owner_tag, Some(0x80809C36))?.0,
            "Guard translation changed a stock guard owner"
        );
    }
    let mut carried = BTreeMap::new();
    let mut open = Vec::new();
    for binding in weapon_component_bindings(runtime, GUARD).map_err(anyhow::Error::msg)? {
        let owner = staged.tag(binding.owner_tag, Some(0x80809C36))?;
        let instance = usize::try_from(binding.resource_offset)?;
        ensure!(
            owner.u32(instance - 4)? == 0x808042AC,
            "Staged guard resource class"
        );
        let definition = usize::try_from(owner.u64(instance + 8)?)?;
        ensure!(
            owner.u32(definition - 4)? == 0x80803C69,
            "Staged guard definition class"
        );
        match owner.u32(definition + 0x2B8)? {
            EMPTY => open.push(names(&owner, definition + 0x2C0)?),
            key => {
                ensure!(
                    carried
                        .insert(key, (owner, instance, definition, binding.owner_tag))
                        .is_none(),
                    "Two staged guards carry key {key:08X}"
                );
            }
        }
    }
    let mut evidence = Vec::new();
    for r in entity.array(32, 12, Some(0x80800715))? {
        let tag = entity.u32(r)?;
        let from = source.read(tag)?;
        let d = from.pointer(24)?;
        if d < 4 || from.u32(d - 4)? != 0x8080315A {
            continue;
        }
        let key = from.u32(d + 0x270)?;
        let Some((owner, instance, definition, staged_owner)) = carried.get(&key) else {
            ensure!(
                !required.contains(&key),
                "Required guard key {key:08X} has no staged guard owner"
            );
            continue;
        };
        let (instance, definition) = (*instance, *definition);
        ensure!(
            names(&from, d + 0x278)? == names(owner, definition + 0x2C0)?,
            "Guard {key:08X} exclusions differ"
        );
        let rows = from.array(d + 0x108, 0x54, Some(0x80802B5E))?;
        let emitted = owner.array(definition + 0x110, 0xA8, Some(0x808029B7))?;
        let live = owner.array(instance + 0x80, 0x50, Some(0x808029B6))?;
        ensure!(
            !rows.is_empty() && rows.len() == emitted.len() && rows.len() == live.len(),
            "Guard {key:08X} holds {} rows and {} row instances for {} source rows",
            emitted.len(),
            live.len(),
            rows.len()
        );
        // Row instances are live state, inside the span the client copies for each weapon.
        let (root, schema) = (owner.pointer(16)?, owner.pointer(24)?);
        ensure!(
            root == instance
                && live[0] > instance
                && live[live.len() - 1] + 0x50 <= schema
                && owner.u64(0x48)? == (schema - root) as u64,
            "Guard {key:08X} row instances lie outside the instance span"
        );
        for (index, ((&row, &native), &state)) in rows.iter().zip(&emitted).zip(&live).enumerate() {
            let place = format!("Guard {key:08X} row {index}");
            ensure!(
                from.bytes::<8>(row)? == owner.bytes::<8>(native + 0x10)?,
                "{place}: damage type masks differ"
            );
            ensure!(
                from.bytes::<2>(row + 0x48)? == owner.bytes::<2>(native + 0x28)?,
                "{place}: damage type bypass differs"
            );
            ensure!(
                scalar(owner, native + 0x60, native + 0x70)? == Some(from.u32(row + 0x4C)?),
                "{place}: share is not the source's constant"
            );
            ensure!(
                owner.u64(native + 0x90)? == 0,
                "{place}: share reads an input"
            );
            ensure!(
                owner.u32(native + 0xA0)? == from.u32(row + 0x50)?
                    && owner.u32(native + 0xA4)? == 0,
                "{place}: ammunition or energy cost differs"
            );
            if from.bytes::<2>(row + 0x4A)? == [1, 1] {
                let masks = owner.pointer(native + 0x20)?;
                ensure!(
                    owner.bytes::<2>(masks + 80)? == [1, 1],
                    "{place}: gained a label test"
                );
            }
            // A row and its instance name each other, and the instance names its owner's root.
            ensure!(
                owner.u64(native + 8)? == state as u64
                    && owner.u64(state + 8)? == native as u64
                    && owner.pointer(state + 0x10)? == instance
                    && owner.u64(native + 0x58)? == (state + 0x20) as u64
                    && owner.u64(state + 0x28)? == (native + 0x50) as u64
                    && owner.pointer(state + 0x30)? == state
                    && owner.u64(state + 0x40)? == 0,
                "{place}: row and instance links differ"
            );
        }
        ensure!(
            open.iter().all(|excluded| excluded.contains(&key)),
            "The open guard does not yield to carried key {key:08X}"
        );
        evidence.push(
            json!({"source_guard":tag,"key":key,"rows":rows.len(),"staged_owner":staged_owner}),
        );
    }
    for key in required {
        ensure!(
            carried.contains_key(key),
            "Required guard key {key:08X} was not carried"
        );
    }
    Ok(json!(evidence))
}

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    staged: &mut Reader,
    receipt: &Value,
) -> Result<Value> {
    if receipt["graph"]["kind"] != "weapon" {
        return Ok(Value::Null);
    }
    let original = &receipt["graph"]["rendering"][0]["source_runtime"];
    let root = u32::from_str_radix(
        original["runtime_entity"]
            .as_str()
            .context("Source runtime")?,
        16,
    )?;
    let entity = source.read(root)?;
    let mut owners = Vec::new();
    for r in entity.array(32, 12, Some(0x80800715))? {
        let tag = entity.u32(r)?;
        let p = source.read(tag)?;
        let d = p.pointer(24)?;
        if d >= 4 && p.u32(d - 4)? == 0x80802056 {
            owners.push((tag, p, d));
        }
    }
    if owners.is_empty() {
        return Ok(Value::Null);
    }
    ensure!(owners.len() == 1, "Source sword owner is ambiguous");
    let (source_tag, from, from_definition) = &owners[0];
    let numbers = |field: &str| -> Result<BTreeSet<u32>> {
        receipt["case"][field]
            .as_array()
            .into_iter()
            .flatten()
            .map(number)
            .collect()
    };
    let required_keys = numbers("required_sword_keys")?;
    let required_guards = numbers("required_guard_keys")?;
    let report = &receipt["graph"]["firing_translation"]["sword"];
    if report["status"] != "translated" {
        ensure!(
            required_keys.is_empty() && required_guards.is_empty(),
            "Required source sword attacks did not translate: {}",
            report["reason"]
        );
        return Ok(json!({"sword":"native","reason":report["reason"]}));
    }
    let manager = |reader: &Reader| {
        sundial::package_authoring::PackageManager::new(
            &reader.manager.package_dir,
            reader.manager.version,
            Some(reader.manager.platform),
        )
        .map_err(anyhow::Error::msg)
    };
    let staged_runtime = runtime::load_runtime_entity_for_item_with_manager(
        &manager(staged)?,
        number(&receipt["item"])?,
    )
    .map_err(anyhow::Error::msg)?;
    let bindings =
        weapon_component_bindings(&staged_runtime.payload, BINDING).map_err(anyhow::Error::msg)?;
    ensure!(bindings.len() == 1, "Staged sword binding differs");
    let owner = staged.tag(bindings[0].owner_tag, Some(0x80809C36))?;
    let donor = runtime::load_runtime_entity_for_item_with_manager(
        &manager(stock)?,
        number(&receipt["donor"])?,
    )
    .map_err(anyhow::Error::msg)?;
    let donor_bindings =
        weapon_component_bindings(&donor.payload, BINDING).map_err(anyhow::Error::msg)?;
    ensure!(donor_bindings.len() == 1, "Stock sword binding differs");
    ensure!(
        bindings[0].owner_tag != donor_bindings[0].owner_tag,
        "Translated sword attacks were not given a private owner"
    );
    ensure!(
        staged.tag(donor_bindings[0].owner_tag, Some(0x80809C36))?.0
            == stock.tag(donor_bindings[0].owner_tag, Some(0x80809C36))?.0,
        "Sword translation changed the stock sword owner"
    );
    let instance = usize::try_from(bindings[0].resource_offset)?;
    ensure!(
        owner.u32(instance - 4)? == 0x808043D2,
        "Staged sword resource class"
    );
    let definition = usize::try_from(owner.u64(instance + 8)?)?;
    ensure!(
        owner.u32(definition - 4)? == 0x80802D66,
        "Staged sword definition class"
    );
    let native_combos = numbers("sword_native_combos")?;
    let mut scope = Scope {
        required_keys: &required_keys,
        native_combos: &native_combos,
        paired: 0,
        keyed: BTreeMap::new(),
        specials: Vec::new(),
    };
    let mut lists = 0;
    for (name, from_lists, to_lists) in [("light", 0x268, 0x230), ("heavy", 0x278, 0x240)] {
        let targets = owner.array(definition + to_lists, 0x78, Some(0x808043D7))?;
        for list in from.array(from_definition + from_lists, 0x68, Some(0x808021A6))? {
            let kind = from.u8(list)?;
            let rows = from
                .array(list + 0x50, 0x198, Some(0x80802A15))?
                .into_iter()
                .map(|row| source_row(from, row, 0))
                .collect::<Result<Vec<_>>>()?;
            let matching = targets
                .iter()
                .filter(|&&t| owner.u8(t).ok() == Some(kind))
                .collect::<Vec<_>>();
            ensure!(
                matching.len() == 1,
                "Staged {name} lists hold {} of selection kind {kind}",
                matching.len()
            );
            // Selection walks the list's row references, so they are what a player reaches.
            let selected = owner
                .array(matching[0] + 0x60, 8, Some(0x80802EAE))?
                .into_iter()
                .map(|entry| native_row(&owner, owner.pointer(entry)?, 0))
                .collect::<Result<Vec<_>>>()?;
            let stored = owner.array(matching[0] + 0x50, 0x220, Some(0x80802D7B))?;
            ensure!(
                stored.len() == selected.len(),
                "Staged {name} list stores {} rows and selects {}",
                stored.len(),
                selected.len()
            );
            compare(&rows, &selected, &format!("{name}[{kind}]"), &mut scope)?;
            lists += 1;
        }
    }
    ensure!(lists > 0 && scope.paired > 0, "Empty sword attack contract");
    for key in scope.required_keys {
        ensure!(
            scope.keyed.contains_key(key),
            "Required sword key {key:08X} has no staged attack row"
        );
    }
    // The key set holds a name for each talent that writes one, so it has the source's slots.
    // It is instance data, which the client copies only between the owner's instance and
    // schema roots, with the extent at header `+48`.
    let from_instance = from.pointer(16)?;
    let source_capacity = from
        .array(from_instance + 0x1E0, 8, Some(0x80802401))?
        .len();
    let capacity = owner.array(instance + 0x1F0, 8, Some(0x808043D5))?.len();
    let keys = owner.pointer(instance + 0x1F8)? + 16;
    let (root, schema) = (owner.pointer(16)?, owner.pointer(24)?);
    ensure!(
        capacity >= source_capacity
            && root == instance
            && keys > instance
            && keys + capacity * 8 <= schema
            && owner.u64(0x48)? == (schema - root) as u64,
        "Staged sword key set of {capacity} slots lies outside the weapon's instance span"
    );
    let guards = guards(
        source,
        &entity,
        stock,
        staged,
        (&staged_runtime.payload, &donor.payload),
        &required_guards,
    )?;
    let specials = scope
        .specials
        .iter()
        .map(|(from, to)| special(source, stock, staged, from, to))
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"source_owner":source_tag,"staged_owner":bindings[0].owner_tag,"guards":guards,
        "private_attacks":specials,
        "stock_owner":donor_bindings[0].owner_tag,"stock_owner_preserved":true,
        "paired_rows":scope.paired,
        "keyed_rows":scope.keyed.iter().map(|(key, rows)| json!({"key":key,"rows":rows})).collect::<Vec<_>>(),
        "key_capacity":capacity,"source_key_capacity":source_capacity,
        "gameplay_verified":false}),
    )
}
