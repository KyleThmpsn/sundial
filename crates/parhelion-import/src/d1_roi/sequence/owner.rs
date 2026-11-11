//! Complete native effect owners for Rise of Iron sequencers.
//!
//! A native sequence owner keeps its envelope's provider wrapper, lifecycle and registration
//! storage. Controls, events, scalar inputs, locators and root policy all come from the source.
//! Rise of Iron `3B5750/3B5B10/3B9860` and Shadowkeep `58ADB0/58FC90/58B9A0` read the root
//! fields with the same roles:
//!
//! | Role | Source | Native |
//! | --- | --- | --- |
//! | Entry selector | `+2C0` | `+2D0` |
//! | Root predicate | `+2C8` | `+2D8` |
//! | Automatic start | Boolean `+2C4` | `+2E0` bit 0 |
//! | Owner retirement on completion | Boolean `+2D0` | `+2E0` bit 1 |
//! | Extra activation service | Boolean `+2D3` | `+2E0` bit 2 |
//! | Update bytes | `+2D1`, `+2D2` | `+2E1`, `+2E2` |
//!
//! Native `+2E3` counts a native-only update loop, gated by bits 4 and 5. It stays zero. No
//! consumer of native bit 3 was found in the sequence code, so the envelope's value is kept.
use super::{locator::Locators, *};
use crate::{
    d1_roi::Source,
    presentation::append,
    tiger::{allocation::Row, instance},
};
use serde_json::{Value, json};

const INPUT_ROW: u32 = 0x86374C3C;
const OUTPUT_ROW: u32 = 771_384_421;
const LOCATOR_ROW: u32 = 0x846F_DE3F;
const AUXILIARY_ROW: u32 = 0xCA95_F655;

pub struct Owner {
    source: Payload,
    plan: Plan,
    locators: Locators,
    inputs: Vec<u32>,
    /// Connected scalar inputs owned by the sequence's embedded input owner. Source keeps
    /// them at definition `+2A8`, parented by the owner paired at `+260`. Native keeps them at
    /// definition `+2B8` and instance `+220`, parented by the embedded object at `+200`.
    auxiliary: Vec<u32>,
    root: [u8; 3],
}

/// A native owner and allocation pair, with every private reference left unresolved.
pub struct Written {
    pub owner: Payload,
    pub allocation: Payload,
    /// Payload offsets of unresolved words and their private symbols, after sealing.
    pub patches: Vec<(usize, String)>,
    pub prepared: Prepared,
    /// Each source scalar input name with its native definition offset, main inputs in source
    /// order followed by auxiliary inputs in source order.
    pub inputs: Vec<(u32, usize)>,
}

fn pair(p: &mut Payload, owner: u32, i: usize, d: usize, ic: u32, dc: u32) -> Result<()> {
    for (at, class, twin) in [(i, dc, d), (d, ic, i)] {
        put(&mut p.0, at, &owner.to_le_bytes())?;
        put(&mut p.0, at + 4, &class.to_le_bytes())?;
        put(&mut p.0, at + 8, &u64::try_from(twin)?.to_le_bytes())?;
    }
    Ok(())
}

