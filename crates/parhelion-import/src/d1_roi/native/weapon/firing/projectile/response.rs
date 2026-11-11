//! Response destinations stay stable while object headers and local channel ordinals change.
use super::*;
use sundial::package_authoring::sandbox_perk::program::NativeAssetResourcePatch;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Expression {
    code: Vec<u8>,
    constants: Vec<[u8; 16]>,
    counts: [u64; 2],
    names: Vec<u32>,
}

impl Expression {
    pub(super) fn code(&self) -> &[u8] {
        &self.code
    }
    pub(super) fn constant_rows(&self) -> &[[u8; 16]] {
        &self.constants
    }
}

pub(super) fn program(p: &Payload, at: usize) -> Result<Expression> {
    Ok(Expression {
        code: p
            .array(at, 1, Some(0x80800009))?
            .into_iter()
            .map(|r| p.u8(r))
            .collect::<Result<_>>()?,
        constants: p
            .array(at + 16, 16, Some(0x80800090))?
            .into_iter()
            .map(|r| p.bytes::<16>(r))
            .collect::<Result<_>>()?,
        counts: [p.u64(at + 32)?, p.u64(at + 40)?],
        names: Vec::new(),
    })
}

fn expression(
    p: &Payload,
    at: usize,
    old: bool,
    path: &str,
    inputs: &mut BTreeMap<usize, String>,
) -> Result<Expression> {
    let header = if old { 24 } else { 16 };
    let mut expr = program(p, at + header)?;
    for (i, row) in p
        .array(
            at + header + 48,
            if old { 48 } else { 40 },
            Some(if old { 0x80800927 } else { 0x80809789 }),
        )?
        .into_iter()
        .enumerate()
    {
        expr.names.push(p.u32(row + if old { 40 } else { 32 })?);
        ensure!(
            inputs.insert(row, format!("{path}/input/{i}")).is_none(),
            "Repeated expression input object"
        );
    }
    Ok(expr)
}

/// Rise of Iron steering parameter extent, and its programs by offset.
const STEERING: usize = 0x108;
const STEERING_PROGRAMS: [usize; 4] = [0x20, 0x68, 0xA0, 0xD8];

/// A label filter's four lists by name, and which of its four operations apply: match any,
/// require all, exclude any and exclude all together. Each version compiles its own masks from
/// its own label registry, so filters compare by label name and operation.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Filter {
    lists: [Vec<u32>; 4],
    active: [bool; 4],
}

impl Filter {
    /// Rise of Iron keeps four lists of `80800365` rows over dictionary `80AAC26E`, then four
    /// 256-bit masks and one bypass byte per operation at `+D0`.
    pub(super) fn source(p: &Payload, at: usize) -> Result<Self> {
        ensure!(
            p.u64(at + 0x40)? == 0 && p.u32(at + 0x48)? == 0x80AAC26E,
            "Source response filter has another dictionary"
        );
        let mut active = [false; 4];
        for (i, flag) in active.iter_mut().enumerate() {
            *flag = p.u8(at + 0xD0 + i)? == 0;
        }
        Ok(Self {
            lists: Self::lists(p, at, 0x80800365, 0x80AAC26E)?,
            active,
        })
    }

    /// Shadowkeep keeps a `808093F3` filter with four lists of `808094B3` rows over registry
    /// `80C70CA1`, then a compiled `808094A8` predicate: mode 0 applies match any and exclude any
    /// with bypass bytes `+50/+51` of its `808093F6` record, mode 1 all four with `+A0..+A3` of
    /// `808093F5`, and mode 255 none.
    pub(super) fn native(p: &Payload, at: usize) -> Result<Self> {
        ensure!(
            p.u64(at + 0x40)? == 0 && p.u32(at + 0x48)? == 0x80C70CA1,
            "Native response filter has another registry"
        );
        let active = match p.u8(at + 0x50)? {
            0xFF => [false; 4],
            0 => {
                let record = p.pointer(at + 0x58)?;
                ensure!(
                    p.u32(record - 4)? == 0x808093F6,
                    "Native response filter record differs"
                );
                [
                    p.u8(record + 0x50)? == 0,
                    false,
                    p.u8(record + 0x51)? == 0,
                    false,
                ]
            }
            1 => {
                let record = p.pointer(at + 0x58)?;
                ensure!(
                    p.u32(record - 4)? == 0x808093F5,
                    "Native response filter record differs"
                );
                let mut active = [false; 4];
                for (i, flag) in active.iter_mut().enumerate() {
                    *flag = p.u8(record + 0xA0 + i)? == 0;
                }
                active
            }
            _ => anyhow::bail!("Native response filter mode is unqualified"),
        };
        Ok(Self {
            lists: Self::lists(p, at, 0x808094B3, 0x80C70CA1)?,
            active,
        })
    }

    /// Whether the filter names no label, so it accepts every object.
    fn is_empty(&self) -> bool {
        self.lists.iter().all(Vec::is_empty)
    }

    fn lists(p: &Payload, at: usize, class: u32, registry: u32) -> Result<[Vec<u32>; 4]> {
        let mut lists: [Vec<u32>; 4] = Default::default();
        for (i, list) in lists.iter_mut().enumerate() {
            for row in p.array(at + i * 0x10, 0x18, Some(class))? {
                ensure!(
                    p.u32(row + 0x10)? == registry,
                    "Response filter label has another registry"
                );
                list.push(p.u32(row)?);
            }
        }
        Ok(lists)
    }
}

struct Action {
    selector: u8,
    parameter: usize,
    common: Vec<u8>,
    resource: Option<u32>,
    expressions: Vec<Expression>,
    /// Root scalar indices the parameter names, compared through the scalar pairing.
    indices: Vec<u32>,
    filter: Option<Filter>,
    /// Offset of the label filter, when there is one.
    filter_at: usize,
    /// Whether a source and carrier class pair is mapped for this selector. An unmapped action
    /// can only be left out.
    known: bool,
}

/// Shadowkeep numbers Rise of Iron selectors 13, 15 and 16 as 14, 16 and 17.
fn native_selector(selector: u8, old: bool) -> u8 {
    match selector {
        13 | 15 | 16 if old => selector + 1,
        other => other,
    }
}

