//! Preserve the native sparse metadata pair when a weapon has no metadata row.
use super::*;

pub(super) fn append(
    data: &mut Vec<u8>,
    index: &mut Vec<u8>,
    donor_hash: u32,
    authored_hash: u32,
    patches: &[WeaponRawPayloadPatch],
) -> AuthoringResult<()> {
    let presence =
        classify_keyed_auxiliary_donor(data, index, donor_hash, authored_hash, METADATA_LAYOUT)?;
    if matches!(presence, KeyedAuxiliaryDonorPresence::Absent) {
        if patches.iter().any(|patch| {
            matches!(
                patch.target,
                WeaponRawPayloadTarget::ItemMetadataRow
                    | WeaponRawPayloadTarget::ItemMetadataIndexRow
            )
        }) {
            return Err(invalid(
                "The gameplay donor has no item metadata row to patch",
            ));
        }
    } else {
        (*data, *index) = append_keyed_auxiliary_pair(
            std::mem::take(data),
            std::mem::take(index),
            donor_hash,
            authored_hash,
            METADATA_LAYOUT,
        )?;
        let row = validate_keyed_auxiliary_alignment(data, index, METADATA_LAYOUT)?.count - 1;
        apply_array_row_raw_payload_patches(
            data,
            8,
            row,
            ITEM_METADATA_ROW_SIZE,
            ITEM_METADATA_ROW_CLASS,
            WeaponRawPayloadTarget::ItemMetadataRow,
            patches,
        )?;
        apply_array_row_raw_payload_patches(
            index,
            8,
            row,
            ITEM_METADATA_INDEX_ROW_SIZE,
            ITEM_METADATA_INDEX_ROW_CLASS,
            WeaponRawPayloadTarget::ItemMetadataIndexRow,
            patches,
        )?;
    }
    validate_keyed_auxiliary_structure(data, index, METADATA_LAYOUT)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (Vec<u8>, Vec<u8>) {
        let mut data = vec![0; 0x80];
        write_u64(&mut data, 8, 1).unwrap();
        write_relative_pointer(&mut data, 16, 0x30).unwrap();
        write_u64(&mut data, 0x30, 1).unwrap();
        write_u32(&mut data, 0x38, ITEM_METADATA_ROW_CLASS).unwrap();
        write_u32(&mut data, 0x40, 42).unwrap();
        write_relative_pointer(&mut data, 0x20, 0x60).unwrap();
        write_u32(&mut data, 0x68, ITEM_METADATA_SECONDARY_CLASS).unwrap();
        let mut index = vec![0; 0x34];
        write_u64(&mut index, 8, 1).unwrap();
        write_relative_pointer(&mut index, 16, 0x20).unwrap();
        write_u64(&mut index, 0x20, 1).unwrap();
        write_u32(&mut index, 0x28, ITEM_METADATA_INDEX_ROW_CLASS).unwrap();
        write_u32(&mut index, 0x30, 42).unwrap();
        (data, index)
    }

    #[test]
    fn absent_metadata_preserves_both_tables_and_rejects_missing_row_patches() {
        let (mut data, mut index) = pair();
        let before = (data.clone(), index.clone());
        append(&mut data, &mut index, 43, 44, &[]).unwrap();
        assert_eq!((&data, &index), (&before.0, &before.1));
        for target in [
            WeaponRawPayloadTarget::ItemMetadataRow,
            WeaponRawPayloadTarget::ItemMetadataIndexRow,
        ] {
            let patch = WeaponRawPayloadPatch {
                target,
                offset: 4,
                bytes: vec![1],
            };
            let error = append(&mut data, &mut index, 43, 44, &[patch]).unwrap_err();
            assert!(error.to_string().contains("no item metadata row to patch"));
            assert_eq!((&data, &index), (&before.0, &before.1));
        }
    }

    #[test]
    fn sparse_metadata_still_rejects_mismatched_keys_and_existing_identity() {
        let (mut data, mut index) = pair();
        assert!(append(&mut data, &mut index, 43, 42, &[]).is_err());
        write_u32(&mut index, 0x30, 99).unwrap();
        assert!(append(&mut data, &mut index, 43, 44, &[]).is_err());
    }

    #[test]
    fn present_metadata_clones_and_patches_only_the_new_row() {
        let (mut data, mut index) = pair();
        let original_row = data[0x40..0x60].to_vec();
        let patch = WeaponRawPayloadPatch {
            target: WeaponRawPayloadTarget::ItemMetadataRow,
            offset: 4,
            bytes: vec![7],
        };
        append(&mut data, &mut index, 42, 44, &[patch]).unwrap();
        let rows = validate_keyed_auxiliary_structure(&data, &index, METADATA_LAYOUT).unwrap();
        assert_eq!(rows.count, 2);
        assert_eq!(&data[0x40..0x60], original_row);
        assert_eq!(read_u32(&data, 0x60).unwrap(), 44);
        assert_eq!(data[0x64], 7);
        assert_eq!(read_u32(&index, 0x34).unwrap(), 44);
    }
}
