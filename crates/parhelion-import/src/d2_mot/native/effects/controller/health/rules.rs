//! Conditional health multipliers preserve OR groups, nested ANDs and scalar indices.
use super::*;
use crate::d2_mot::native::effects::resources::selectors;
use std::collections::BTreeSet;

struct Writer<'a> {
    source: &'a Payload,
    output: &'a mut Payload,
    resources: &'a selectors::Resources,
    programs: usize,
    active: BTreeSet<usize>,
    nodes: usize,
}

impl Writer<'_> {
    fn conditions(&mut self, from: usize, to: usize, depth: usize) -> Result<()> {
        let rows = self.source.array(from, 8, Some(0x80803F4F))?;
        ensure!(
            rows.len() <= 256 && (!rows.is_empty() || self.source.u64(from + 8)? == 0),
            "health condition list extent differs"
        );
        append_array(
            &mut self.output.0,
            to,
            0x80804AE2,
            &vec![0; rows.len() * 8],
            8,
        )?;
        let targets = self.output.array(to, 8, Some(0x80804AE2))?;
        for (&row, &target) in rows.iter().zip(&targets) {
            let node = self.condition(self.source.pointer(row)?, depth + 1)?;
            relative(self.output, target, node)?;
        }
        Ok(())
    }

    fn condition(&mut self, from: usize, depth: usize) -> Result<usize> {
        ensure!(
            depth <= 32 && self.nodes < 4096 && self.active.insert(from),
            "health condition tree exceeds limits or contains a cycle"
        );
        self.nodes += 1;
        let source = self.source;
        let (class, size, kind) =
            match source.u32(from.checked_sub(4).context("health condition prefix")?)? {
                0x80803F43 => (0x80804AD7, 152, 3),
                0x80803F45 => (0x80802A4A, 32, 1),
                0x80803F47 => (0x80804ADA, 24, 0),
                class => anyhow::bail!("health condition {class:08X} requires translation"),
            };
        ensure!(
            source.u64(from)? == kind,
            "health condition dispatch or flags differ"
        );
        let to = typed(self.output, class, size)?;
        put(&mut self.output.0, to, &kind.to_le_bytes())?;
        match kind {
            0 => self.conditions(from + 8, to + 8, depth)?,
            1 => {
                ensure!(
                    source.u64(from + 8)? == 0 && source.u32(from + 28)? == 0,
                    "health threshold expression extension requires translation"
                );
                let minimum = source.f32(from + 16)?;
                let maximum = source.f32(from + 20)?;
                ensure!(
                    minimum.is_finite()
                        && maximum.is_finite()
                        && minimum <= maximum
                        && (source.u32(from + 24)? as usize) < self.programs,
                    "health threshold bounds or expression index differ"
                );
                put(&mut self.output.0, to + 16, &source.bytes::<12>(from + 16)?)?;
            }
            3 => {
                filter::damage(source, from + 8, self.output, to + 8, self.resources)?;
                filter::ability(source, from + 0x80, self.output, to + 0x78, self.resources)?;
            }
            _ => unreachable!(),
        }
        self.active.remove(&from);
        Ok(to)
    }

    fn actions(&mut self, from: usize, to: usize) -> Result<()> {
        let rows = self.source.array(from, 8, Some(0x80803F4D))?;
        ensure!(
            rows.len() <= 256 && (!rows.is_empty() || self.source.u64(from + 8)? == 0),
            "health action list extent differs"
        );
        append_array(
            &mut self.output.0,
            to,
            0x80804AE0,
            &vec![0; rows.len() * 8],
            8,
        )?;
        let targets = self.output.array(to, 8, Some(0x80804AE0))?;
        for (&row, &field) in rows.iter().zip(&targets) {
            let at = self.source.pointer(row)?;
            let target = match self
                .source
                .u32(at.checked_sub(4).context("health action prefix")?)?
            {
                0x80803F37 => {
                    ensure!(
                        self.source.u32(at)? == 3 && self.source.f32(at + 4)?.is_finite(),
                        "health literal action differs"
                    );
                    let target = typed(self.output, 0x80804ACC, 8)?;
                    put(&mut self.output.0, target, &self.source.bytes::<8>(at)?)?;
                    target
                }
                0x80803F39 => {
                    ensure!(
                        self.source.u64(at)? == 1
                            && self.source.u64(at + 8)? == 0
                            && self.source.u32(at + 16)? == 0
                            && (self.source.u32(at + 20)? as usize) < self.programs,
                        "health multiplier expression or extension differs"
                    );
                    let target = typed(self.output, 0x80804ACE, 24)?;
                    put(&mut self.output.0, target, &1u64.to_le_bytes())?;
                    // Original consumers 2D7930 and D54CB0 select the expression
                    // through +14 and +10 respectively, despite equal type sizes.
                    put(
                        &mut self.output.0,
                        target + 16,
                        &self.source.bytes::<4>(at + 20)?,
                    )?;
                    target
                }
                class => anyhow::bail!("health rule action {class:08X} requires translation"),
            };
            relative(self.output, field, target)?;
        }
        Ok(())
    }
}

pub(super) fn emit(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    resources: &selectors::Resources,
    programs: usize,
) -> Result<usize> {
    let rows = source.array(from, 40, Some(0x8080405C))?;
    ensure!(
        rows.len() <= 256 && (!rows.is_empty() || source.u64(from + 8)? == 0),
        "health rule group extent differs"
    );
    let mut candidate = output.clone();
    append_array(
        &mut candidate.0,
        to,
        0x80804C08,
        &vec![0; rows.len() * 32],
        32,
    )?;
    let targets = candidate.array(to, 32, Some(0x80804C08))?;
    let mut writer = Writer {
        source,
        output: &mut candidate,
        resources,
        programs,
        active: BTreeSet::new(),
        nodes: 0,
    };
    for (&row, &target) in rows.iter().zip(&targets) {
        ensure!(
            source.u32(row)? == 0x811C9DC5 && source.u32(row + 4)? == 0,
            "named health rule groups require translation"
        );
        writer.conditions(row + 8, target, 0)?;
        writer.actions(row + 24, target + 16)?;
    }
    *output = candidate;
    Ok(rows.len())
}
