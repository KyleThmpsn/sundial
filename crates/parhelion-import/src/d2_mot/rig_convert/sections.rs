//! Preserve the source rig's animation sections and their bind transforms.
use super::*;

pub(super) fn convert(source: &Payload, native: &mut Payload) -> Result<usize> {
    let si = source.pointer(16)?;
    let ni = native.pointer(16)?;
    let sr = source.pointer(24)?;
    let nr = native.pointer(24)?;
    ensure!(si >= 4 && ni >= 4, "FK instance has no type header");
    let sections = source.array(si + 0x30, 64, Some(0x808081DC))?;
    let targets = native.array(ni + 0x30, 64, Some(0x80808544))?;
    let schemas = source.array(sr + 0xF8, 24, Some(0x808081E1))?;
    let target_schemas = native.array(nr + 0xD8, 24, Some(0x80808549))?;
    ensure!(
        sections.len() == targets.len()
            && sections.len() == schemas.len()
            && sections.len() == target_schemas.len(),
        "FK section topology requires a different native component envelope"
    );
    let ranges = source.array_range(si + 0x40, 8, Some(0x80808640))?;
    ensure!(
        ranges.len() / 8 == sections.len(),
        "FK section range count differs"
    );
    ensure!(
        native.array(ni + 0x40, 8, Some(0x80808A06))?.len() == sections.len(),
        "native FK section range count differs"
    );
    if sections.is_empty() {
        return Ok(0);
    }
    ensure!(
        source.u32(si - 4)? == 0x808081DD && native.u32(ni - 4)? == 0x80808545,
        "FK section instance type differs"
    );
    for ((&row, &target), (&schema, &target_schema)) in sections
        .iter()
        .zip(&targets)
        .zip(schemas.iter().zip(&target_schemas))
    {
        // Preserve the envelope's typed references, including their absolute
        // offsets. Each source section must form the same bidirectional pair.
        ensure!(
            source.u32(row + 4)? == 0x808081E1
                && source.u64(row + 8)? == schema as u64
                && source.u32(schema + 4)? == 0x808081DC
                && source.u64(schema + 8)? == row as u64
                && source.pointer(row + 16)? == si,
            "source FK section binding differs"
        );
        ensure!(
            native.u32(target + 4)? == 0x80808549
                && native.u64(target + 8)? == target_schema as u64
                && native.u32(target_schema + 4)? == 0x80808544
                && native.u64(target_schema + 8)? == target as u64
                && native.pointer(target + 16)? == ni,
            "native FK section binding differs"
        );
        let transforms = source.array_range(row + 32, 32, Some(0x80809F4F))?;
        ensure!(
            transforms.len() / 32 == source.u32(schema + 16)? as usize,
            "FK section bind-transform count differs"
        );
        let bytes = &source.0[transforms];
        ensure!(
            bytes
                .chunks_exact(4)
                .all(|b| f32::from_le_bytes(b.try_into().unwrap()).is_finite()),
            "FK section has a nonfinite bind transform"
        );
        write_array(
            &mut native.0,
            target + 32,
            0x80809F75,
            bytes.len() / 32,
            bytes,
        )?;
        native.0[target + 24..target + 32].copy_from_slice(&source.0[row + 24..row + 32]);
        native.0[target + 48..target + 64].copy_from_slice(&source.0[row + 48..row + 64]);
        native.0[target_schema + 16..target_schema + 24]
            .copy_from_slice(&source.0[schema + 16..schema + 24]);
    }
    write_array(
        &mut native.0,
        ni + 0x40,
        0x80808A06,
        sections.len(),
        &source.0[ranges],
    )?;
    Ok(sections.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(modern: bool, counts: [usize; 2]) -> Payload {
        let mut data = vec![0; 0x400];
        let instance = 0x80usize;
        let resource = 0x180usize;
        data[16..24].copy_from_slice(&((instance - 16) as u64).to_le_bytes());
        data[24..32].copy_from_slice(&((resource - 24) as u64).to_le_bytes());
        let (instance_class, section_class, schema_class, transform_class, range_class) = if modern
        {
            (
                0x808081DDu32,
                0x808081DC,
                0x808081E1,
                0x80809F4F,
                0x80808640,
            )
        } else {
            (0x80808545, 0x80808544, 0x80808549, 0x80809F75, 0x80808A06)
        };
        data[instance - 4..instance].copy_from_slice(&instance_class.to_le_bytes());
        write_array(&mut data, instance + 0x30, section_class, 2, &[0; 128]).unwrap();
        let descriptor = resource + if modern { 0xF8 } else { 0xD8 };
        write_array(&mut data, descriptor, schema_class, 2, &[0; 48]).unwrap();
        write_array(&mut data, instance + 0x40, range_class, 2, &[0; 16]).unwrap();
        let p = Payload(data.clone());
        let rows = p.array(instance + 0x30, 64, None).unwrap();
        let schemas = p.array(descriptor, 24, None).unwrap();
        for (i, (&row, &schema)) in rows.iter().zip(&schemas).enumerate() {
            data[row + 4..row + 8].copy_from_slice(&schema_class.to_le_bytes());
            data[row + 8..row + 16].copy_from_slice(&(schema as u64).to_le_bytes());
            data[row + 16..row + 24]
                .copy_from_slice(&(instance as i64 - row as i64 - 16).to_le_bytes());
            data[schema + 4..schema + 8].copy_from_slice(&section_class.to_le_bytes());
            data[schema + 8..schema + 16].copy_from_slice(&(row as u64).to_le_bytes());
            data[schema + 16..schema + 20].copy_from_slice(&(counts[i] as u32).to_le_bytes());
            let bytes = vec![if modern { 0.25f32 } else { 0.5f32 }; counts[i] * 8]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>();
            write_array(&mut data, row + 32, transform_class, counts[i], &bytes).unwrap();
        }
        Payload(data)
    }

    #[test]
    fn sections_keep_native_connections_and_source_bind_transforms() {
        let source = fixture(true, [3, 2]);
        let mut native = fixture(false, [1, 1]);
        let rows = native.array(0xB0, 64, Some(0x80808544)).unwrap();
        let connections = rows
            .iter()
            .map(|r| native.0[*r..*r + 24].to_vec())
            .collect::<Vec<_>>();
        assert_eq!(convert(&source, &mut native).unwrap(), 2);
        for (i, row) in rows.into_iter().enumerate() {
            assert_eq!(native.0[row..row + 24], connections[i]);
            let transforms = native.array(row + 32, 32, Some(0x80809F75)).unwrap();
            assert_eq!(transforms.len(), [3, 2][i]);
            assert!(transforms.iter().all(|at| native.f32(*at).unwrap() == 0.25));
        }
        let mut invalid = source;
        let row = invalid.array(0xB0, 64, None).unwrap()[1];
        invalid.0[row + 8..row + 16].fill(0);
        assert!(convert(&invalid, &mut native).is_err());
    }
}