fn action(p: &Payload, row: usize, old: bool) -> Result<Action> {
    let parameter = p.pointer(row)?;
    let selector = p.u8(row + 8)?;
    let (source, native, size) = match selector {
        2 => (0x8080294A, 0x808037EA, 208),
        3 => (0x80802B07, 0x808037F6, 1),
        4 => (0x80802AE3, 0x808037F4, 40),
        9 => (0x80802337, 0x808037F3, 16),
        10 => (0x80802BF6, 0x808037F1, 8),
        // Rise of Iron steering keeps the selector 2 base programs and adds a curve. Its fields
        // over the first 108 bytes match Shadowkeep selector 17, `808037E9`.
        16 if old && p.u32(parameter - 4)? == 0x808010EF => {
            let common =
                p.0.get(parameter..parameter + STEERING)
                    .context("Projectile steering parameter extent")?
                    .to_vec();
            // Bytes `+00..+18` are the response magnitude prefix shared with selector 2, such as
            // the count increment of 1 that Truth's steering holds. The operand member at `+18`
            // and the one at `+D0` must be empty.
            ensure!(
                p.u64(parameter + 0x18)? == 0 && p.u64(parameter + 0xD0)? == 0,
                "Projectile steering has a resource or index operand"
            );
            let expressions = STEERING_PROGRAMS
                .into_iter()
                .map(|off| program(p, parameter + off))
                .collect::<Result<_>>()?;
            return Ok(Action {
                selector,
                parameter,
                common,
                resource: None,
                expressions,
                indices: Vec::new(),
                filter: None,
                filter_at: 0,
                known: true,
            });
        }
        // Shadowkeep steering keeps the same fields over the first 108 bytes, then a tail that
        // a kept parameter must hold at its defaults.
        17 if !old && p.u32(parameter - 4)? == 0x808037E9 => {
            let common =
                p.0.get(parameter..parameter + STEERING)
                    .context("Projectile steering parameter extent")?
                    .to_vec();
            ensure!(
                p.u64(parameter + 0x18)? == 0
                    && p.u64(parameter + 0xD0)? == 0
                    && p.u64(parameter + 0x108)? == 0
                    && p.u32(parameter + 0x110)? == u32::MAX
                    && p.u32(parameter + 0x114)? == 0
                    && p.u32(parameter + 0x118)? == 0
                    && p.u32(parameter + 0x11C)? == 0x811C9DC5,
                "Native projectile steering has a resource, operand or nondefault tail"
            );
            let expressions = STEERING_PROGRAMS
                .into_iter()
                .map(|off| program(p, parameter + off))
                .collect::<Result<_>>()?;
            return Ok(Action {
                selector,
                parameter,
                common,
                resource: None,
                expressions,
                indices: Vec::new(),
                filter: None,
                filter_at: 0,
                known: true,
            });
        }
        0 | 1 | 8 => return kept(p, parameter, selector, old),
        13 | 15 if old => return kept(p, parameter, selector, old),
        14 | 16 if !old => return kept(p, parameter, selector, old),
        _ => {
            return Ok(Action {
                selector,
                parameter,
                common: Vec::new(),
                resource: None,
                expressions: Vec::new(),
                indices: Vec::new(),
                filter: None,
                filter_at: 0,
                known: false,
            });
        }
    };
    ensure!(
        p.u32(parameter - 4)? == if old { source } else { native },
        "Projectile response parameter class differs"
    );
    let mut common =
        p.0.get(parameter..parameter + size)
            .context("Projectile response parameter extent")?
            .to_vec();
    let resource = if matches!(selector, 4 | 9) {
        let offset = if selector == 4 { 16 } else { 8 };
        let tag = p.u32(parameter + offset)?;
        common[offset..offset + 4].fill(0);
        Some(tag)
    } else {
        None
    };
    if selector == 10 {
        p.f32(parameter + 4)?;
        if !old {
            ensure!(
                p.bytes::<16>(parameter + 8)? == [0; 16]
                    && p.u32(parameter + 24)? == u32::MAX
                    && p.u32(parameter + 28)? == 0,
                "Native contact response has nondefault additional parameters"
            );
        }
    }
    let expressions = if selector == 2 {
        [0x20, 0x68, 0xA0]
            .into_iter()
            .map(|off| program(p, parameter + off))
            .collect::<Result<_>>()?
    } else {
        Vec::new()
    };
    Ok(Action {
        selector,
        parameter,
        common,
        resource,
        expressions,
        indices: Vec::new(),
        filter: None,
        filter_at: 0,
        known: true,
    })
}

/// Selectors whose carrier parameter is kept when it does what the source's does, compared field
/// by field because the layouts differ.
///
/// | Selector | Rise of Iron | Shadowkeep | Compared |
/// | --- | --- | --- | --- |
/// | 0 | `8080251C` | `808037E1` | Tag `+10`, root scalar indices `+20` and `+30` through the pairing |
/// | 1 | `80802395` | `808037E2` | Program of the object at relative `+00`, `80802095` or `808037E5`, at its `+08` |
/// | 8 | `808022DB` | `808037F2` | Attached graph `+08`, filter at `+10` in both |
/// | 13, 14 | `8080320B` | `808037D3` | Header `+10..+30`, then one row: filter at `+08`, graph at source `+F0` and native `+78` |
/// | 15, 16 | `808030B3` | `808037DB` | Spawned graph `+08`, filter at source `+10` and native `+18` |
///
/// Selector 8 attaches its graph to the first entity in the hit chain the filter accepts, and
/// Rise of Iron 15 spawns one at the contact. Rise of Iron 13 holds a list of filtered graphs in
/// `808030FB` rows of `F8` bytes, Shadowkeep 14 in `808037D8` rows of `80` bytes. Both source follow-ups at `+E8/+F0` must be absent,
/// as must the native gate byte `+10`, transform `+78` and follow-ups `+80/+88`.
fn kept(p: &Payload, parameter: usize, selector: u8, old: bool) -> Result<Action> {
    let class = p.u32(parameter - 4)?;
    let mut action = Action {
        selector,
        parameter,
        common: Vec::new(),
        resource: None,
        expressions: Vec::new(),
        indices: Vec::new(),
        filter: None,
        filter_at: 0,
        known: true,
    };
    match selector {
        0 => {
            ensure!(
                class == if old { 0x8080251C } else { 0x808037E1 },
                "Projectile response parameter class differs"
            );
            // Two operands, each an empty member followed by a root scalar index, FFFFFFFF for
            // none. A nonempty member would hold an inline expression instead.
            ensure!(
                p.u64(parameter + 0x18)? == 0 && p.u64(parameter + 0x28)? == 0,
                "Projectile response operand holds an inline expression"
            );
            let mut common =
                p.0.get(parameter..parameter + 0x38)
                    .context("Response parameter extent")?
                    .to_vec();
            for at in [0x20, 0x30] {
                action.indices.push(p.u32(parameter + at)?);
                common[at..at + 4].fill(0);
            }
            action.common = common;
        }
        1 => {
            ensure!(
                class == if old { 0x80802395 } else { 0x808037E2 },
                "Projectile response parameter class differs"
            );
            let object = p.pointer(parameter)?;
            ensure!(
                p.u32(object - 4)? == if old { 0x80802095 } else { 0x808037E5 }
                    && p.u64(object)? == 0,
                "Projectile response expression object differs"
            );
            action.expressions.push(program(p, object + 8)?);
        }
        8 => {
            ensure!(
                class == if old { 0x808022DB } else { 0x808037F2 },
                "Projectile response parameter class differs"
            );
            action.resource = Some(p.u32(parameter + 8)?);
            action.filter = Some(if old {
                Filter::source(p, parameter + 0x10)?
            } else {
                Filter::native(p, parameter + 0x10)?
            });
            action.filter_at = parameter + 0x10;
        }
        13 | 14 => {
            ensure!(
                class == if old { 0x8080320B } else { 0x808037D3 },
                "Projectile response parameter class differs"
            );
            let (row_class, stride, graph) = if old {
                (0x808030FB, 0xF8, 0xE8)
            } else {
                (0x808037D8, 0x80, 0x70)
            };
            let rows = p.array(parameter, stride, Some(row_class))?;
            let [row] = rows.as_slice() else {
                anyhow::bail!("Projectile filtered attachment has another row count");
            };
            // Rise of Iron rows hold 1.0 at `+DC`, which Shadowkeep rows have no field for, as
            // Zhalo Supercell's does. Shadowkeep rows hold zero at `+68`. The graph is named by
            // tag alone.
            let qualified = if old {
                p.f32(row + 0xDC)? == 1.0 && p.u64(row + 0xE0)? == 0
            } else {
                p.u64(row + 0x68)? == 0
            };
            ensure!(
                qualified && p.u64(row + graph)? == 0,
                "Projectile filtered attachment row has unqualified fields"
            );
            action.resource = Some(p.u32(row + graph + 8)?);
            action.filter = Some(if old {
                Filter::source(p, row + 8)?
            } else {
                Filter::native(p, row + 8)?
            });
            action.filter_at = row + 8;
            let mut common =
                p.0.get(parameter + 0x10..parameter + 0x30)
                    .context("Projectile filtered attachment header")?
                    .to_vec();
            common.extend_from_slice(&p.bytes::<8>(*row)?);
            action.common = common;
        }
        _ => {
            ensure!(
                class == if old { 0x808030B3 } else { 0x808037DB },
                "Projectile response parameter class differs"
            );
            action.resource = Some(p.u32(parameter + 8)?);
            if old {
                ensure!(
                    p.u64(parameter + 0xE8)? == 0 && p.u64(parameter + 0xF0)? == 0,
                    "Projectile spawn response has follow-up records"
                );
                action.filter = Some(Filter::source(p, parameter + 0x10)?);
                action.filter_at = parameter + 0x10;
            } else {
                ensure!(
                    p.u64(parameter + 0x10)? == 0
                        && p.u64(parameter + 0x78)? == 0
                        && p.u64(parameter + 0x80)? == 0
                        && p.u64(parameter + 0x88)? == 0,
                    "Native spawn response has a gate, transform or follow-up"
                );
                action.filter = Some(Filter::native(p, parameter + 0x18)?);
                action.filter_at = parameter + 0x18;
            }
        }
    }
    Ok(action)
}

