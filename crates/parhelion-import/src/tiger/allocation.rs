//! Native allocation rows, child arrays and separately tagged indirect nodes.
use super::payload::Payload;
use crate::presentation::{append as append_array, put};
use anyhow::{Context, Result};

const CLASS: u32 = 0x80808852;
const ROW_SIZE: usize = 40;

pub(crate) struct Row {
    name: u32,
    class: u32,
    count: usize,
    children: Vec<Row>,
    target: Option<Box<Row>>,
}

impl Row {
    pub(crate) fn named(name: u32, class: u32, count: usize, children: Vec<Self>) -> Self {
        Self {
            name,
            class,
            count,
            children,
            target: None,
        }
    }

    /// An embedded expression's input array is reached through a separate allocation node.
    pub(crate) fn input(leaf: u32, count: usize) -> Self {
        let children = if count == 0 {
            Vec::new()
        } else {
            vec![Self::indirect(leaf, Self::inputs(count))]
        };
        Self::named(0x811C9DC5, u32::MAX, 0, children)
    }

    pub(crate) fn indirect(name: u32, target: Self) -> Self {
        let mut row = Self::named(name, u32::MAX, 0, Vec::new());
        row.target = Some(Box::new(target));
        row
    }

    pub(crate) fn inputs(count: usize) -> Self {
        Self::named(
            0x811C9DC5,
            u32::MAX,
            0,
            if count == 0 {
                Vec::new()
            } else {
                vec![Self::named(0x1D7C9056, 0x80809788, count, Vec::new())]
            },
        )
    }

    /// Keep positional rows only when an element needs a nested allocation.
    pub(crate) fn elements(rows: Vec<Self>) -> Vec<Self> {
        if rows.iter().all(|row| {
            row.name == 0x811C9DC5
                && row.class == u32::MAX
                && row.count == 0
                && row.target.is_none()
                && row.children.is_empty()
        }) {
            Vec::new()
        } else {
            rows
        }
    }

    pub(crate) fn write(rows: &[Self], output: &mut Payload, field: usize) -> Result<()> {
        let size = rows
            .len()
            .checked_mul(ROW_SIZE)
            .context("allocation array size overflow")?;
        append_array(
            &mut output.0,
            field,
            u64::from(CLASS),
            &vec![0; size],
            ROW_SIZE,
        )?;
        let written = output.array(field, ROW_SIZE, Some(CLASS))?;
        for (&at, row) in written.iter().zip(rows) {
            row.write_at(output, at)?;
        }
        Ok(())
    }

    /// Write `rows` first, then keep every existing top-level row whose name is not in
    /// `replaced`. Kept rows' child arrays and indirect targets stay where they are, so their
    /// relative pointers are rebased to the rows' new positions.
    pub(crate) fn replace(
        rows: &[Self],
        output: &mut Payload,
        field: usize,
        replaced: &[u32],
    ) -> Result<()> {
        let retained = output
            .array(field, ROW_SIZE, Some(CLASS))?
            .into_iter()
            .filter(|&at| output.u32(at).is_ok_and(|name| !replaced.contains(&name)))
            .collect::<Vec<_>>();
        let old = retained
            .iter()
            .map(|&at| output.bytes::<ROW_SIZE>(at))
            .collect::<Result<Vec<_>>>()?;
        Self::write(rows, output, field)?;
        let mut joined = output
            .array(field, ROW_SIZE, Some(CLASS))?
            .into_iter()
            .map(|at| Ok((at, output.bytes::<ROW_SIZE>(at)?)))
            .collect::<Result<Vec<_>>>()?;
        joined.extend(retained.into_iter().zip(old));
        append_array(
            &mut output.0,
            field,
            u64::from(CLASS),
            &vec![0; joined.len() * ROW_SIZE],
            ROW_SIZE,
        )?;
        let written = output.array(field, ROW_SIZE, Some(CLASS))?;
        for (&to, (from, bytes)) in written.iter().zip(joined) {
            put(&mut output.0, to, &bytes)?;
            for delta in [8, 32] {
                let offset = i64::from_le_bytes(bytes[delta..delta + 8].try_into()?);
                if offset != 0 {
                    let target = i64::try_from(from + delta)? + offset;
                    let relative = target - i64::try_from(to + delta)?;
                    put(&mut output.0, to + delta, &relative.to_le_bytes())?;
                }
            }
        }
        Ok(())
    }

    fn write_at(&self, output: &mut Payload, at: usize) -> Result<()> {
        put(&mut output.0, at, &self.name.to_le_bytes())?;
        put(&mut output.0, at + 16, &self.class.to_le_bytes())?;
        put(
            &mut output.0,
            at + 20,
            &u32::try_from(self.count)?.to_le_bytes(),
        )?;
        Self::write(&self.children, output, at + 24)?;
        if let Some(target) = &self.target {
            // Native standalone rows have a class marker immediately before their
            // eight-byte-aligned payload. The +8 field is a relative pointer.
            let start = output
                .0
                .len()
                .checked_add(11)
                .context("allocation node alignment overflow")?
                & !7;
            let end = start
                .checked_add(ROW_SIZE)
                .context("allocation node size overflow")?;
            output.0.resize(end, 0);
            put(&mut output.0, start - 4, &CLASS.to_le_bytes())?;
            let delta = i64::try_from(start)? - i64::try_from(at + 8)?;
            put(&mut output.0, at + 8, &delta.to_le_bytes())?;
            target.write_at(output, start)?;
        }
        Ok(())
    }
}