impl Owner {
    pub fn read(source: &Source, owner: &Payload, omitted: &BTreeSet<(u32, u32)>) -> Result<Self> {
        let plan = Plan::read_omitting(source, owner, omitted)?;
        let mut locators = Locators::read(owner)?;
        if let Some(origin) = plan.origin() {
            ensure!(
                locators.add_origin()? == origin,
                "Source table origin locator ordinal differs"
            );
        }
        let definition = owner.pointer(24)?;
        ensure!(
            owner
                .array(definition + 0x190, 56, Some(0x80800788))?
                .is_empty(),
            "Source effect sequence publishes numeric outputs"
        );
        let names = |field| {
            owner
                .array(definition + field, 48, Some(0x80800927))?
                .into_iter()
                .map(|row| owner.u32(row + 0x28))
                .collect::<Result<Vec<_>>>()
        };
        let inputs = names(0x1A0)?;
        let auxiliary = names(0x2A8)?;
        let mut flags = 0;
        for (bit, field) in [(0, 0x2C4), (1, 0x2D0), (2, 0x2D3)] {
            let value = owner.u8(definition + field)?;
            ensure!(value <= 1, "Source sequence root Boolean differs");
            flags |= value << bit;
        }
        // Topology translation owns the entry selector and root predicate.
        ensure!(
            owner.u32(definition + 0x2C0)? == 0,
            "Source sequence starts at a different root selector"
        );
        let locator_count = locators.count();
        for index in plan.particle_locators() {
            ensure!(
                usize::from(index) < locator_count,
                "Source particle locator is undeclared"
            );
        }
        source.unchanged()?;
        Ok(Self {
            source: owner.clone(),
            plan,
            locators,
            inputs,
            auxiliary,
            root: [
                flags,
                owner.u8(definition + 0x2D1)?,
                owner.u8(definition + 0x2D2)?,
            ],
        })
    }

    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    pub fn inputs(&self) -> &[u32] {
        &self.inputs
    }

    /// Source scalar input definitions, main inputs then auxiliary inputs, in the order of the
    /// written owner's inputs. Graph connection rows name these offsets.
    pub fn definitions(&self) -> Result<Vec<usize>> {
        let definition = self.source.pointer(24)?;
        let mut rows = self
            .source
            .array(definition + 0x1A0, 48, Some(0x80800927))?;
        rows.extend(
            self.source
                .array(definition + 0x2A8, 48, Some(0x80800927))?,
        );
        Ok(rows)
    }

    pub fn locator_count(&self) -> usize {
        self.locators.count()
    }

    pub fn evidence(&self) -> Value {
        json!({"inputs":self.inputs,"auxiliary_inputs":self.auxiliary,
            "locators":self.locators.count(),
            "root":{"flags":self.root[0],"update":[self.root[1],self.root[2]]},
            "omitted":self.plan.omitted().into_iter()
                .map(|((class, kind), count)| json!({"class":class,"kind":kind,"count":count}))
                .collect::<Vec<_>>()})
    }