struct Node {
    at: usize,
    name: u32,
    mask: u64,
    expression: Expression,
    actions: Vec<Action>,
    /// The label filter every object a node responds to must pass, at source `+40` and native
    /// `+30`, read for qualified groups.
    filter: Option<Filter>,
    filter_at: usize,
}
struct Group {
    at: usize,
    name: u32,
    scalar: u32,
    nodes: Vec<Node>,
}

impl Group {
    fn read(
        p: &Payload,
        at: usize,
        old: bool,
        ordinal: usize,
        inputs: &mut BTreeMap<usize, String>,
        qualify: bool,
    ) -> Result<Self> {
        let scalar = p.u32(at + if old { 0x48 } else { 0x40 })?;
        let mut nodes = Vec::new();
        let mut names = BTreeSet::new();
        for node in p.array(
            at + if old { 0x50 } else { 0x48 },
            if old { 472 } else { 392 },
            Some(if old { 0x8080274D } else { 0x808037D0 }),
        )? {
            let node_name = p.u32(node + if old { 24 } else { 16 })?;
            ensure!(
                names.insert(node_name),
                "Ambiguous projectile response node"
            );
            let expr = expression(
                p,
                node + if old { 0x158 } else { 0xF0 },
                old,
                &format!("group/{ordinal}/node/{node_name:08X}"),
                inputs,
            )?;
            // A left-out carrier group keeps its inputs for the connection checks, but its
            // actions are neither translated nor retained.
            let actions = if qualify {
                p.array(node + if old { 0x1C8 } else { 0x178 }, 16, None)?
                    .into_iter()
                    .map(|r| action(p, r, old))
                    .collect::<Result<Vec<_>>>()?
            } else {
                Vec::new()
            };
            ensure!(
                !qualify || !actions.is_empty(),
                "Empty projectile response node"
            );
            let filter_at = node + if old { 0x40 } else { 0x30 };
            let filter = if !qualify {
                None
            } else if old {
                Some(Filter::source(p, filter_at)?)
            } else {
                Some(Filter::native(p, filter_at)?)
            };
            nodes.push(Node {
                at: node,
                name: node_name,
                mask: p.u64(node + if old { 0x38 } else { 0x28 })?,
                expression: expr,
                actions,
                filter,
                filter_at,
            });
        }
        // A group may hold no nodes, as rocket group `984AF7B1` does in both versions.
        Ok(Self {
            at,
            name: p.u32(at + if old { 0x18 } else { 0x10 })?,
            scalar,
            nodes,
        })
    }
}

pub(super) type Resources = BTreeSet<(u8, u32, u32)>;

pub(super) struct Translation {
    pub patches: Vec<Value>,
    pub scalar_patches: Vec<Value>,
    pub resources: Resources,
    pub plan: edit::Plan,
    pub created: BTreeSet<String>,
    /// Source child creation parameters the carrier lacks, by namespace and name. The caller
    /// leaves them out only when nothing the source movement creates can read them.
    pub omitted: Vec<(u32, u32)>,
    /// Native graphs and profiles paired with source ones by kept actions. One source profile can
    /// pair with different carrier profiles at different positions.
    pub graphs: Counterparts,
    pub report: Value,
}

/// Carrier resources each source resource pairs with through kept actions.
pub(super) type Counterparts = BTreeMap<u32, BTreeSet<u32>>;

/// The one carrier resource a source resource pairs with, if it is unambiguous.
pub(super) fn unique(counterparts: &Counterparts, tag: u32) -> Option<u32> {
    let found = counterparts.get(&tag)?;
    if found.len() == 1 {
        found.first().copied()
    } else {
        None
    }
}

/// A program that forwards its one named input.
fn is_forward(e: &Expression) -> bool {
    e.code == [0x3C, 1, 0x3E, 0]
        && e.constants.is_empty()
        && e.counts == [2, 1]
        && e.names.len() == 1
}

