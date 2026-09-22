//! Keep mutable component arrays inside the instance span copied by the engine.
use super::*;

struct Layout {
    symbol: &'static str,
    class: u32,
    arrays: &'static [(usize, usize, u32)],
}

const LAYOUTS: &[Layout] = &[
    Layout {
        symbol: "owner",
        class: 0x808072B8,
        arrays: &[(0x120, 96, 0x80809788)],
    },
    Layout {
        symbol: "object-channels",
        class: 0x8080979F,
        arrays: &[
            (0x30, 88, 0x808097A7),
            (0x50, 16, 0x80800090),
            (0x60, 16, 0x80800090),
            (0x80, 8, 0x8080979D),
            (0x90, 16, 0x8080979C),
        ],
    },
];

fn bounds(p: &Payload, layout: &Layout) -> Result<(usize, usize, usize)> {
    let instance = p.pointer(16)?;
    let schema = p.pointer(24)?;
    ensure!(
        instance >= 4 && p.u32(instance - 4)? == layout.class && instance < schema,
        "{} component instance layout differs",
        layout.symbol
    );
    let mut end = instance;
    for &(field, stride, class) in layout.arrays {
        let rows = p.array(instance + field, stride, Some(class))?;
        if let Some(last) = rows.last() {
            ensure!(rows[0] >= instance, "mutable array precedes its instance");
            end = end.max(last + stride);
        }
    }
    Ok((instance, schema, end))
}

fn seal(p: &mut Payload, patches: &mut Vec<Value>, layout: &Layout) -> Result<Option<Value>> {
    let (instance, schema, end) = bounds(p, layout)?;
    if end <= schema {
        return Ok(None);
    }
    // The loader recalculates header 0x48 from these two pointers. Merely
    // increasing the serialized size is discarded during resource loading.
    // Preserve every existing tag-relative address by retaining the original
    // records and placing a second schema view after all mutable arrays. Copy
    // the complete record stream so relative references into its earlier
    // records retain their meaning. File-header bytes are not typed records.
    let first = p
        .pointer(8)?
        .checked_sub(4)
        .context("component record start")?;
    ensure!(
        first >= 0x60 && first < instance && p.u32(first)? & 0xFFFF0000 == 0x80800000,
        "component record stream differs"
    );
    let original_size = p.0.len();
    let delta = original_size
        .checked_add(15)
        .context("component size overflow")?
        & !15;
    let new_schema = delta
        .checked_add(schema)
        .context("component schema overflow")?;
    let copy = p.0[first..].to_vec();
    p.0.resize(delta + first, 0);
    p.0.extend(copy);
    let mut copied_patches = vec![];
    for patch in patches.iter() {
        let offset = usize::try_from(patch["offset"].as_u64().context("component patch offset")?)?;
        ensure!(
            offset
                .checked_add(4)
                .is_some_and(|end| end <= original_size),
            "component patch exceeds original payload"
        );
        if offset >= first {
            let mut patch = patch.clone();
            patch["offset"] = json!(
                offset
                    .checked_add(delta)
                    .context("component patch overflow")?
            );
            copied_patches.push(patch);
        }
    }
    patches.extend(copied_patches);
    put(&mut p.0, 24, &((new_schema - 24) as u64).to_le_bytes())?;
    put(
        &mut p.0,
        0x48,
        &((new_schema - instance) as u64).to_le_bytes(),
    )?;
    let length = p.0.len() as u64;
    put(&mut p.0, 0, &length.to_le_bytes())?;
    let (_, after, required) = bounds(p, layout)?;
    ensure!(
        required <= after,
        "mutable component data exceeds runtime span"
    );
    Ok(Some(json!({
        "symbol":layout.symbol,"instance":instance,"schema_before":schema,
        "schema_after":after,"instance_data_end":required,"original_size":original_size,
        "copied_record_start":first,"copy_delta":delta
    })))
}