    /// Rebuild a native envelope as this source sequence. `inputs` supplies a native scalar
    /// input row pair when the envelope has none. Locator ordinals keep their source order.
    /// The result is sealed so the loader copies every appended runtime row.
    pub fn write(
        &self,
        template: &Payload,
        allocation: &Payload,
        inputs: &Payload,
        envelopes: &[Payload],
        bindings: &Bindings,
    ) -> Result<Written> {
        let i = template.pointer(16)?;
        let d = template.pointer(24)?;
        let owner = template.u32(i)?;
        ensure!(
            template.u32(i - 4)? == 0x808084D7
                && template.u32(d - 4)? == 0x808084E9
                && template.u64(i + 8)? == d as u64
                && template.u64(d + 8)? == i as u64
                && template.u32(d)? == owner
                && template.u64(0)? == template.0.len() as u64,
            "Native effect envelope is not an unsealed sequence pair"
        );
        ensure!(
            bindings
                .locators
                .iter()
                .all(|(&from, &to)| from == to && usize::from(from) < self.locators.count()),
            "Translated locators keep their source ordinals"
        );
        let mut output = template.clone();
        // Numeric outputs have no source counterpart in this form.
        put(&mut output.0, d + 0x178, &[0; 16])?;
        put(&mut output.0, i + 0xC0, &[0; 16])?;
        let ii = inputs.pointer(16)?;
        let id = inputs.pointer(24)?;
        let defaults = if self.inputs.is_empty() && self.auxiliary.is_empty() {
            None
        } else {
            let row = *inputs
                .array(id + 0x188, 40, Some(0x80809789))?
                .first()
                .context("Native scalar input envelope is absent")?;
            let instance = usize::try_from(inputs.u64(row + 8)?)?;
            ensure!(
                inputs.u32(row + 4)? == 0x80809788
                    && inputs.u32(instance + 4)? == 0x80809789
                    && instance > ii,
                "Native scalar input envelope pair differs"
            );
            Some((inputs.bytes::<96>(instance)?, inputs.bytes::<40>(row)?))
        };
        let mut rows = Vec::new();
        let mut written_inputs = Vec::new();
        // Main inputs belong to the sequence instance. Auxiliary inputs name the embedded
        // input owner at instance +200 as their parent, as Shadowkeep sequences do.
        for (names, runtime_field, definition_field, parent) in [
            (&self.inputs, i + 0xD0, d + 0x188, i),
            (&self.auxiliary, i + 0x220, d + 0x2B8, i + 0x200),
        ] {
            let count = names.len();
            let (runtime_row, definition_row) = match defaults {
                Some(defaults) if count > 0 => defaults,
                _ => {
                    put(&mut output.0, runtime_field, &[0; 16])?;
                    put(&mut output.0, definition_field, &[0; 16])?;
                    continue;
                }
            };
            append(
                &mut output.0,
                runtime_field,
                0x80809788,
                &runtime_row.repeat(count),
                96,
            )?;
            append(
                &mut output.0,
                definition_field,
                0x80809789,
                &definition_row.repeat(count),
                40,
            )?;
            for ((runtime, definition), &name) in output
                .array(runtime_field, 96, Some(0x80809788))?
                .into_iter()
                .zip(output.array(definition_field, 40, Some(0x80809789))?)
                .zip(names)
            {
                pair(
                    &mut output,
                    owner,
                    runtime,
                    definition,
                    0x80809788,
                    0x80809789,
                )?;
                crate::tiger::sequence::relative(&mut output, runtime + 16, parent)?;
                put(&mut output.0, definition + 32, &name.to_le_bytes())?;
                written_inputs.push((name, definition));
            }
        }
        if !self.inputs.is_empty() {
            rows.push(Row::named(
                INPUT_ROW,
                0x80809788,
                self.inputs.len(),
                Vec::new(),
            ));
        }
        if !self.auxiliary.is_empty() {
            rows.push(Row::named(
                AUXILIARY_ROW,
                u32::MAX,
                0,
                vec![Row::named(
                    0xFC39_56AD,
                    0x80809788,
                    self.auxiliary.len(),
                    Vec::new(),
                )],
            ));
        }
        rows.push(self.locators.write(&mut output)?);
        output.0[d + 0x2E0] = (output.0[d + 0x2E0] & 0x08) | self.root[0];
        output.0[d + 0x2E1] = self.root[1];
        output.0[d + 0x2E2] = self.root[2];
        output.0[d + 0x2E3] = 0;
        let mut storage = allocation.clone();
        Row::replace(
            &rows,
            &mut storage,
            32,
            &[INPUT_ROW, OUTPUT_ROW, LOCATOR_ROW, AUXILIARY_ROW],
        )?;
        let size = u64::try_from(output.0.len())?;
        put(&mut output.0, 0, &size.to_le_bytes())?;
        let prepared =
            self.plan
                .write(&self.source, &mut output, &mut storage, envelopes, bindings)?;
        let size = u64::try_from(storage.0.len())?;
        put(&mut storage.0, 0, &size.to_le_bytes())?;
        // The owner tag stays the envelope's until the containing graph assigns a private one.
        let mut patches = prepared
            .resources
            .iter()
            .map(|patch| json!({"offset":patch.offset,"symbol":patch.symbol}))
            .collect::<Vec<_>>();
        let end = output.0.len();
        instance::seal_span(&mut output, &mut patches, end)?;
        let patches = patches
            .into_iter()
            .map(|patch| {
                Ok((
                    usize::try_from(patch["offset"].as_u64().context("Sealed patch offset")?)?,
                    patch["symbol"]
                        .as_str()
                        .context("Sealed patch symbol")?
                        .to_owned(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Written {
            owner: output,
            allocation: storage,
            patches,
            prepared,
            inputs: written_inputs,
        })
    }
}
