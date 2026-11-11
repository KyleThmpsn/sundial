//! Rise of Iron kind-10 object creation becomes native kind-15 creation of a private graph.
//!
//! Source start callback `0045AF10` reads mode `+20`, graph `+24`, parameter list `+28`, trace and
//! identity bytes `+38/+39`, locator `+3C`, speed interval `+40/+44` and spread `+48`. Mode 2
//! creates a detached world object, the same role native mode 2 has at `+72`. Without a spread
//! object the source takes its single-object path, matching native creation with no spread.
//!
//! Native sequence `80B75177` ports Outbreak Prime's Virulence sequence `80A88C77`, keeping its
//! three event names, created graph role and values. It places the source parameter list `+28` at
//! `+88`, locator `+3C` at `+98`, speed `+40/+44` at `+9C/+A0`, spread `+48` at `+B0` and the
//! single-object word `+50` at `+B8`. It keeps no counterpart of identity byte `+39`, which is one
//! in all three source events, so the byte is not carried.
//!
//! The parameter list is a counted array of named expressions the created graph receives, 72-byte
//! source `808008A1` rows and native `8080889B` rows of the same layout: program `+00`, constants
//! `+10`, dimensions `+20/+28`, inputs `+30` and name `+40`. Evaluator `58AFD0` walks it on
//! creation. The source lists builtin time as input `-1`, which native supplies implicitly, so
//! `80B75177` keeps the program and the remaining sequence input ordinals.
use super::*;
use crate::{
    d1_roi::{Source, dependency, procedure},
    presentation::append,
};

/// One named expression the created graph receives.
struct Parameter {
    code: Vec<u8>,
    constants: Vec<u8>,
    inputs: Vec<u32>,
    name: u32,
}

/// The source parameter list at `field`, checked against the sequence's `declared` inputs.
fn parameters(owner: &Payload, field: usize, declared: usize) -> Result<Vec<Parameter>> {
    let mut parameters = Vec::new();
    for row in owner.array(field, 0x48, Some(0x808008A1))? {
        let code = owner.array_range(row, 1, Some(0x80800009))?;
        let constants = owner.array(row + 0x10, 16, Some(0x80800090))?;
        let lanes = constants
            .iter()
            .map(|&at| {
                Ok([
                    owner.u32(at)?,
                    owner.u32(at + 4)?,
                    owner.u32(at + 8)?,
                    owner.u32(at + 12)?,
                ])
            })
            .collect::<Result<Vec<_>>>()?;
        let inputs = owner
            .array(row + 0x30, 4, Some(0x80800007))?
            .into_iter()
            .map(|at| owner.u32(at))
            .collect::<Result<Vec<_>>>()?;
        let Some((&u32::MAX, ordinals)) = inputs.split_first() else {
            anyhow::bail!("Source creation parameter does not lead with builtin time");
        };
        ensure!(
            ordinals
                .iter()
                .all(|&ordinal| (ordinal as usize) < declared)
                && owner.u64(row + 0x20)? == 2
                && owner.u64(row + 0x28)? == 1
                && owner.u32(row + 0x44)? == 0,
            "Source creation parameter has another input or dimension"
        );
        procedure::validate(&owner.0[code.clone()], &lanes, inputs.len())?;
        parameters.push(Parameter {
            code: owner.0[code].to_vec(),
            constants: constants
                .into_iter()
                .map(|at| owner.bytes::<16>(at))
                .collect::<Result<Vec<_>>>()?
                .concat(),
            inputs: ordinals.to_vec(),
            name: owner.u32(row + 0x40)?,
        });
    }
    Ok(parameters)
}

/// Rise of Iron spread objects have three interface rows of stride `20` before their form, where
/// native `80808891` rows have stride `18`. The flag bytes and four floats are the same form in
/// both builds. Stock native and source corpora share every examined form, such as Virulence's
/// `04 00 01 00` with equal 50 and 80 degree bounds.
const SPREAD: u32 = 0x80800803;

pub struct Spawn {
    source_offset: usize,
    /// Source and native mode: 0 joins the host's graph chain, 2 creates a detached object.
    mode: u8,
    graph: u32,
    locator: u8,
    parameters: Vec<Parameter>,
    speed: [f32; 2],
    spread: Option<([u8; 4], [f32; 4])>,
    single: u32,
}

fn spread(owner: &Payload, field: usize) -> Result<Option<([u8; 4], [f32; 4])>> {
    if owner.u64(field)? == 0 {
        return Ok(None);
    }
    let at = owner.pointer(field)?;
    for ordinal in 0..3 {
        let row = at + ordinal * 0x20;
        ensure!(
            owner.u32(row)? == SPREAD
                && owner.u32(row + 4)? == ordinal as u32
                && owner.u64(row + 0x10)? == at as u64,
            "Source creation spread interface rows differ"
        );
    }
    let values = [
        owner.f32(at + 0x64)?,
        owner.f32(at + 0x68)?,
        owner.f32(at + 0x6C)?,
        owner.f32(at + 0x70)?,
    ];
    ensure!(
        values.iter().all(|value| value.is_finite()),
        "Source creation spread is not finite"
    );
    Ok(Some((owner.bytes::<4>(at + 0x60)?, values)))
}

