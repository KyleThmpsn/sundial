//! Rise of Iron area queries become native kind-29 queries with the same geometry, programs,
//! actions and damage block.
//!
//! Source `808028A6` and native `80804AB9` both declare `520` bytes. From source `+F0` through
//! `+517` every field sits `48` bytes lower in the native definition. The front differs:
//!
//! | Role | Source | Native |
//! | --- | --- | --- |
//! | Locator for `3B7020` / `D534BC` | `+20` | `+40` |
//! | Geometry | `8080250A` at `+28`, radius `+A8`, bound `8080246A` scale `+80` or `8080248B` | Sphere `808092B1` at `+48`, bound `808092C3` scale `+60` or `808092C4` |
//! | Broadphase block passed with the geometry to `5AD6E0` / `B94700` | `+30..+5F` | `+70..+9F` |
//! | Relationship exclusion `+61`, scale `+64` and label filter `+68` | Source filters | Predicate `+50` |
//!
//! Rise of Iron completes an empty query by clearing its instance word. Native completes only
//! when byte `+4D0` is set, and every stock query sets that byte together with flag bit `10`.
//! The native-only program at `+4D8` is the constant one used by 2,035 of 2,094 stock queries.
//!
//! The source relationship mask at `+61` reaches `5ACBA0` for every candidate. Native `B87740`
//! ports it bit for bit: caster entity, all but the caster, players, non-players, friendly
//! targets other than the caster's own actor, hostile targets and one bitmap. The native damage
//! action `80804AC8`, derived from `80804AC9` with the mask at `+E8`, calls it before its damage
//! (`D55EE3..D55F0A`). Bit 6 needs the scale at `+64`, which that action has no field for. The
//! source skips a masked candidate at collection, and native skips it at the action, after line
//! of sight and work budgets count it.
//!
//! Attachment actions create a graph on each surviving candidate, with the host as owner, and
//! feed it the direction and spatial weight named at query `+1D0` (native `+188`). Source
//! dispatcher `004AC2C0` and native `D55C70` have the same shape. Source `808020BC` (graph at
//! `+08`) is native `80804AC7`, and its derived `80803207` adds a relationship mask at `+10`, as
//! native `80804AC6` does. Native tests only the action's mask, so a masked query's attachments
//! take the derived class with the query mask merged in.
use super::*;
use crate::{
    d1_roi::{Source, dependency},
    presentation::append,
};

use super::program::Program;

struct Action {
    gate: Option<Program>,
    effect: Effect,
}

enum Effect {
    Damage {
        /// The damage action's two programs, which may read sequence inputs.
        scales: [Program; 2],
        /// Source outgoing profile component owner, or `None` for an empty action resource.
        profile: Option<u32>,
    },
    /// A graph attached to each candidate, with the action's own relationship mask.
    Attach { graph: u32, mask: u8 },
}

/// A query sphere's bound.
#[derive(Clone, Copy)]
enum Bound {
    /// None, as 1,003 of the 2,094 stock native queries have.
    Absent,
    /// `8080246A` with its scale at `+80`, written as `808092C3` with the scale at `+60`.
    Scaled(f32),
    /// `8080248B`, three method rows and its other fields at their defaults, written as
    /// `808092C4` with its own defaults. Rise of Iron graph `80A6A699` and its native counterpart
    /// `80C0EE13`, paired by kept spawns in automatic rifle trees, hold these bounds on the same
    /// radius-2 sphere.
    Default,
}

pub struct Query {
    source_offset: usize,
    flags: u32,
    locator: u8,
    radius: f32,
    bound_shape: Bound,
    block: [u8; 48],
    budgets: [u8; 12],
    spatial: Option<Program>,
    bound: [Option<Program>; 2],
    /// The relationship mask the damage actions apply, zero for none.
    exclusion: u8,
    actions: Vec<Action>,
    name: [u8; 16],
    shaping: Option<Program>,
    feedback: Option<u32>,
    damage_word: [u8; 8],
    damage: Vec<Option<Program>>,
    tail: [u8; 8],
}

fn zero(owner: &Payload, from: usize, to: usize) -> Result<bool> {
    Ok(owner
        .0
        .get(from..to)
        .context("Source query field extent")?
        .iter()
        .all(|b| *b == 0))
}

