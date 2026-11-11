use super::*;

pub(super) fn pointer_targets(
    data: &[u8],
    row: usize,
    rows_end: usize,
) -> AuthoringResult<Vec<(usize, usize)>> {
    let mut targets = Vec::new();
    for field in COLLECTIBLE_POINTER_FIELDS {
        let Some(target) = crate::tag_payload::nested_header(data, row + field, rows_end)? else {
            continue;
        };
        if target < rows_end {
            return Err(invalid("Collectible nested array points into fixed rows"));
        }
        targets.push((field, target));
    }
    Ok(targets)
}

pub(super) fn rebase_row_pointers(
    data: &mut [u8],
    row: usize,
    old_rows_end: usize,
    shift: usize,
) -> AuthoringResult<()> {
    for field in COLLECTIBLE_POINTER_FIELDS {
        let Some(target) = crate::tag_payload::nested_header(data, row + field, old_rows_end)?
        else {
            continue;
        };
        if target < old_rows_end {
            return Err(invalid(
                "Collectible pointer unexpectedly targets fixed rows",
            ));
        }
        write_relative_pointer(data, row + field + 8, target + shift)?;
    }
    Ok(())
}
