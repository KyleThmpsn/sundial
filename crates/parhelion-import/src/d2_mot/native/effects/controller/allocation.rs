//! Native allocation rows, child arrays and separately tagged indirect nodes.
use super::{Payload, append_array, put};
use anyhow::{Context, Result};

const CLASS: u32 = 0x80808852;
const ROW_SIZE: usize = 40;

pub(super) struct Row {
    name: u32,
    class: u32,
    count: usize,
    children: Vec<Row>,
    target: Option<Box<Row>>,
}

impl Row {
    pub(super) fn named(name: u32, class: u32, count: usize, children: Vec<Self>) -> Self {
        Self {
            name,
            class,
            count,
            children,
            target: None,
        }
    }

    /// An embedded expression's input array is reached through a separate allocation node.
    pub(super) fn input(leaf: u32, count: usize) -> Self {
        let children = if count == 0 {
            Vec::new()
        } else {
            vec![Self::indirect(leaf, Self::inputs(count))]
        };
        Self::named(0x811C9DC5, u32::MAX, 0, children)
    }

    pub(super) fn indirect(name: u32, target: Self) -> Self {
        let mut row = Self::named(name, u32::MAX, 0, Vec::new());
        row.target = Some(Box::new(target));
        row
    }

    pub(super) fn inputs(count: usize) -> Self {
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
    pub(super) fn elements(rows: Vec<Self>) -> Vec<Self> {
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

    pub(super) fn write(rows: &[Self], output: &mut Payload, field: usize) -> Result<()> {
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
