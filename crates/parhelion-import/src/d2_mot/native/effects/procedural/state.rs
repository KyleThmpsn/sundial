//! Allocation of vec4 history used by native channel interpolation procedures.
use super::*;

// Native channel-bank instance member at +0x60, confirmed by its type schema.
const HISTORY: u32 = 0xB6515162;
const VALUES: u32 = 0xFC2F3D6F;

pub(crate) fn interpolation_allocation(
    metadata: &mut Payload,
    before: usize,
    after: usize,
) -> Result<()> {
    ensure!(
        after >= before && after <= 64,
        "unsupported interpolation state capacity"
    );
    let rows = metadata.array(0x20, 40, Some(0x80808852))?;
    let found = rows
        .iter()
        .copied()
        .filter(|at| metadata.u32(*at).ok() == Some(HISTORY))
        .collect::<Vec<_>>();
    ensure!(found.len() <= 1, "duplicate interpolation state allocation");
    if let Some(&row) = found.first() {
        ensure!(
            metadata.u32(row + 16)? == 0x80800090
                && metadata.u32(row + 20)? as usize == before
                && metadata.u64(row + 24)? == 0,
            "native interpolation allocation contract differs"
        );
        return put(
            &mut metadata.0,
            row + 20,
            &u32::try_from(after)?.to_le_bytes(),
        );
    }
    ensure!(before == 0, "native interpolation allocation is missing");
    let templates = rows
        .iter()
        .copied()
        .filter(|at| metadata.u32(*at).ok() == Some(VALUES))
        .collect::<Vec<_>>();
    ensure!(
        templates.len() == 1,
        "native cached-vector allocation is ambiguous"
    );
    let template = templates[0];
    ensure!(
        metadata.u32(template + 16)? == 0x80800090 && metadata.u64(template + 24)? == 0,
        "native cached-vector allocation contract differs"
    );
    let mut data = Vec::new();
    for row in rows {
        // Copying nested descriptors would require pointer relocation.
        ensure!(
            metadata.u64(row + 24)? == 0,
            "nested channel allocation is unsupported"
        );
        data.extend_from_slice(&metadata.0[row..row + 40]);
        if row == template {
            let at = data.len();
            data.extend_from_slice(&metadata.0[row..row + 40]);
            put(&mut data, at, &HISTORY.to_le_bytes())?;
            put(&mut data, at + 20, &u32::try_from(after)?.to_le_bytes())?;
        }
    }
    append_array(&mut metadata.0, 0x20, 0x80808852, &data, 40)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_uses_private_vec4_allocation_and_preserves_existing_rows() {
        let mut p = Payload(vec![0; 0x40]);
        let mut row = vec![0; 40];
        put(&mut row, 0, &VALUES.to_le_bytes()).unwrap();
        put(&mut row, 16, &0x80800090u32.to_le_bytes()).unwrap();
        put(&mut row, 20, &7u32.to_le_bytes()).unwrap();
        append_array(&mut p.0, 0x20, 0x80808852, &row, 40).unwrap();
        interpolation_allocation(&mut p, 0, 3).unwrap();
        let rows = p.array(0x20, 40, Some(0x80808852)).unwrap();
        assert_eq!(&p.0[rows[0]..rows[0] + 40], row);
        assert_eq!(p.u32(rows[1]).unwrap(), HISTORY);
        assert_eq!(p.u32(rows[1] + 20).unwrap(), 3);
        assert!(interpolation_allocation(&mut p, 2, 4).is_err());
        interpolation_allocation(&mut p, 3, 5).unwrap();
        assert_eq!(p.u32(rows[1] + 20).unwrap(), 5);
        assert!(interpolation_allocation(&mut p, 5, 4).is_err());
    }
}
