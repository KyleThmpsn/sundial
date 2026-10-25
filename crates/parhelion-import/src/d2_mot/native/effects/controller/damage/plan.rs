//! Validate expression storage and plan record and array emission order.
use super::*;

pub(super) struct Plan {
    pub lifecycle: Array,
    pub arrays: BTreeMap<usize, Array>,
    pub descriptors: BTreeMap<usize, usize>,
    pub inputs: BTreeMap<usize, usize>,
}

impl Plan {
    pub(super) fn read(
        source: &Payload,
        source_records: &BTreeMap<usize, Record>,
        si: usize,
    ) -> Result<Self> {
        let lifecycle =
            array(source, 0x40, 0x8080907C, 16)?.context("damage lifecycle provider missing")?;
        ensure!(
            lifecycle.count == 1,
            "damage lifecycle provider shape differs"
        );
        let mut arrays = BTreeMap::from([(lifecycle.header, lifecycle.clone())]);
        let mut descriptors = BTreeMap::new();
        let mut inputs = BTreeMap::new();
        for record in source_records
            .values()
            .filter(|r| matches!(r.class, 0x80808631 | 0x8080862F))
        {
            let at = record.at;
            let runtime = record.twin;
            ensure!(
                source_records
                    .get(&runtime)
                    .is_some_and(|r| r.class == record.class + 1),
                "damage expression reciprocal class differs"
            );
            ensure!(
                source.u64(runtime + 24)? == 0,
                "damage expression runtime is initialized"
            );
            let parent = source.pointer(runtime + 16)?;
            ensure!(
                parent == si
                    || matches!(
                        source_records.get(&parent).map(|r| r.class),
                        Some(0x8080295A | 0x80802958)
                    ),
                "damage expression parent differs"
            );
            let ds = source.array(at + 64, 40, Some(0x80809591))?;
            let ir = source.array(runtime + 32, 48, Some(0x80809590))?;
            ensure!(
                ds.len() == ir.len()
                    && ds
                        .iter()
                        .zip(&ir)
                        .all(|(&d, &i)| source.u64(d + 8).ok() == Some(i as u64)),
                "damage input runtime order differs"
            );
            ensure!(
                source.u64(at + 48)? == (5 + ds.len()) as u64 && source.u64(at + 56)? == 1,
                "damage builtin input or output count differs"
            );
            if record.class == 0x8080862F {
                ensure!(
                    source.u64(at + 80)? == 0 && source.u64(at + 88)? == 0x811C9DC5,
                    "damage reference expression needs external linkage"
                );
            }
            inputs.insert(at, ds.len());
            let mut constants = super::super::array_bytes(source, at + 32, 16)?;
            let code = super::super::array_bytes(source, at + 16, 1)?;
            let lowered = procedural::lower_program(&code, constants.len() / 16, 5 + ds.len())?;
            super::super::constants::broadcast(&code, &mut constants);
            for (field, class, stride, data) in [
                (at + 16, 0x80800009, 1, Some(lowered)),
                (at + 32, 0x80800090, 16, Some(constants)),
                (at + 64, 0x80809591, 40, None),
                (runtime + 32, 0x80809590, 48, None),
            ] {
                if let Some(mut row) = array(source, field, class, stride)? {
                    if let Some(data) = data {
                        row.data = data;
                    }
                    descriptors.insert(field, row.header);
                    ensure!(
                        arrays.insert(row.header, row).is_none(),
                        "shared damage array requires alias support"
                    );
                }
            }
        }
        let mut end = 0;
        for row in arrays.values() {
            ensure!(row.header >= end, "overlapping damage arrays");
            end = row
                .header
                .checked_add(16)
                .and_then(|x| {
                    row.count
                        .checked_mul(row.stride)
                        .and_then(|n| x.checked_add(n))
                })
                .context("damage array extent overflow")?;
        }
        Ok(Self {
            lifecycle,
            arrays,
            descriptors,
            inputs,
        })
    }
}

pub(super) fn events(
    source_records: &BTreeMap<usize, Record>,
    arrays: &BTreeMap<usize, Array>,
    si: usize,
    sd: usize,
) -> Result<BTreeMap<usize, Event>> {
    let mut events = BTreeMap::new();
    for record in source_records.values() {
        events.insert(record.at, Event::Record(*record));
    }
    for row in arrays.values() {
        ensure!(
            events
                .insert(row.header, Event::Array(row.clone()))
                .is_none(),
            "damage record overlaps array header"
        );
    }
    for (at, event) in [
        (si + 0x90, Event::Conditions),
        (si + 0x100, Event::State),
        (sd + 0x1C0, Event::Tail),
    ] {
        ensure!(
            events.insert(at, event).is_none(),
            "damage inline field overlap"
        );
    }
    Ok(events)
}
