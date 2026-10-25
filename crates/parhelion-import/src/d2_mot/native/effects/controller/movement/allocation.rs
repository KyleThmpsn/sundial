//! Allocation metadata derived from the validated movement object tree.
use super::{Curve, Payload, Slot, Tree, put};
use crate::d2_mot::native::effects::controller::allocation::Row;
use anyhow::{Context, Result};

pub(super) fn build(source: &Payload, tree: &Tree, template: &Payload) -> Result<Payload> {
    let mut output = Payload(
        template
            .0
            .get(..48)
            .context("movement allocation header")?
            .to_vec(),
    );
    let mut rows = Vec::new();
    if !tree.blocks.is_empty() {
        rows.push(Row::named(
            0x4F12DF5B,
            u32::MAX,
            0,
            vec![Row::named(
                0x1D7C9056,
                0x8080851B,
                tree.blocks.len(),
                Row::elements(
                    tree.blocks
                        .iter()
                        .map(|block| Row::input(0x45DEE76F, block.expression.inputs.len()))
                        .collect(),
                ),
            )],
        ));
    }
    if !tree.slots.is_empty() {
        rows.push(Row::named(
            0x5680393D,
            0x808037CC,
            tree.slots.len(),
            Row::elements(tree.slots.iter().map(slot).collect()),
        ));
    }
    rows.push(Row::named(
        0x455355D5,
        0x808089F7,
        tree.scalars.len(),
        Row::elements(
            tree.scalars
                .iter()
                .map(|expression| Row::inputs(expression.inputs.len()))
                .collect(),
        ),
    ));
    rows.push(states(source, tree)?);
    Row::write(&rows, &mut output, 0x20)?;
    let len = output.0.len() as u64;
    put(&mut output.0, 0, &len.to_le_bytes())?;
    Ok(output)
}

fn slot(slot: &Slot) -> Row {
    Row::named(
        0x811C9DC5,
        u32::MAX,
        0,
        if slot.curves.is_empty() {
            Vec::new()
        } else {
            vec![Row::named(
                0x5B2D36EB,
                0x808037CF,
                slot.curves.len(),
                Row::elements(slot.curves.iter().map(curve).collect()),
            )]
        },
    )
}

fn curve(curve: &Curve) -> Row {
    Row::named(
        0x811C9DC5,
        u32::MAX,
        0,
        if curve.expression.inputs.is_empty() {
            Vec::new()
        } else {
            // The curve owns its data object, which in turn owns the expression.
            vec![Row::named(
                0x4137EF97,
                u32::MAX,
                0,
                vec![Row::indirect(
                    0xC214E9FF,
                    Row::inputs(curve.expression.inputs.len()),
                )],
            )]
        },
    )
}

fn states(source: &Payload, tree: &Tree) -> Result<Row> {
    let elements = tree
        .states
        .iter()
        .map(|state| {
            Ok(Row::named(
                0x811C9DC5,
                u32::MAX,
                0,
                if source.u32(state.at + 0x14)? == 0x808029B8 {
                    vec![Row::named(0x0E0E736E, 0x808037BD, 1, Vec::new())]
                } else {
                    Vec::new()
                },
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Row::named(
        0x7D1F1C58,
        0x808037BA,
        tree.states.len(),
        Row::elements(elements),
    ))
}
