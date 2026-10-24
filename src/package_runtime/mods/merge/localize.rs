//! Objects the added bytes of a recipe table point at, copied after the entry.
//!
//! Sunrise's merge moves the bytes a mod appended after the stock entry, and rebases only the
//! pointers inside them. An element appended there that points at an object in the stock region,
//! such as an added arrangement row's holder pointing at its donor's group, would point elsewhere
//! once moved, so Sunrise Studio refuses to describe such an entry and the merge reads it as
//! opaque. Each such object is copied, with the arrays it owns, behind the entry, and the element
//! points at the copy. The copy holds the same bytes, so the entry reads the same in game.
use super::layout::{ARRAY_MARKER, HEADER_SIZE, MARKER_BACK, i64_at, put, u32_at, u64_at};
use super::recipes::{Field, Recipe, Shape};
use super::sidecar::FieldKind;
use std::collections::BTreeMap;

/// An object's class word sits just before it.
const CLASS_BACK: usize = 4;

/// `bytes` with every object an appended element points at in the first `stock_size` bytes copied
/// behind it, or `None` when no element does or the entry isn't in the recipe's shape.
pub fn localize(stock_size: usize, bytes: &[u8], recipe: &Recipe) -> Option<Vec<u8>> {
    let mut slots = Vec::new();
    for array in recipe.arrays {
        let (count, header) = array_at(bytes, array.descriptor)?;
        for row in 0..count {
            let start = header + HEADER_SIZE + row * array.row.stride;
            collect(bytes, stock_size, start, array.row, &mut slots)?;
        }
    }
    if slots.is_empty() {
        return None;
    }
    let mut output = bytes.to_vec();
    let mut copies = BTreeMap::new();
    for (slot, target, shape) in slots {
        let copy = match copies.get(&target) {
            Some(&copy) => copy,
            None => {
                let copy = copy_object(&mut output, stock_size, target, shape)?;
                copies.insert(target, copy);
                copy
            }
        };
        point(&mut output, slot, copy)?;
    }
    Some(output)
}

/// The count and header of the array whose descriptor sits at `at`, with a count of 0 and no
/// header for an empty one. `None` when the descriptor isn't a typed array.
fn array_at(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    let count = usize::try_from(u64_at(bytes, at)?).ok()?;
    let relative = i64_at(bytes, at + 8)?;
    if relative == 0 {
        return (count == 0).then_some((0, 0));
    }
    let header = (at + 8).checked_add_signed(isize::try_from(relative).ok()?)?;
    let fits = header >= MARKER_BACK
        && header.checked_add(HEADER_SIZE)? <= bytes.len()
        && u32_at(bytes, header - MARKER_BACK) == Some(ARRAY_MARKER)
        && u64_at(bytes, header) == Some(count as u64);
    fits.then_some((count, header))
}

/// The target of the self-relative pointer at `at`, or `None` for a null one.
fn target_of(bytes: &[u8], at: usize) -> Option<Option<usize>> {
    let relative = i64_at(bytes, at)?;
    if relative == 0 {
        return Some(None);
    }
    let target = at.checked_add_signed(isize::try_from(relative).ok()?)?;
    (target < bytes.len()).then_some(Some(target))
}

/// Notes each pointer from an element appended after the stock region to an object inside it,
/// through the arrays the row at `start` owns.
fn collect(
    bytes: &[u8],
    stock_size: usize,
    start: usize,
    shape: &'static Shape,
    slots: &mut Vec<(usize, usize, &'static Shape)>,
) -> Option<()> {
    for field in shape.fields {
        let (FieldKind::OwnedArray, Some(element)) = (field.kind, field.element) else {
            continue;
        };
        let (count, header) = array_at(bytes, start + field.offset)?;
        if count == 0 || field.element_stride == 0 {
            continue;
        }
        for index in 0..count {
            let element_start = header + HEADER_SIZE + index * field.element_stride;
            if header >= stock_size {
                note_locals(bytes, stock_size, element_start, element, slots)?;
            }
            collect(bytes, stock_size, element_start, element, slots)?;
        }
    }
    Some(())
}

/// Notes each object pointer of the appended element at `start` whose target is in the stock
/// region.
fn note_locals(
    bytes: &[u8],
    stock_size: usize,
    start: usize,
    shape: &'static Shape,
    slots: &mut Vec<(usize, usize, &'static Shape)>,
) -> Option<()> {
    for local in shape.local {
        let slot = start + local.offset;
        if let Some(target) = target_of(bytes, slot)?
            && target < stock_size
        {
            slots.push((slot, target, local.target));
        }
    }
    Some(())
}

/// Appends the object at `target`, behind its class word, and copies of the stock arrays it owns.
/// Returns where the copy starts.
fn copy_object(
    output: &mut Vec<u8>,
    stock_size: usize,
    target: usize,
    shape: &Shape,
) -> Option<usize> {
    let class = output
        .get(target.checked_sub(CLASS_BACK)?..target)?
        .to_vec();
    let end = target.checked_add(shape.stride)?;
    if end > stock_size {
        return None;
    }
    let object = output[target..end].to_vec();
    let copy = (output.len() + CLASS_BACK).next_multiple_of(16);
    output.resize(copy - CLASS_BACK, 0);
    output.extend_from_slice(&class);
    output.extend_from_slice(&object);
    for field in shape.fields {
        let (original, at) = (target + field.offset, copy + field.offset);
        match field.kind {
            FieldKind::OwnedArray => {
                let (count, header) = array_at(output, original)?;
                if header != 0 {
                    let header = own_array(output, stock_size, header, count, field)?;
                    point(output, at + 8, header)?;
                }
            }
            FieldKind::ObjectPointer => {
                if let Some(object) = target_of(output, original)? {
                    point(output, at, object)?;
                }
            }
            FieldKind::RowPointer => return None,
        }
    }
    Some(copy)
}

/// Where a copied object's array sits: a copy behind the entry for an array in the stock region,
/// else where it is. An array whose elements hold pointers isn't copied.
fn own_array(
    output: &mut Vec<u8>,
    stock_size: usize,
    header: usize,
    count: usize,
    field: &Field,
) -> Option<usize> {
    if header >= stock_size {
        return Some(header);
    }
    if field.element.is_some() || field.element_stride == 0 {
        return None;
    }
    let end = header + HEADER_SIZE + count.checked_mul(field.element_stride)?;
    let array = output.get(header..end)?.to_vec();
    let copy = (output.len() + MARKER_BACK).next_multiple_of(16);
    output.resize(copy - MARKER_BACK, 0);
    output.extend_from_slice(&ARRAY_MARKER.to_le_bytes());
    output.extend_from_slice(&array);
    Some(copy)
}

/// Points the self-relative slot at `slot` at `target`.
fn point(output: &mut [u8], slot: usize, target: usize) -> Option<()> {
    let relative = i64::try_from(target).ok()? - i64::try_from(slot).ok()?;
    if slot + 8 > output.len() {
        return None;
    }
    put(output, slot, &relative.to_le_bytes());
    Some(())
}