impl Spawn {
    pub fn read(source: &Source, owner: &Payload, at: usize) -> Result<Self> {
        ensure!(
            owner.u32(at.checked_sub(4).context("Source creation marker")?)? == 0x808003BB
                && owner.u32(at + 0x1C)? == 10,
            "Source creation event class or kind differs"
        );
        owner.bytes::<0x80>(at)?;
        // Detached world creation has a qualified native counterpart. The parameter list, speed,
        // spread and single-object word carry to their native fields. Mode 0 instead adds the
        // created graph to the host object's own graph chain, capped at 64, in source `002C3980`
        // and its native port `0056DE00`, and reads no locator, speed, spread or creation word.
        // Native removes that graph when its event ends if the event has flags `18` or a
        // duration, and the source never does, so mode 0 needs an event with no flags or timing.
        let speed = [owner.f32(at + 0x40)?, owner.f32(at + 0x44)?];
        let single = owner.u32(at + 0x50)?;
        let mode = owner.u8(at + 0x20)?;
        ensure!(
            mode != 0 || owner.0[at + 0x08..at + 0x1C].iter().all(|b| *b == 0),
            "Source attached creation has flags or timing"
        );
        ensure!(
            matches!(mode, 0 | 2)
                && owner.bytes::<3>(at + 0x21)? == [0; 3]
                && owner.u8(at + 0x38)? == 0
                && owner.u8(at + 0x39)? <= 1
                && owner.bytes::<2>(at + 0x3A)? == [0; 2]
                && speed.iter().all(|value| value.is_finite())
                && single <= 1
                && owner.0[at + 0x54..at + 0x80].iter().all(|b| *b == 0),
            "Source creation needs another mode, trace or creation word"
        );
        let spread = spread(owner, at + 0x48)?;
        // A zero single-object word always comes with a spread, but 175 source and 251 stock
        // detached creations keep one beside a spread. No reader of the word was found, so it
        // carries as stored.
        ensure!(
            single == 1 || spread.is_some(),
            "Source creation spread and single-object word disagree"
        );
        let graph = owner.u32(at + 0x24)?;
        ensure!(
            source.reference(graph)? == 0x80800734,
            "Source creation does not name an entity graph"
        );
        let locator = owner.u32(at + 0x3C)?;
        let declared = owner
            .array(owner.pointer(24)? + 0x1B0, 5, Some(0x80800429))?
            .len();
        ensure!(
            (locator as usize) < declared,
            "Source creation locator is undeclared"
        );
        let declared_inputs = owner
            .array(owner.pointer(24)? + 0x1A0, 48, Some(0x80800927))?
            .len();
        Ok(Self {
            source_offset: at,
            mode,
            graph,
            locator: u8::try_from(locator)?,
            parameters: parameters(owner, at + 0x28, declared_inputs)?,
            speed,
            spread,
            single,
        })
    }

    pub fn graph(&self) -> u32 {
        self.graph
    }

    pub fn locator(&self) -> u8 {
        self.locator
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.source_offset,
            NativeEvent {
                source_class: 0x808003BB,
                source_kind: 10,
                class: 0x80808881,
            },
        )
    }

    /// The created graph stays unresolved until the containing graph allocates its private
    /// translation.
    pub fn write(
        &self,
        target: &mut Payload,
        event: usize,
        symbol: &str,
        locators: &BTreeMap<u8, u8>,
    ) -> Result<ResourcePatch> {
        dependency::symbol(symbol)?;
        let offset = self.place(target, event, u32::MAX, locators)?;
        Ok(ResourcePatch {
            offset,
            symbol: symbol.into(),
            source: self.graph,
            source_class: 0x80800734,
            native_class: 0x80809C0F,
        })
    }

    /// Create a stock graph, which needs no private translation.
    pub fn write_stock(
        &self,
        target: &mut Payload,
        event: usize,
        graph: u32,
        locators: &BTreeMap<u8, u8>,
    ) -> Result<()> {
        self.place(target, event, graph, locators).map(|_| ())
    }

    /// Write the event with `graph` in its entity-reference field, returning that field.
    fn place(
        &self,
        target: &mut Payload,
        event: usize,
        graph: u32,
        locators: &BTreeMap<u8, u8>,
    ) -> Result<usize> {
        let locator = locators
            .get(&self.locator)
            .copied()
            .context("Creation locator has no native mapping")?;
        let mut output = target.clone();
        let offset = native::spawn::write(
            &mut output,
            event,
            &native::spawn::Parameters {
                locator: u32::from(locator),
                position: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 2],
                speed: self.speed,
                cone: 0.0,
                angular_min: [0.0; 3],
                angular_max: [0.0; 3],
            },
        )?;
        put(&mut output.0, offset, &graph.to_le_bytes())?;
        // The writer stores detached creation, `2, FF`. Byte `+73` is a slot only modes 1 and 2
        // use, and stock mode 0 events keep it at `FF` too.
        output.0[event + 0x72] = self.mode;
        let rows = self
            .parameters
            .iter()
            .flat_map(|parameter| {
                let mut row = [0; 0x48];
                row[0x20] = 2;
                row[0x28] = 1;
                row[0x40..0x44].copy_from_slice(&parameter.name.to_le_bytes());
                row
            })
            .collect::<Vec<_>>();
        append(&mut output.0, event + 0x88, 0x8080889B, &rows, 0x48)?;
        let placed = output.array(event + 0x88, 0x48, Some(0x8080889B))?;
        for (row, parameter) in placed.into_iter().zip(&self.parameters) {
            append(&mut output.0, row, 0x80800009, &parameter.code, 1)?;
            append(
                &mut output.0,
                row + 0x10,
                0x80800090,
                &parameter.constants,
                16,
            )?;
            let inputs = parameter
                .inputs
                .iter()
                .flat_map(|ordinal| ordinal.to_le_bytes())
                .collect::<Vec<_>>();
            append(&mut output.0, row + 0x30, 0x80800007, &inputs, 4)?;
        }
        put(&mut output.0, event + 0xB8, &self.single.to_le_bytes())?;
        if let Some((flags, values)) = self.spread {
            native::spawn::spread_form(&mut output, event + 0xB0, flags, values)?;
        }
        *target = output;
        Ok(offset)
    }
}