impl Query {
    pub fn read(source: &Source, owner: &Payload, at: usize) -> Result<Self> {
        ensure!(
            owner.u32(at.checked_sub(4).context("Source query marker")?)? == 0x808028A6
                && owner.u32(at + 0x1C)? == 26,
            "Source query event class or kind differs"
        );
        owner.bytes::<0x520>(at)?;
        let locator = owner.u32(at + 0x20)?;
        let declared = owner
            .array(owner.pointer(24)? + 0x1B0, 5, Some(0x80800429))?
            .len();
        ensure!(
            (locator as usize) < declared && owner.u32(at + 0x24)? == 0,
            "Source query locator is undeclared"
        );
        // Geometry: the radius-only shape and its scaled bound.
        let shape = owner.pointer(at + 0x28)?;
        ensure!(
            owner.u32(shape - 4)? == 0x8080250A,
            "Source query geometry is not the qualified sphere"
        );
        let radius = owner.f32(shape + 0xA8)?;
        ensure!(
            radius.is_finite() && radius > 0.0,
            "Source query radius is invalid"
        );
        let bound_shape = if owner.u64(shape + 0xB0)? == 0 {
            Bound::Absent
        } else {
            let bound = owner.pointer(shape + 0xB0)?;
            match owner.u32(bound - 4)? {
                0x8080246A => {
                    let scale = owner.f32(bound + 0x80)?;
                    ensure!(
                        scale.is_finite() && scale > 0.0,
                        "Source query bound scale is invalid"
                    );
                    Bound::Scaled(scale)
                }
                // Method rows of 32 bytes at `+00`, `+40` and `+60` name the owner and the bound.
                // The fields between the first two rows and the word at `+80` hold the defaults.
                0x8080248B => {
                    let owner_tag = owner.u32(owner.pointer(16)?)?;
                    for (method, row) in [(0u32, 0x00), (1, 0x40), (2, 0x60)] {
                        ensure!(
                            owner.u32(bound + row)? == 0x8080248B
                                && owner.u32(bound + row + 4)? == method
                                && owner.u32(bound + row + 8)? == owner_tag
                                && owner.u64(bound + row + 16)? == bound as u64
                                && owner.u32(bound + row + 24)? == 0x8080248B,
                            "Source query bound method row differs"
                        );
                    }
                    let fields = (0..8)
                        .map(|word| owner.u32(bound + 0x20 + word * 4))
                        .collect::<Result<Vec<_>>>()?;
                    ensure!(
                        fields == [u32::MAX, u32::MAX, u32::MAX, 0, 0, 0, u32::MAX, 0]
                            && owner.u32(bound + 0x80)? == 0,
                        "Source query bound fields are not the defaults"
                    );
                    Bound::Default
                }
                _ => anyhow::bail!("Source query bound class differs"),
            }
        };
        // The relationship mask carries to the native damage action. Its scale bit and the
        // label filter have no native counterpart here. Source `005ACBA0` reads the scale at
        // `+64` only under bit 6, through `005ACE10`, so with bit 6 clear any finite scale is
        // inert, as With a Laser Beam's 0.0 is.
        let exclusion = owner.u8(at + 0x61)?;
        ensure!(
            owner.u8(at + 0x60)? == 0
                && owner.u16(at + 0x62)? == 0
                && exclusion & 0x40 == 0
                && owner.f32(at + 0x64)?.is_finite()
                && zero(owner, at + 0x68, at + 0xE8)?,
            "Source query has a scaled relationship exclusion or label filters"
        );
        // The label filter is four compiled 256-bit masks followed by compiler `00435C00`'s
        // bypass flags. Every mask is empty, so every flag is one.
        ensure!(
            owner.bytes::<4>(at + 0xE8)? == [1; 4],
            "Source query label bypass flags differ"
        );
        ensure!(
            zero(owner, at + 0x1A8, at + 0x1C0)?
                && owner.u64(at + 0x210)? == 0
                && owner.u32(at + 0x218)? == u32::MAX
                && zero(owner, at + 0x21C, at + 0x238)?
                && zero(owner, at + 0x23C, at + 0x290)?,
            "Source query has auxiliary, presentation or named damage references"
        );
        let mut actions = Vec::new();
        for row in owner.array(at + 0x1C0, 0x58, Some(0x8080238E))? {
            ensure!(
                owner.u32(row + 0x40)? == 0
                    && owner.u32(row + 0x44)? == 0
                    && owner.u32(row + 0x48)? == u32::MAX
                    && owner.u32(row + 0x4C)? == 0,
                "Source query action row has an auxiliary resource"
            );
            let gate = Program::read(owner, row, true, 1)?;
            let action = owner.pointer(row + 0x50)?;
            let class = owner.u32(action - 4)?;
            if matches!(class, 0x808020BC | 0x80803207) {
                // Neither dispatcher reads `+00`. The derived class's mask is one byte.
                let derived = class == 0x80803207;
                let mask = if derived { owner.u8(action + 0x10)? } else { 0 };
                let graph = owner.u32(action + 8)?;
                ensure!(
                    owner.u64(action)? == 0
                        && owner.u32(action + 12)? == 0
                        && (!derived || zero(owner, action + 0x11, action + 0x18)?)
                        && mask & 0x40 == 0,
                    "Source query attachment has a scaled relationship exclusion or an extension"
                );
                ensure!(
                    source.reference(graph)? == 0x80800734,
                    "Source query attachment does not name an entity graph"
                );
                actions.push(Action {
                    gate,
                    effect: Effect::Attach { graph, mask },
                });
                continue;
            }
            ensure!(
                class == 0x8080243F
                    && owner.u64(action)? == 0
                    && zero(owner, action + 12, action + 0x20)?,
                "Source query action class or extension differs"
            );
            // The two programs fill the native damage action's first two in order. Most stock
            // queries carry the constant one in both. Twenty stock actions read a sequence input
            // in the first, as Rise of Iron hand cannon spawn `80A6A859` does.
            let scale = |offset| -> Result<Program> {
                Program::read_with_inputs(owner, action + offset, 1)?
                    .context("Source query action program is absent")
            };
            let scales = [scale(0x20)?, scale(0x60)?];
            let tag = owner.u32(action + 8)?;
            let profile = if tag == u32::MAX {
                None
            } else {
                ensure!(
                    source.reference(tag)? == 0x80800861,
                    "Source query action does not name a profile owner"
                );
                Some(tag)
            };
            actions.push(Action {
                gate,
                effect: Effect::Damage { scales, profile },
            });
        }
        ensure!(!actions.is_empty(), "Source query has no action");
        let feedback = match owner.u32(at + 0x238)? {
            u32::MAX => None,
            tag => {
                ensure!(
                    source.reference(tag)? == 0x80802637,
                    "Source query damage block does not name a feedback resource"
                );
                Some(tag)
            }
        };
        let damage = (0..10)
            .map(|i| Program::read(owner, at + 0x298 + i * 64, true, 2))
            .collect::<Result<Vec<_>>>()?;
        source.unchanged()?;
        Ok(Self {
            source_offset: at,
            flags: owner.u32(at + 8)?,
            locator: u8::try_from(locator)?,
            radius,
            bound_shape,
            block: owner.bytes::<48>(at + 0x30)?,
            budgets: owner.bytes::<12>(at + 0xEC)?,
            spatial: Program::read(owner, at + 0xF8, false, 1)?,
            bound: [
                Program::read(owner, at + 0x128, true, 1)?,
                Program::read(owner, at + 0x168, true, 1)?,
            ],
            exclusion,
            actions,
            name: owner.bytes::<16>(at + 0x1D0)?,
            shaping: Program::read(owner, at + 0x1E0, false, 1)?,
            feedback,
            damage_word: owner.bytes::<8>(at + 0x290)?,
            damage,
            tail: owner.bytes::<8>(at + 0x518)?,
        })
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.source_offset,
            NativeEvent {
                source_class: 0x808028A6,
                source_kind: 26,
                class: 0x80804AB9,
            },
        )
    }

    pub fn locator(&self) -> u8 {
        self.locator
    }

    pub fn profiles(&self) -> impl Iterator<Item = u32> + '_ {
        self.actions
            .iter()
            .filter_map(|action| match action.effect {
                Effect::Damage { profile, .. } => profile,
                Effect::Attach { .. } => None,
            })
    }

    /// Graphs the attachment actions create on candidates.
    pub fn graphs(&self) -> impl Iterator<Item = u32> + '_ {
        self.actions
            .iter()
            .filter_map(|action| match action.effect {
                Effect::Attach { graph, .. } => Some(graph),
                Effect::Damage { .. } => None,
            })
    }

    pub fn feedback(&self) -> Option<u32> {
        self.feedback
    }

    /// Fill a freshly allocated native query. Profile owners, attached private graphs and the
    /// feedback resource stay unresolved until the containing graph allocates their private
    /// translations. An attached graph in `stock_graphs` is named directly.
    pub fn write(
        &self,
        target: &mut Payload,
        event: usize,
        profiles: &BTreeMap<u32, String>,
        (graphs, stock_graphs): (&BTreeMap<u32, String>, &BTreeMap<u32, u32>),
        feedback: Option<&str>,
        (locators, inputs): (&BTreeMap<u8, u8>, &BTreeMap<u32, u32>),
    ) -> Result<Vec<ResourcePatch>> {
        target.bytes::<0x520>(event)?;
        let instance = usize::try_from(target.u64(event + 8)?)?;
        ensure!(
            target.u32(event.checked_sub(4).context("Native query marker")?)? == 0x80804AB9
                && target.u32(event + 0x30)? == 29
                && target.u32(event + 4)? == 0x80804AB8
                && target.u32(
                    instance
                        .checked_sub(4)
                        .context("Native query runtime marker")?
                )? == 0x80804AB8
                && target.u64(instance + 8)? == event as u64
                && zero(target, event + 0x40, event + 0x520)?,
            "Native query event pair, kind or body differs"
        );
        ensure!(
            feedback.is_some() == self.feedback.is_some(),
            "Query feedback binding differs"
        );
        let mut output = target.clone();
        let mut patches = Vec::new();
        let flags = (self.flags | 0x10).to_le_bytes();
        put(&mut output.0, event + 0x18, &flags)?;
        let locator = locators
            .get(&self.locator)
            .copied()
            .context("Query locator has no native mapping")?;
        put(
            &mut output.0,
            event + 0x40,
            &u32::from(locator).to_le_bytes(),
        )?;
        // Native sphere and an optional bound. The bound's four method rows name this owner and
        // object.
        let owner = output.u32(output.pointer(16)?)?;
        let sphere = native::object(&mut output, 0x808092B1, 16)?;
        put(&mut output.0, sphere, &2u32.to_le_bytes())?;
        put(&mut output.0, sphere + 4, &self.radius.to_le_bytes())?;
        native::relative(&mut output, event + 0x48, sphere)?;
        // Each method row names this owner and the bound object.
        let rows = |output: &mut Payload, bound: usize, class: u32, rows: &[usize]| -> Result<()> {
            for (method, &row) in rows.iter().enumerate() {
                let row = bound + row;
                put(&mut output.0, row, &class.to_le_bytes())?;
                put(
                    &mut output.0,
                    row + 4,
                    &u32::try_from(method)?.to_le_bytes(),
                )?;
                put(&mut output.0, row + 8, &owner.to_le_bytes())?;
                put(&mut output.0, row + 12, &class.to_le_bytes())?;
                put(&mut output.0, row + 16, &(bound as u64).to_le_bytes())?;
            }
            Ok(())
        };
        match self.bound_shape {
            Bound::Absent => {}
            Bound::Scaled(scale) => {
                let bound = native::object(&mut output, 0x808092C3, 104)?;
                native::relative(&mut output, sphere + 8, bound)?;
                rows(&mut output, bound, 0x808092C3, &[0, 24, 48, 72])?;
                put(&mut output.0, bound + 0x60, &scale.to_le_bytes())?;
            }
            // Rows at `+00`, `+30` and `+48`, and between the first two the defaults stock
            // `80C0EE13` holds: four `FFFFFFFF` words and eight zero bytes.
            Bound::Default => {
                let bound = native::object(&mut output, 0x808092C4, 96)?;
                native::relative(&mut output, sphere + 8, bound)?;
                rows(&mut output, bound, 0x808092C4, &[0, 0x30, 0x48])?;
                put(&mut output.0, bound + 0x18, &[0xFF; 16])?;
                put(&mut output.0, bound + 0x28, &[0; 8])?;
            }
        }
        put(&mut output.0, event + 0x70, &self.block)?;
        put(&mut output.0, event + 0xA4, &self.budgets)?;
        Program::write(self.spatial.as_ref(), &mut output, event + 0xB0, false)?;
        Program::write(self.bound[0].as_ref(), &mut output, event + 0xE0, true)?;
        Program::write(self.bound[1].as_ref(), &mut output, event + 0x120, true)?;
        let rows = vec![0; self.actions.len() * 0x58];
        append(&mut output.0, event + 0x178, 0x80804ABF, &rows, 0x58)?;
        let rows = output.array(event + 0x178, 0x58, Some(0x80804ABF))?;
        for (row, action) in rows.into_iter().zip(&self.actions) {
            Program::write(action.gate.as_ref(), &mut output, row, true)?;
            put(&mut output.0, row + 0x48, &u32::MAX.to_le_bytes())?;
            let (scales, profile) = match action.effect {
                Effect::Damage {
                    ref scales,
                    profile,
                } => (scales, profile),
                Effect::Attach { graph, mask } => {
                    let mask = self.exclusion | mask;
                    let node = if mask == 0 {
                        native::object(&mut output, 0x80804AC7, 0x10)?
                    } else {
                        let node = native::object(&mut output, 0x80804AC6, 0x18)?;
                        output.0[node + 0x10] = mask;
                        node
                    };
                    native::relative(&mut output, row + 0x50, node)?;
                    if let Some(&stock) = stock_graphs.get(&graph) {
                        put(&mut output.0, node + 8, &stock.to_le_bytes())?;
                    } else {
                        let symbol = graphs
                            .get(&graph)
                            .context("Query attachment graph has no private translation")?;
                        dependency::symbol(symbol)?;
                        put(&mut output.0, node + 8, &u32::MAX.to_le_bytes())?;
                        patches.push(ResourcePatch {
                            offset: node + 8,
                            symbol: symbol.clone(),
                            source: graph,
                            source_class: 0x80800734,
                            native_class: 0x80809C0F,
                        });
                    }
                    continue;
                }
            };
            // A masked query's damage takes the derived action, whose `+E8` holds the mask.
            let node = if self.exclusion == 0 {
                native::object(&mut output, 0x80804AC9, 0xE8)?
            } else {
                let node = native::object(&mut output, 0x80804AC8, 0xF0)?;
                output.0[node + 0xE8] = self.exclusion;
                node
            };
            native::relative(&mut output, row + 0x50, node)?;
            put(&mut output.0, node + 8, &u32::MAX.to_le_bytes())?;
            // The source programs fill the first two. The native-only third fills request `+0C`
            // with the zero stock damage actions evaluate.
            for (offset, program) in [0x28, 0x68].into_iter().zip(scales) {
                Program::write(
                    Some(&program.mapped(inputs)?),
                    &mut output,
                    node + offset,
                    true,
                )?;
            }
            Program::write(
                Some(&Program::constant(0.0, 1)),
                &mut output,
                node + 0xA8,
                true,
            )?;
            if let Some(profile) = profile {
                let symbol = profiles
                    .get(&profile)
                    .context("Query profile has no private translation")?;
                dependency::symbol(symbol)?;
                patches.push(ResourcePatch {
                    offset: node + 8,
                    symbol: symbol.clone(),
                    source: profile,
                    source_class: 0x80800861,
                    native_class: 0x80809C36,
                });
            }
        }
        put(&mut output.0, event + 0x188, &self.name)?;
        Program::write(self.shaping.as_ref(), &mut output, event + 0x198, false)?;
        put(&mut output.0, event + 0x1D0, &u32::MAX.to_le_bytes())?;
        put(&mut output.0, event + 0x1F0, &u32::MAX.to_le_bytes())?;
        if let (Some(source), Some(symbol)) = (self.feedback, feedback) {
            dependency::symbol(symbol)?;
            patches.push(ResourcePatch {
                offset: event + 0x1F0,
                symbol: symbol.into(),
                source,
                source_class: 0x80802637,
                native_class: 0x80803EB0,
            });
        }
        put(&mut output.0, event + 0x248, &self.damage_word)?;
        for (i, program) in self.damage.iter().enumerate() {
            Program::write(program.as_ref(), &mut output, event + 0x250 + i * 64, true)?;
        }
        output.0[event + 0x4D0] = 1;
        Program::write(
            Some(&Program::constant(1.0, 1)),
            &mut output,
            event + 0x4D8,
            true,
        )?;
        put(&mut output.0, event + 0x518, &self.tail)?;
        // The runtime record keeps the qualified stock envelope's initial age and empty queues.
        *target = output;
        Ok(patches)
    }
}