/// Shadowkeep contact response `808037F1` from a Rise of Iron `80802BF6` body: count increment and
/// speed, then the native defaults the carrier's own parameters hold.
fn contact(common: &[u8]) -> Vec<u8> {
    let mut bytes = common.to_vec();
    bytes.extend([0; 16]);
    bytes.extend(u32::MAX.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes
}

pub(super) struct Responses {
    groups: Vec<Group>,
    scalars: Vec<Expression>,
    /// Where each scalar's constant rows begin, for writing source values.
    scalar_constants: Vec<usize>,
    scalar_rows: Vec<usize>,
    /// Source definition +138 or carrier +128, the computed scalar.
    computed: u32,
    /// The contact and bounce count limits, source definition `+8C` and `+8D` or carrier `+78`
    /// and `+79`.
    limits: [u8; 2],
    signals: BTreeMap<(u32, u32), Expression>,
    pub inputs: BTreeMap<usize, String>,
}

impl Responses {
    /// Graphs the source's selector 4, 8 and 15 responses create.
    pub fn spawned(&self) -> BTreeSet<u32> {
        self.groups
            .iter()
            .flat_map(|group| &group.nodes)
            .flat_map(|node| &node.actions)
            .filter(|action| matches!(action.selector, 4 | 8 | 13 | 15))
            .filter_map(|action| action.resource)
            .filter(|&tag| !matches!(tag, 0 | u32::MAX))
            .collect()
    }

    pub fn read(owner: &graph::Owner, old: bool, dropped: &[usize]) -> Result<Self> {
        let p = &owner.payload;
        let d = owner.definition;
        let mut inputs = BTreeMap::new();
        let signals = read_signals(owner, old, &mut inputs)?;
        let scalars = p
            .array(
                d + if old { 0x230 } else { 0x228 },
                if old { 88 } else { 80 },
                Some(if old { 0x808005A0 } else { 0x808089F8 }),
            )?
            .into_iter()
            .enumerate()
            .map(|(i, r)| expression(p, r, old, &format!("scalar/{i}"), &mut inputs))
            .collect::<Result<Vec<_>>>()?;
        let scalar_rows = p.array(
            d + if old { 0x230 } else { 0x228 },
            if old { 88 } else { 80 },
            None,
        )?;
        let scalar_constants = scalar_rows
            .iter()
            .copied()
            .map(|row| {
                let rows = p.array(row + if old { 24 } else { 16 } + 16, 16, Some(0x80800090))?;
                Ok(rows.first().copied().unwrap_or(usize::MAX))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut groups = Vec::new();
        for (ordinal, g) in p
            .array(
                d + if old { 0x220 } else { 0x218 },
                if old { 96 } else { 88 },
                Some(if old { 0x8080239D } else { 0x808037CD }),
            )?
            .into_iter()
            .enumerate()
        {
            // Material qualification owns group identity, including unnamed source groups.
            let group = Group::read(p, g, old, ordinal, &mut inputs, !dropped.contains(&ordinal))?;
            ensure!(
                group.scalar == u32::MAX || (group.scalar as usize) < scalars.len(),
                "Projectile scalar index exceeds its array"
            );
            groups.push(group);
        }
        ensure!(
            !groups.is_empty() && !scalars.is_empty(),
            "Empty projectile response tree"
        );
        Ok(Self {
            groups,
            scalars,
            scalar_constants,
            scalar_rows,
            computed: p.u32(d + if old { 0x138 } else { 0x128 })?,
            limits: [
                p.u8(d + if old { 0x8C } else { 0x78 })?,
                p.u8(d + if old { 0x8D } else { 0x79 })?,
            ],
            signals,
            inputs,
        })
    }

    /// Translate the carrier tree to this source tree. Returns response operand patches, root
    /// scalar constant patches, paired resources, the structural edits for the private movement,
    /// the source input paths those edits create, and a report.
    #[allow(clippy::cognitive_complexity)]
    /// Profiles the tree's selector 9 responses name, in tree order.
    pub fn profiles(&self) -> Vec<u32> {
        self.groups
            .iter()
            .flat_map(|group| &group.nodes)
            .flat_map(|node| &node.actions)
            .filter(|action| action.selector == 9)
            .filter_map(|action| action.resource)
            .filter(|&tag| !matches!(tag, 0 | u32::MAX))
            .collect()
    }

    /// Translate the source tree onto the carrier's. `fallback` names, for a source profile, the
    /// carrier profile a created response takes when nothing in the tree pairs it. `labels`
    /// compiles the source filter at an offset for the native registry.
    #[allow(clippy::type_complexity)]
    pub fn translate(
        &self,
        native: &mut Self,
        owner: &graph::Owner,
        groups: &align::Groups,
        (resource, fallback): (usize, &BTreeMap<u32, u32>),
        labels: &dyn Fn(usize) -> Result<([Vec<u32>; 4], u8, Option<(u32, Vec<u8>)>)>,
    ) -> Result<Translation> {
        let unnamed = 0x811C9DC5;
        ensure!(
            groups.paired.len() == self.groups.len(),
            "Projectile response group pairing differs"
        );
        let ResponseWitness {
            kept_operands,
            graphs,
            seeds,
        } = response_witness(self, native, owner, groups)?;
        let ScalarPairing {
            map,
            converted,
            retargeted,
            appended,
        } = pair_scalars(self, native, &seeds, &kept_operands)?;
        let mapped = |scalar| mapped_scalar(&map, scalar);
        let mut plan = edit::Plan::default();
        let mut created = BTreeSet::new();
        let scalar_patches = plan_scalars(
            self,
            native,
            resource,
            (&map, &converted, &retargeted, appended),
            &mut plan,
            &mut created,
        )?;
        let omitted = compare_signals(self, native, unnamed)?;
        let mut patches = Vec::new();
        let mut resources = BTreeSet::new();
        let mut node_names = BTreeMap::new();
        let create = |action: &Action| native_parameter(action, (&graphs, fallback), &mapped);
        for (index, (s, n)) in self.groups.iter().zip(&groups.paired).enumerate() {
            let Some(carrier_index) = *n else {
                let group = inserted(s, index, mapped(s.scalar)?, &create)?;
                for (node, new) in s.nodes.iter().zip(&group.nodes) {
                    if new.input.is_some() {
                        created.insert(format!("group/{index}/node/{:08X}/input/0", node.name));
                    }
                }
                plan.inserted.push(group);
                continue;
            };
            let n = &native.groups[carrier_index];
            let gate = mapped(s.scalar)?;
            if gate != n.scalar {
                plan.words.push((n.at + 0x40, gate));
            }
            if s.name != unnamed && s.name != n.name {
                plan.words.push((n.at + 0x10, s.name));
            }
            // Source nodes the carrier group lacks are inserted at their source positions.
            let mut new_nodes = Vec::new();
            for (position, (sn, found)) in s
                .nodes
                .iter()
                .zip(node_pairs(&s.nodes, &n.nodes)?)
                .enumerate()
            {
                let Some(found) = found else {
                    let node = new_node(sn, &create)?;
                    if node.input.is_some() {
                        created.insert(format!("group/{index}/node/{:08X}/input/0", sn.name));
                    }
                    new_nodes.push((position, node));
                    continue;
                };
                let nn = &n.nodes[found];
                node_names.insert((n.at, nn.name), sn.name);
                // A paired node takes the source's label filter. Thorn's `C8D5DC97` has none where
                // carrier `D86C086E`, whose place it takes, matches any of three labels.
                if sn.filter != nn.filter {
                    let (lists, mode, record) = labels(sn.filter_at)?;
                    plan.labels.push(edit::Relabel {
                        at: nn.filter_at,
                        lists,
                        mode,
                        record,
                    });
                }
                if sn.name != nn.name {
                    plan.words.push((nn.at + 0x10, sn.name));
                }
                if sn.mask != nn.mask {
                    plan.masks.push((nn.at + 0x28, sn.mask));
                }
                if sn.expression != nn.expression {
                    ensure!(
                        forwards_input(&sn.expression)
                            && nn.expression.names.is_empty()
                            && nn.expression.code.len() >= sn.expression.code.len(),
                        "Projectile response destination or program differs"
                    );
                    plan.expressions
                        .push((nn.at + 0xF0, sn.expression.names[0]));
                    created.insert(format!("group/{index}/node/{:08X}/input/0", sn.name));
                }
                translate_node_actions(
                    sn,
                    nn,
                    (resource, labels, &mapped, &create),
                    &mut plan,
                    (&mut patches, &mut resources),
                )?;
            }
            if !new_nodes.is_empty() {
                plan.nodes.push((carrier_index, new_nodes));
            }
        }
        // Carrier definition +128 names the computed scalar that source +138 names.
        let computed = mapped(self.computed)?;
        if computed != u32::MAX && native.computed != computed {
            plan.words.push((owner.definition + 0x128, computed));
        }
        // The contact and bounce limits bound the counts the responses add, and those increments
        // are the source's. Instance `+16C` and `+170` copy the limits. A bounce is refused once
        // its count reaches the limit, and a count past it finishes the projectile, which the
        // carrier's own increments never reach. Sleeper Simulant counts 3 per bounce against 6
        // where its carrier counts 1 against 2.
        if self.limits != native.limits {
            let at = owner.definition + 0x78;
            let [_, _, third, fourth] = owner.payload.u32(at)?.to_le_bytes();
            plan.words.push((
                at,
                u32::from_le_bytes([self.limits[0], self.limits[1], third, fourth]),
            ));
        }
        rebind_input_paths(native, groups, &map, &node_names, &retargeted)?;
        let report = json!({"dropped_groups":groups.dropped,"inserted_groups":plan.inserted.iter().map(|group| group.index).collect::<Vec<_>>(),
            "converted_scalars":converted.iter().map(|c| c.2).collect::<Vec<_>>(),
            "retargeted_scalars":retargeted.iter().map(|r| json!({"source":r.2,"carrier":r.3,"name":r.1})).collect::<Vec<_>>(),
            "omitted_child_parameters":omitted.iter().map(|(namespace,name)| json!({"namespace":namespace,"name":name})).collect::<Vec<_>>(),
            "scalar_map":map,"created_inputs":created,"edits":plan.report()});
        Ok(Translation {
            patches,
            scalar_patches,
            resources,
            plan,
            created,
            omitted,
            graphs,
            report,
        })
    }

    pub fn additional_inputs(&self, native: &Self) -> BTreeSet<String> {
        native
            .signals
            .keys()
            .filter(|key| !self.signals.contains_key(key))
            .map(|(namespace, name)| format!("child/{namespace:08X}/{name:08X}/input/0"))
            .collect()
    }

    pub fn report(&self) -> Value {
        json!({"groups":self.groups.len(),"nodes":self.groups.iter().map(|g|g.nodes.len()).sum::<usize>(),
            "actions":self.groups.iter().flat_map(|g|&g.nodes).map(|n|n.actions.len()).sum::<usize>(),
            "named_inputs":self.inputs.len(),"signals":self.signals.len()})
    }
}

/// Pair a group's source nodes with the carrier's. Equal counts pair by position, which renames.
/// Otherwise every carrier node pairs by name in order and the source nodes left over are inserted.
/// Scout carrier `80BBC8B2` group `6E6CAB3B` lacks the source's middle node `003F6E7E`.
/// Pair every carrier node with a source node in order. Equal counts pair by position. Otherwise
/// carrier nodes pair by name, or failing that by name or by holding: a source node holds a carrier
/// node when it has each of the carrier node's actions, as Thorn's `C8D5DC97` with actions 0, 10,
/// 9 and 8 holds carrier `D86C086E` with 10, 8 and 9 and takes its place. Unpaired source nodes are
/// inserted.
fn node_pairs(source: &[Node], carrier: &[Node]) -> Result<Vec<Option<usize>>> {
    if source.len() == carrier.len() {
        return Ok((0..source.len()).map(Some).collect());
    }
    let ordered = |fits: &dyn Fn(&Node, &Node) -> bool| {
        let mut next = 0;
        let pairs = source
            .iter()
            .map(|node| {
                let found = carrier.get(next).is_some_and(|c| fits(node, c));
                found.then(|| {
                    next += 1;
                    next - 1
                })
            })
            .collect::<Vec<_>>();
        (next == carrier.len()).then_some(pairs)
    };
    let holds = |node: &Node, carrier: &Node| {
        let mut selectors = node
            .actions
            .iter()
            .map(|action| native_selector(action.selector, true))
            .collect::<Vec<_>>();
        carrier.actions.iter().all(|action| {
            selectors
                .iter()
                .position(|&selector| selector == action.selector)
                .map(|at| selectors.remove(at))
                .is_some()
        })
    };
    ordered(&|node, carrier| node.name == carrier.name)
        .or_else(|| ordered(&|node, carrier| node.name == carrier.name || holds(node, carrier)))
        .context("Projectile response group differs")
}

/// A new native parameter from a source action. Only bodies whose references have a native
/// counterpart are created: a spawn's graph or a profile response's profile is the one its source
/// resource pairs with elsewhere in the tree.
fn native_parameter(
    action: &Action,
    (graphs, fallback): (&Counterparts, &BTreeMap<u32, u32>),
    mapped: &dyn Fn(u32) -> Result<u32>,
) -> Result<edit::Act> {
    Ok(match action.selector {
        // A scalar response keeps its `38`-byte body, the two root scalar indices at `+20` and
        // `+30` renumbered. Its tag at `+10` must be absent.
        0 if action.known => {
            ensure!(
                action.common.len() == 0x38
                    && action.common[0x10..0x14] == u32::MAX.to_le_bytes()
                    && action.indices.len() == 2,
                "Created projectile scalar response names a resource"
            );
            let mut bytes = action.common.clone();
            for (at, &index) in [0x20, 0x30].into_iter().zip(&action.indices) {
                bytes[at..at + 4].copy_from_slice(&mapped(index)?.to_le_bytes());
            }
            edit::Act::New {
                selector: 0,
                class: 0x808037E1,
                bytes,
                arrays: Vec::new(),
                spawns: Vec::new(),
            }
        }
        // An expression response: a reference at `+00` to the `808037E5` object that follows at
        // `+18`, its class word at `+14`. The object holds an empty member and the source program,
        // code at object `+08`, constants at `+18` and counts at `+28`.
        1 if action.known => {
            let program = action
                .expressions
                .first()
                .context("Created projectile expression response has no program")?;
            let mut bytes = vec![0u8; 0x50];
            bytes[..8].copy_from_slice(&0x18i64.to_le_bytes());
            bytes[0x14..0x18].copy_from_slice(&0x808037E5u32.to_le_bytes());
            bytes[0x40..0x48].copy_from_slice(&program.counts[0].to_le_bytes());
            bytes[0x48..0x50].copy_from_slice(&program.counts[1].to_le_bytes());
            edit::Act::New {
                selector: 1,
                class: 0x808037E2,
                bytes,
                arrays: program_arrays(&[0x20], std::slice::from_ref(program))?,
                spawns: Vec::new(),
            }
        }
        // Bounce bodies share their `D0`-byte layout, so the source body is kept with its three
        // programs rebuilt in the native payload. Its `+00` bounce count increment stays the
        // source's, as the three of Sleeper Simulant bounce group `F13988D3`.
        2 if action.known => {
            ensure!(
                action.common.len() == 0xD0 && action.expressions.len() == 3,
                "Projectile bounce extent differs"
            );
            edit::Act::New {
                selector: 2,
                class: 0x808037EA,
                bytes: action.common.clone(),
                arrays: program_arrays(&[0x20, 0x68, 0xA0], &action.expressions)?,
                spawns: Vec::new(),
            }
        }
        3 if action.known && action.common == [0] => edit::Act::New {
            selector: 3,
            class: 0x808037F6,
            bytes: vec![0],
            arrays: Vec::new(),
            spawns: Vec::new(),
        },
        // A source graph without a counterpart is translated, as hand cannon spawn `80A6A859` is.
        4 if action.known => {
            let tag = action
                .resource
                .filter(|&tag| !matches!(tag, 0 | u32::MAX))
                .context("Created projectile spawn names no graph")?;
            let mut bytes = action.common.clone();
            let mut spawns = Vec::new();
            match unique(graphs, tag) {
                Some(graph) => bytes[16..20].copy_from_slice(&graph.to_le_bytes()),
                None => {
                    bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
                    spawns.push((16, tag));
                }
            }
            let index = u32::from_le_bytes(bytes[0x20..0x24].try_into()?);
            bytes[0x20..0x24].copy_from_slice(&mapped(index)?.to_le_bytes());
            edit::Act::New {
                selector: 4,
                class: 0x808037F4,
                bytes,
                arrays: Vec::new(),
                spawns,
            }
        }
        9 if action.known => {
            let profile = match action.resource {
                Some(u32::MAX) => u32::MAX,
                tag => tag
                    .and_then(|tag| unique(graphs, tag).or_else(|| fallback.get(&tag).copied()))
                    .context("Created projectile profile response has no native counterpart")?,
            };
            let mut bytes = action.common.clone();
            bytes[8..12].copy_from_slice(&profile.to_le_bytes());
            edit::Act::New {
                selector: 9,
                class: 0x808037F3,
                bytes,
                arrays: Vec::new(),
                spawns: Vec::new(),
            }
        }
        10 if action.known => edit::Act::New {
            selector: 10,
            class: 0x808037F1,
            bytes: contact(&action.common),
            arrays: Vec::new(),
            spawns: Vec::new(),
        },
        16 if action.known => steering(action)?,
        _ => anyhow::bail!(
            "Projectile response action {} needs a native parameter",
            action.selector
        ),
    })
}

/// Describe a source node for insertion. It keeps the source name, mask and program, which may
/// forward one named input, as scout node `003F6E7E` forwards `8A0D4131`. Its actions are created.
fn new_node(
    node: &Node,
    create: &dyn Fn(&Action) -> Result<edit::Act>,
) -> Result<edit::InsertedNode> {
    ensure!(
        node.expression.names.is_empty() || is_forward(&node.expression),
        "Inserted projectile response node reads an input"
    );
    // Inserted nodes copy a carrier node with an empty label filter.
    ensure!(
        node.filter.as_ref().is_some_and(Filter::is_empty),
        "Inserted projectile response node has a label filter"
    );
    Ok(edit::InsertedNode {
        input: node.expression.names.first().copied(),
        name: node.name,
        mask: node.mask,
        code: node.expression.code.clone(),
        constants: node.expression.constants.concat(),
        counts: node.expression.counts,
        actions: node
            .actions
            .iter()
            .map(create)
            .collect::<Result<Vec<_>>>()?,
    })
}

/// Describe a source group the carrier lacks for insertion, with each node as [`new_node`] does.
fn inserted(
    group: &Group,
    index: usize,
    scalar: u32,
    create: &dyn Fn(&Action) -> Result<edit::Act>,
) -> Result<edit::Inserted> {
    let nodes = group
        .nodes
        .iter()
        .map(|node| new_node(node, create))
        .collect::<Result<Vec<_>>>()?;
    Ok(edit::Inserted {
        index,
        name: group.name,
        scalar,
        nodes,
    })
}

/// Bounce or steering fields without the program array descriptors, whose relative offsets differ
/// between copies. The programs compare separately. Counts and every other field compare.
fn program_view(common: &[u8]) -> Vec<u8> {
    let mut view = common.to_vec();
    for off in STEERING_PROGRAMS {
        if let Some(region) = view.get_mut(off..off + 32) {
            region.fill(0);
        }
    }
    view
}

/// Shadowkeep selector 17 from Rise of Iron steering: the same fields over the first 108 bytes,
/// programs rebuilt in the native payload, and the default native tail. The tail leaves the
/// effect override absent, recipient updates off and the event packet unnamed.
fn steering(action: &Action) -> Result<edit::Act> {
    let mut bytes = action.common.clone();
    ensure!(
        bytes.len() == STEERING,
        "Projectile steering extent differs"
    );
    bytes.extend(0u64.to_le_bytes());
    bytes.extend(u32::MAX.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend([0u8; 4]);
    bytes.extend(0x811C9DC5u32.to_le_bytes());
    Ok(edit::Act::New {
        selector: 17,
        class: 0x808037E9,
        bytes,
        arrays: program_arrays(&STEERING_PROGRAMS, &action.expressions)?,
        spawns: Vec::new(),
    })
}

/// Code and constant arrays for embedded programs at the given parameter offsets. A created
/// program cannot read a named input, which the parameter has no slot list for.
#[allow(clippy::type_complexity)]
fn program_arrays(
    offsets: &[usize],
    programs: &[Expression],
) -> Result<Vec<(usize, u32, Vec<u8>, usize)>> {
    let mut arrays = Vec::new();
    for (&off, program) in offsets.iter().zip(programs) {
        ensure!(
            program.names.is_empty(),
            "Created projectile response program reads an input"
        );
        arrays.push((off, 0x80800009, program.code.clone(), 1));
        arrays.push((off + 16, 0x80800090, program.constants.concat(), 16));
    }
    Ok(arrays)
}

fn read_signals(
    owner: &graph::Owner,
    old: bool,
    inputs: &mut BTreeMap<usize, String>,
) -> Result<BTreeMap<(u32, u32), Expression>> {
    let p = &owner.payload;
    let d = owner.definition;
    let signals = p
        .array(
            d + if old { 0x1A0 } else { 0x198 },
            if old { 136 } else { 120 },
            Some(if old { 0x808008F1 } else { 0x8080851C }),
        )?
        .into_iter()
        .map(|row| {
            let instance = usize::try_from(p.u64(row + 8)?)?;
            ensure!(
                p.u32(row)? == owner.tag
                    && p.u32(instance)? == owner.tag
                    && p.u64(instance + 8)? == row as u64
                    && p.u32(instance + if old { 16 } else { 4 })?
                        == if old { 0x808008F1 } else { 0x8080851C }
                    && p.u32(row + if old { 16 } else { 4 })?
                        == if old { 0x8080098A } else { 0x8080851B },
                "Projectile signal pair differs"
            );
            let namespace = p.u32(row + if old { 120 } else { 104 })?;
            let name = p.u32(row + if old { 128 } else { 112 })?;
            let expression = expression(
                p,
                row + if old { 24 } else { 16 },
                old,
                &format!("child/{namespace:08X}/{name:08X}"),
                inputs,
            )?;
            Ok(((namespace, name), expression))
        })
        .collect::<Result<Vec<_>>>()?;
    let count = signals.len();
    let signals = signals.into_iter().collect::<BTreeMap<_, _>>();
    ensure!(
        signals.len() == count,
        "Projectile child parameter outputs are ambiguous"
    );
    Ok(signals)
}

fn action_pairs(source: &[Action], carrier: &[Action]) -> Vec<Option<usize>> {
    // Carrier actions each source action keeps, found by selector in order. Rise of Iron
    // selectors 15 and 16 are 16 and 17 in Shadowkeep.
    let mut used = vec![false; carrier.len()];
    source
        .iter()
        .map(|action| {
            let selector = native_selector(action.selector, true);
            let found = carrier
                .iter()
                .enumerate()
                .find(|(index, candidate)| !used[*index] && candidate.selector == selector)
                .map(|(index, _)| index);
            if let Some(index) = found {
                used[index] = true;
            }
            found
        })
        .collect()
}

struct ResponseWitness {
    kept_operands: BTreeSet<u32>,
    graphs: Counterparts,
    seeds: BTreeMap<u32, u32>,
}

fn response_witness(
    source: &Responses,
    native: &Responses,
    owner: &graph::Owner,
    groups: &align::Groups,
) -> Result<ResponseWitness> {
    // Scalar operands the private movement keeps from the carrier.
    let mut kept_operands = BTreeSet::new();
    for at in [0xC0, 0x138, 0x148, 0x158] {
        kept_operands.insert(owner.payload.u32(owner.definition + at)?);
    }
    // Native graphs and profiles paired with source ones by kept spawn, attachment and profile
    // actions. A created action takes the native counterpart its source resource already
    // pairs with in this tree.
    let mut graphs = Counterparts::new();
    // Source and carrier operands of kept scalar responses, which pair those scalars first.
    let mut seeds = BTreeMap::<u32, u32>::new();
    for (s, n) in source.groups.iter().zip(&groups.paired) {
        let Some(n) = n else { continue };
        let carrier = &native.groups[*n].nodes;
        for (sn, found) in s.nodes.iter().zip(node_pairs(&s.nodes, carrier)?) {
            let Some(found) = found else { continue };
            let nn = &carrier[found];
            for (sa, index) in sn
                .actions
                .iter()
                .zip(action_pairs(&sn.actions, &nn.actions))
            {
                let Some(index) = index else { continue };
                let action = &nn.actions[index];
                if action.selector == 4 {
                    kept_operands.insert(owner.payload.u32(action.parameter + 0x20)?);
                }
                kept_operands.extend(action.indices.iter().copied());
                for (&from, &to) in sa.indices.iter().zip(&action.indices) {
                    if from != u32::MAX && to != u32::MAX {
                        seeds.entry(from).or_insert(to);
                    }
                }
                if let (4 | 8 | 9 | 13 | 15, Some(from), Some(to)) =
                    (sa.selector, sa.resource, action.resource)
                    && !matches!(from, 0 | u32::MAX)
                {
                    graphs.entry(from).or_default().insert(to);
                }
            }
        }
    }
    Ok(ResponseWitness {
        kept_operands,
        graphs,
        seeds,
    })
}

struct ScalarPairing {
    map: Vec<Option<usize>>,
    converted: Vec<(usize, u32, usize)>,
    retargeted: Vec<RetargetedScalar>,
    appended: Vec<(edit::Appended, usize)>,
}

type RetargetedScalar = (usize, u32, usize, usize);

fn forwards_input(e: &Expression) -> bool {
    e.code == [0x3C, 1, 0x3E, 0]
        && e.constants.is_empty()
        && e.counts == [2, 1]
        && e.names.len() == 1
}

fn pair_scalars(
    source: &Responses,
    native: &Responses,
    seeds: &BTreeMap<u32, u32>,
    kept_operands: &BTreeSet<u32>,
) -> Result<ScalarPairing> {
    // Root scalars pair by program and inputs, in order where they can and otherwise with any
    // carrier scalar left. A source scalar forwarding one named input takes the place of a
    // carrier scalar that nothing kept reads. A constant becomes a forward, as fusion carrier
    // `81529502` scalar 0 is read only by native-only selector 4 actions. A forward's input
    // takes the source name, as automatic rifle carrier `80BBC7E6` scalar 6 forwards
    // `BAA93581` only for a carrier-only group where the source forwards `9602E894`. A forward
    // with no carrier scalar left is appended, as Sleeper Simulant's second `35DCAE4B` gate
    // is where carrier `80BBB1B9` has no seventh forward.
    let same = |a: &Expression, b: &Expression| {
        a.code == b.code
            && a.counts == b.counts
            && a.names == b.names
            && a.constants.len() == b.constants.len()
    };
    let mut map: Vec<Option<usize>> = vec![None; source.scalars.len()];
    let mut converted = Vec::new();
    let mut retargeted = Vec::new();
    let mut appended = Vec::new();
    let count = native.scalars.len();
    let mut used = vec![false; count];
    let mut next = 0;
    for (index, scalar) in source.scalars.iter().enumerate() {
        let order = || (next..count).chain(0..next).filter(|&k| !used[k]);
        // A kept scalar response's operand pairs with the carrier operand it replaces, as
        // Thorn's equal constants each take the row their carrier response reads.
        let seeded = seeds
            .get(&u32::try_from(index)?)
            .map(|&k| k as usize)
            .filter(|&k| k < count && !used[k] && same(scalar, &native.scalars[k]));
        let k = match seeded.or_else(|| order().find(|&k| same(scalar, &native.scalars[k]))) {
            Some(k) => k,
            // A constant with no carrier scalar left is appended, as Thorn's six constants
            // for its scalar responses are where carrier `80BBC0FE` has two.
            None if scalar.names.is_empty() && scalar.code == [0x34, 0, 0x3E, 0] => {
                appended.push((
                    edit::Appended {
                        input: None,
                        code: scalar.code.clone(),
                        constants: scalar.constants.concat(),
                        counts: scalar.counts,
                    },
                    index,
                ));
                count + appended.len() - 1
            }
            None => {
                ensure!(
                    forwards_input(scalar),
                    "Projectile scalar destinations or programs differ"
                );
                let found = order().find(|&k| {
                    let carrier = &native.scalars[k];
                    !kept_operands.contains(&(k as u32))
                        && ((carrier.names.is_empty() && carrier.code.len() == scalar.code.len())
                            || forwards_input(carrier))
                });
                match found {
                    Some(k) if native.scalars[k].names.is_empty() => {
                        converted.push((native.scalar_rows[k], scalar.names[0], index));
                        k
                    }
                    Some(k) => {
                        retargeted.push((native.scalar_rows[k], scalar.names[0], index, k));
                        k
                    }
                    None => {
                        appended.push((
                            edit::Appended {
                                input: Some(scalar.names[0]),
                                code: scalar.code.clone(),
                                constants: Vec::new(),
                                counts: scalar.counts,
                            },
                            index,
                        ));
                        count + appended.len() - 1
                    }
                }
            }
        };
        map[index] = Some(k);
        if k < count {
            used[k] = true;
            next = k + 1;
        }
    }
    Ok(ScalarPairing {
        map,
        converted,
        retargeted,
        appended,
    })
}

fn rebind_input_paths(
    native: &mut Responses,
    groups: &align::Groups,
    map: &[Option<usize>],
    node_names: &BTreeMap<(usize, u32), u32>,
    retargeted: &[RetargetedScalar],
) -> Result<()> {
    // Input paths follow source positions and names. Inputs of what is left out keep a
    // `dropped/` path, so their connections stay without being compared.
    let position = |n: usize| groups.paired.iter().position(|p| *p == Some(n));
    let scalar_position = |k: usize| map.iter().position(|m| *m == Some(k));
    let group_at = native.groups.iter().map(|g| g.at).collect::<Vec<_>>();
    for path in native.inputs.values_mut() {
        let parts = path.split('/').map(str::to_owned).collect::<Vec<_>>();
        let renamed = match parts
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["group", number, "node", node, rest @ ..] => {
                let number = number.parse::<usize>()?;
                let node = u32::from_str_radix(node, 16)?;
                match position(number) {
                    Some(i) => {
                        let name = node_names
                            .get(&(group_at[number], node))
                            .copied()
                            .unwrap_or(node);
                        format!("group/{i}/node/{name:08X}/{}", rest.join("/"))
                    }
                    None => format!("dropped/{path}"),
                }
            }
            // A retargeted scalar's carrier input reads another name, so its connection is
            // set aside and rebuilt like a created input's.
            ["scalar", number, rest @ ..] => {
                let k = number.parse::<usize>()?;
                match scalar_position(k) {
                    Some(j) if !retargeted.iter().any(|r| r.3 == k) => {
                        format!("scalar/{j}/{}", rest.join("/"))
                    }
                    _ => format!("dropped/{path}"),
                }
            }
            _ => continue,
        };
        *path = renamed;
    }
    Ok(())
}

type Labels = ([Vec<u32>; 4], u8, Option<(u32, Vec<u8>)>);
type ActionTranslation<'a> = (
    usize,
    &'a dyn Fn(usize) -> Result<Labels>,
    &'a dyn Fn(u32) -> Result<u32>,
    &'a dyn Fn(&Action) -> Result<edit::Act>,
);

fn translate_node_actions(
    sn: &Node,
    nn: &Node,
    (resource, labels, mapped, create): ActionTranslation<'_>,
    plan: &mut edit::Plan,
    (patches, resources): (&mut Vec<Value>, &mut Resources),
) -> Result<()> {
    let matched = action_pairs(&sn.actions, &nn.actions);
    let mut list = Vec::new();
    let mut replaced = false;
    for (sa, found) in sn.actions.iter().zip(&matched) {
        let Some(k) = *found else {
            list.push(create(sa)?);
            continue;
        };
        let na = &nn.actions[k];
        // A bounce whose body or programs differ is created from the source's, as
        // Sleeper Simulant group `F13988D3` counts 3 per bounce where its carrier
        // counts 1.
        if sa.selector == 2
            && na.selector == 2
            && sa.known
            && na.known
            && (sa.expressions != na.expressions
                || program_view(&sa.common) != program_view(&na.common))
        {
            list.push(create(sa)?);
            replaced = true;
            continue;
        }
        let operands = sa.indices.len() == na.indices.len()
            && sa
                .indices
                .iter()
                .zip(&na.indices)
                .all(|(&s, &n)| mapped(s).ok() == Some(n));
        // A scalar response whose operands pair with other carrier scalars is created
        // from the source's, so it reads the scalars its own operands pair with.
        if sa.selector == 0 && na.selector == 0 && sa.known && na.known && !operands {
            list.push(create(sa)?);
            replaced = true;
            continue;
        }
        ensure!(
            sa.known
                && na.known
                && native_selector(sa.selector, true) == na.selector
                && sa.expressions == na.expressions,
            "Projectile response action or embedded program differs"
        );
        // A kept attachment or spawn whose filter names other labels takes the
        // source's, compiled for the native registry. Thorn's attachment filter names
        // two labels where carrier `80BBC0FE`'s names three.
        if sa.filter != na.filter {
            let (lists, mode, record) = labels(sa.filter_at)?;
            let at = na.filter_at;
            ensure!(
                plan.labels.iter().all(|relabel| relabel.at != at),
                "Relabeled projectile filter is shared"
            );
            plan.labels.push(edit::Relabel {
                at,
                lists,
                mode,
                record,
            });
        }
        ensure!(operands, "Projectile response scalar operand differs");
        if sa.selector == 10 {
            patches.push(serde_json::to_value(NativeAssetResourcePatch {
                binding_hash: MOVEMENT,
                resource_index: 0,
                offset: u32::try_from(
                    na.parameter
                        .checked_sub(resource)
                        .context("Response precedes movement resource")?,
                )?,
                expected: na.common.clone(),
                bytes: sa.common.clone(),
                imported_particle: None,
            })?);
        } else if matches!(sa.selector, 2 | 16) {
            ensure!(
                program_view(&sa.common) == program_view(&na.common),
                "Projectile bounce or steering controls differ"
            );
        } else {
            ensure!(
                sa.common == na.common,
                "Projectile response controls require another mapping"
            );
        }
        if let (Some(a), Some(b)) = (sa.resource, na.resource) {
            resources.insert((sa.selector, a, b));
        }
        list.push(edit::Act::Keep {
            selector: na.selector,
            parameter: na.parameter,
        });
    }
    // Carrier actions that no source action keeps are left out.
    let unchanged = !replaced
        && list.len() == nn.actions.len()
        && matched
            .iter()
            .enumerate()
            .all(|(i, found)| *found == Some(i));
    if !unchanged {
        plan.actions.push((nn.at, list));
    }
    Ok(())
}

fn compare_signals(
    source: &Responses,
    native: &Responses,
    unnamed: u32,
) -> Result<Vec<(u32, u32)>> {
    let mut omitted = Vec::new();
    for (key, expression) in &source.signals {
        match native.signals.get(key) {
            Some(carrier) => ensure!(
                carrier == expression,
                "Projectile child parameter output or producer differs"
            ),
            None => omitted.push(*key),
        }
    }
    for (&(namespace, name), expression) in &native.signals {
        if source.signals.contains_key(&(namespace, name)) {
            continue;
        }
        ensure!(
            namespace == unnamed
                && expression.code == [0x3C, 1, 0x3E, 0]
                && expression.constants.is_empty()
                && expression.counts == [2, 1]
                && expression.names == [name],
            "Additional native child parameter is not an unchanged named input"
        );
    }
    Ok(omitted)
}

type ScalarEdits<'a> = (
    &'a [Option<usize>],
    &'a [(usize, u32, usize)],
    &'a [RetargetedScalar],
    Vec<(edit::Appended, usize)>,
);

fn mapped_scalar(map: &[Option<usize>], scalar: u32) -> Result<u32> {
    if scalar == u32::MAX {
        return Ok(scalar);
    }
    let k = map
        .get(scalar as usize)
        .copied()
        .flatten()
        .context("Projectile scalar index exceeds its array")?;
    Ok(u32::try_from(k)?)
}

fn plan_scalars(
    source: &Responses,
    native: &Responses,
    resource: usize,
    (map, converted, retargeted, appended): ScalarEdits<'_>,
    plan: &mut edit::Plan,
    created: &mut BTreeSet<String>,
) -> Result<Vec<Value>> {
    let count = native.scalars.len();
    let mut scalar_patches = Vec::new();
    for (index, scalar) in source.scalars.iter().enumerate() {
        let k = map[index].context("Unpaired projectile scalar")?;
        if k >= count
            || converted.iter().any(|(_, _, source)| *source == index)
            || retargeted.iter().any(|(_, _, source, _)| *source == index)
            || scalar.constants == native.scalars[k].constants
        {
            continue;
        }
        // Tuned constants take source values, as response counts and speed variation do.
        // The automatic rifle's computed scalar holds 0.57 in Rise of Iron where carrier
        // `80BBC8AC` holds 0.5.
        ensure!(
            scalar
                .constants
                .iter()
                .flat_map(|row| row.chunks_exact(4))
                .all(|lane| {
                    f32::from_le_bytes(lane.try_into().expect("four-byte lane")).is_finite()
                }),
            "Nonfinite source scalar constant"
        );
        scalar_patches.push(serde_json::to_value(NativeAssetResourcePatch {
            binding_hash: MOVEMENT,
            resource_index: 0,
            offset: u32::try_from(
                native.scalar_constants[k]
                    .checked_sub(resource)
                    .context("Scalar constants precede movement resource")?,
            )?,
            expected: native.scalars[k].constants.concat(),
            bytes: scalar.constants.concat(),
            imported_particle: None,
        })?);
    }
    for &(row, name, index) in converted {
        plan.scalars.push((row, name));
        created.insert(format!("scalar/{index}/input/0"));
    }
    for &(row, name, index, _) in retargeted {
        plan.renamed.push((row, name));
        created.insert(format!("scalar/{index}/input/0"));
    }
    for (scalar, index) in appended {
        if scalar.input.is_some() {
            created.insert(format!("scalar/{index}/input/0"));
        }
        plan.appended.push(scalar);
    }
    Ok(scalar_patches)
}