pub(super) fn finish(graph: &mut Graph) -> Result<()> {
    let mut changed = vec![];
    for layout in LAYOUTS {
        if graph.node(layout.symbol).is_err() {
            continue;
        }
        let mut payload = graph.read(layout.symbol)?;
        let mut patches = graph.node(layout.symbol)?["patches"]
            .as_array()
            .context("component patches")?
            .clone();
        if let Some(evidence) = seal(&mut payload, &mut patches, layout)? {
            graph.write(layout.symbol, &payload.0)?;
            graph.node_mut(layout.symbol)?["patches"] = json!(patches);
            changed.push(evidence);
        }
    }
    if !changed.is_empty() {
        graph.manifest["instance_layout"] = json!(changed);
    }
    if graph.node("object-channels").is_ok() {
        crate::d2_mot::audit::channels::interpolation(
            &graph.read("object-channels")?,
            &graph.read("object-channel-allocation")?,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Payload {
        let mut p = Payload(vec![0; 0x240]);
        put(&mut p.0, 8, &0x60u64.to_le_bytes()).unwrap();
        put(&mut p.0, 16, &0x70u64.to_le_bytes()).unwrap();
        put(&mut p.0, 24, &0x1E8u64.to_le_bytes()).unwrap();
        put(&mut p.0, 0x48, &0x180u64.to_le_bytes()).unwrap();
        put(&mut p.0, 0x64, &0x8080978Fu32.to_le_bytes()).unwrap();
        put(&mut p.0, 0x7C, &0x8080979Fu32.to_le_bytes()).unwrap();
        put(&mut p.0, 0x1FC, &0x80809790u32.to_le_bytes()).unwrap();
        put(&mut p.0, 0x218, &(-0x198i64).to_le_bytes()).unwrap();
        append_array(&mut p.0, 0xD0, 0x80800090, &[0xAB; 32], 16).unwrap();
        p
    }

    #[test]
    fn runtime_copy_contains_appended_rows_and_schema_references_survive() {
        let mut p = fixture();
        let before = p.clone();
        let mut patches = vec![
            json!({"offset":68,"symbol":"allocation"}),
            json!({"offset":0x80,"symbol":"bank"}),
        ];
        let proof = seal(&mut p, &mut patches, &LAYOUTS[1]).unwrap().unwrap();
        let delta = proof["copy_delta"].as_u64().unwrap() as usize;
        let (instance, schema, end) = bounds(&p, &LAYOUTS[1]).unwrap();
        assert!(end <= schema);
        assert_eq!(&p.0[0x60..before.0.len()], &before.0[0x60..]);
        assert_eq!(&p.0[delta + 0x64..], &before.0[0x64..]);
        assert!(p.0[before.0.len()..delta + 0x64].iter().all(|b| *b == 0));
        assert_eq!(p.pointer(schema + 0x18).unwrap(), delta + instance);
        // Simulate the engine's memcpy, using its recalculated boundary.
        let copied_instance = Payload(p.0[instance..schema].to_vec());
        let rows = copied_instance.array(0x50, 16, Some(0x80800090)).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(&copied_instance.0[rows[0]..rows[1] + 16], &[0xAB; 32]);
        assert_eq!(patches.len(), 3);
        assert_eq!(patches[2]["offset"], delta + 0x80);
        assert!(seal(&mut p, &mut patches, &LAYOUTS[1]).unwrap().is_none());
    }

    #[test]
    fn size_field_alone_does_not_repair_the_runtime_boundary() {
        let mut p = fixture();
        let size = p.0.len() as u64;
        put(&mut p.0, 0x48, &size.to_le_bytes()).unwrap();
        let (instance, schema, end) = bounds(&p, &LAYOUTS[1]).unwrap();
        assert!(end > schema);
        let copied = Payload(p.0[instance..schema].to_vec());
        assert!(copied.array(0x50, 16, Some(0x80800090)).is_err());
    }
}
