//! Source ally selection on the native embedded query, before owner sealing.
use super::{Owner, layout};
use crate::{
    d1_roi::{Source, native::perk::label, procedure},
    presentation::{append, put},
    tiger::{
        payload::Payload,
        sequence::{object, relative},
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub(crate) struct Selection {
    radius: f32,
    bound_scale: f32,
    vertical_offset: f32,
    labels: [Vec<u32>; 4],
    predicate: Vec<u8>,
    code: Vec<u8>,
    constants: Vec<u8>,
}

impl Selection {
    pub fn read(source: &Source, p: &Payload, registry: &Payload) -> Result<Self> {
        let live = p.pointer(16)?;
        let definition = p.pointer(24)?;
        ensure!(
            p.u32(live.checked_sub(4).context("Source controller marker")?)? == 0x808025B8
                && p.u32(
                    definition
                        .checked_sub(4)
                        .context("Source controller marker")?
                )? == 0x80802A4A,
            "Source query has another controller ownership contract"
        );
        Self::query(source, p, registry, definition + 0x740, live + 0xE10)
    }

    /// The ally query of an embedded source query helper, class `80802168`, at `helper`, whose
    /// runtime twin `808027D5` is at `live`. Radiance controllers and condition volume scripts
    /// embed the same helper.
    pub(crate) fn query(
        source: &Source,
        p: &Payload,
        registry: &Payload,
        helper: usize,
        live: usize,
    ) -> Result<Self> {
        let owner = p.u32(p.pointer(16)?)?;
        ensure!(
            p.u32(helper)? == owner
                && p.u64(helper + 8)? == live as u64
                && p.u64(helper + 16)? == 0x808027D5,
            "Source query has another ownership contract"
        );
        let shape = p.pointer(helper + 0x20)?;
        let bound = p.pointer(shape + 0xB0)?;
        ensure!(
            p.u32(
                shape
                    .checked_sub(4)
                    .context("Source query geometry marker")?
            )? == 0x8080250A
                && p.u32(bound.checked_sub(4).context("Source query bound marker")?)? == 0x8080246A,
            "Source query needs another geometry translation"
        );
        for method in 0..4usize {
            let row = bound + method * 32;
            ensure!(
                p.u32(row)? == 0x8080246A
                    && p.u32(row + 4)? == method as u32
                    && p.u64(row + 8)? == u64::from(owner)
                    && p.u64(row + 16)? == bound as u64
                    && p.u64(row + 24)? == 0x8080246A,
                "Source query bound has another method contract"
            );
        }
        let radius = p.f32(shape + 0xA8)?;
        let bound_scale = p.f32(bound + 0x80)?;
        let vertical_offset = p.f32(helper + 0x28)?;
        ensure!(
            radius.is_finite()
                && radius > 0.0
                && bound_scale.is_finite()
                && bound_scale > 0.0
                && vertical_offset.is_finite()
                && p.u64(helper + 0x18)? == 0x811C9DC5
                && p.u64(helper + 0xC0)? == 0xA1
                && p.u32(helper + 0x398)? == 1
                && p.u32(helper + 0x39C)? == 0x808020AE,
            "Source query has unqualified geometry, locator, exclusions or required components"
        );
        let labels = label::destination(source, p, helper + 0xC8, registry)?
            .context("Source ally labels cannot hold in Shadowkeep")?;
        let child_labels = label::destination(source, p, helper + 0x1A0, registry)?
            .context("Source child labels cannot hold in Shadowkeep")?;
        ensure!(
            child_labels.iter().all(Vec::is_empty),
            "Source query needs child-label translation"
        );
        let predicate = label::predicate(&labels, registry)?;

        // This scalar uses builtin time only. Additional named inputs need graph connections,
        // so they cannot be retained as unbound slots in the new controller.
        let expression = helper + 0x58;
        ensure!(
            p.u32(expression)? == owner
                && p.u64(expression + 8)? == (live + 0x220) as u64
                && p.u64(expression + 16)? == 0x808008AC
                && p.u64(expression + 0x38)? == 1
                && p.u32(expression + 0x40)? == 1
                && p.u32(expression + 0x44)? <= 1
                && p.u64(expression + 0x48)? == 0
                && p.u64(expression + 0x50)? == 0
                && p.u64(expression + 0x58)? == 0
                && p.u64(expression + 0x60)? == 0x811C9DC5,
            "Source query scale needs another numeric-input translation"
        );
        let code = p.0[p.array_range(expression + 0x18, 1, Some(0x80800009))?].to_vec();
        let constants = p
            .array(expression + 0x28, 16, Some(0x80800090))?
            .into_iter()
            .map(|row| p.bytes::<16>(row))
            .collect::<Result<Vec<_>>>()?;
        let lanes = constants
            .iter()
            .map(|bytes| {
                std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("four-byte lane"))
                })
            })
            .collect::<Vec<_>>();
        ensure!(
            lanes
                .iter()
                .flatten()
                .all(|&v| f32::from_bits(v).is_finite()),
            "Nonfinite query constant"
        );
        procedure::validate(&code, &lanes, 1)?;
        Ok(Self {
            radius,
            bound_scale,
            vertical_offset,
            labels,
            predicate,
            code,
            constants: constants.concat(),
        })
    }

    pub(super) fn write(&self, owner: &mut Owner, geometry: usize) -> Result<Value> {
        let evidence = self.write_at(&mut owner.payload, owner.definition + 0x7F0, geometry)?;
        owner.size()?;
        Ok(evidence)
    }

    /// Write the query into a native embedded query helper, class `8080422E`, at `helper`, whose
    /// kind-2 sphere `808092B1` without a bound is at `geometry`.
    pub(crate) fn write_at(
        &self,
        payload: &mut Payload,
        helper: usize,
        geometry: usize,
    ) -> Result<Value> {
        ensure!(
            layout(0x808092C3)?.0 == 104 && layout(0x80809316)?.0 == 16,
            "Native query bound or predicate declaration differs"
        );
        let id = payload.u32(payload.pointer(16)?)?;
        let bound = object(payload, 0x808092C3, 104)?;
        put(&mut payload.0, geometry + 4, &self.radius.to_le_bytes())?;
        relative(payload, geometry + 8, bound)?;
        for method in 0..4u32 {
            let row = bound + method as usize * 24;
            put(&mut payload.0, row, &0x808092C3u32.to_le_bytes())?;
            put(&mut payload.0, row + 4, &method.to_le_bytes())?;
            put(&mut payload.0, row + 8, &id.to_le_bytes())?;
            put(&mut payload.0, row + 12, &0x808092C3u32.to_le_bytes())?;
            put(&mut payload.0, row + 16, &(bound as u64).to_le_bytes())?;
        }
        put(
            &mut payload.0,
            bound + 0x60,
            &self.bound_scale.to_le_bytes(),
        )?;
        put(
            &mut payload.0,
            helper + 0xE0,
            &self.vertical_offset.to_le_bytes(),
        )?;
        // Both player graph families export a fieldless marker with one no-op method:
        // source 27E2 through metadata 80AABC12, native 3B6E through 80C2262A.
        put(&mut payload.0, helper + 0x170, &1u32.to_le_bytes())?;
        put(&mut payload.0, helper + 0x174, &0x80803B70u32.to_le_bytes())?;
        put(&mut payload.0, helper + 0x28, &[0])?;
        append(&mut payload.0, helper + 0x30, 0x80809316, &[0; 48], 16)?;
        let rows = payload.array(helper + 0x30, 16, Some(0x80809316))?;
        let label = object(payload, 0x80804D73, self.predicate.len())?;
        put(&mut payload.0, label, &self.predicate)?;
        relative(payload, rows[0] + 8, label)?;
        // Original 5ACBA0 bits 5 and 7 become inverted predicates. Inversion also preserves
        // the relationship predicate's absent-caster branch. F44120 excludes the caster.
        for (row, class) in [(rows[1], 0x80804D76), (rows[2], 0x80804D7B)] {
            let term = object(payload, class, layout(class)?.0)?;
            put(&mut payload.0, row, &[1])?;
            relative(payload, row + 8, term)?;
            if class == 0x80804D76 {
                put(&mut payload.0, term, &[1])?;
            }
        }
        let scale = helper + 0xF8;
        append(&mut payload.0, scale + 0x10, 0x80800009, &self.code, 1)?;
        append(
            &mut payload.0,
            scale + 0x20,
            0x80800090,
            &self.constants,
            16,
        )?;
        put(&mut payload.0, scale + 0x30, &1u64.to_le_bytes())?;
        put(&mut payload.0, scale + 0x38, &1u64.to_le_bytes())?;
        Ok(json!({"class":0x808092B1u32,"kind":2,"radius":self.radius,
            "nested_provider":true,"bound_scale":self.bound_scale,"vertical_offset":self.vertical_offset,
            "labels":self.labels,"required_component":0x80803B70u32,
            "scale_program":hex::encode(&self.code),"source_exclusion_mask":0xA1,
            "gameplay_verified":false}))
    }
}
