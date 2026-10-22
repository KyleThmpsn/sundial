//! A recipe table rebuilt in the layout Sunrise's merge reads: every stock byte where the game put
//! it, and the rows a mod adds after the stock length.
//!
//! An editor that grows an array by inserting rows at its end moves every byte after it, so the
//! stock rows of later arrays, and the arrays they own, no longer sit at their stock offsets. The
//! merge compares a mod's entry with the stock entry offset by offset and can't describe that.
//! Sunrise Studio instead leaves the stock bytes alone and appends a whole copy of each array it
//! grows. This rebuilds an entry grown in place the same way: the inserted rows come out of the
//! stock region, each grown array is copied, stock rows and added rows, after everything else, and
//! every pointer the recipe lists is pointed at its target's new place.
//!
//! Bytes the editor appended after the stock entry stay at their offsets, behind a gap as long as
//! the rows that came out, so pointers among them still hold. A pointer the recipe doesn't list
//! must not cross the stock region's inserted rows, as the editor had to keep it right when it
//! inserted them.
use super::layout::{ARRAY_MARKER, HEADER_SIZE, MARKER_BACK, i64_at, put, u32_at, u64_at};
use super::recipes::{Recipe, Shape};
use super::sidecar::{FieldKind, OpKind};
use std::collections::HashSet;

/// Rows a build inserted at the end of one stock array.
struct Grown {
    /// Where the array's header sits in the stock entry and in the built entry.
    stock_header: usize,
    built_header: usize,
    stock_count: usize,
    built_count: usize,
    stride: usize,
    /// Where added rows start, and the net inserted bytes after existing padding is consumed.
    inserted: usize,
    size: usize,
    /// Where the array's copy starts in the output.
    copy_header: usize,
}

/// The header a `{count, relative}` descriptor at `at` names, or `None` for an empty array
/// without one. `Err` when the descriptor isn't a typed array.
fn header_at(bytes: &[u8], at: usize) -> Result<Option<(usize, usize)>, ()> {
    let count = usize::try_from(u64_at(bytes, at).ok_or(())?).map_err(|_| ())?;
    let relative = i64_at(bytes, at + 8).ok_or(())?;
    if count == 0 && relative == 0 {
        return Ok(None);
    }
    let header =
        usize::try_from(i64::try_from(at + 8).map_err(|_| ())? + relative).map_err(|_| ())?;
    if header < MARKER_BACK
        || header + HEADER_SIZE > bytes.len()
        || u32_at(bytes, header - MARKER_BACK) != Some(ARRAY_MARKER)
        || u64_at(bytes, header) != Some(count as u64)
    {
        return Err(());
    }
    Ok(Some((count, header)))
}

/// Where a built offset lands in the output.
struct Map<'a> {
    grown: &'a [Grown],
    /// Where the bytes appended after the stock entry start in the built entry.
    tail: usize,
}

impl Map<'_> {
    fn place(&self, built: usize) -> usize {
        if built >= self.tail {
            return built;
        }
        let mut removed = 0;
        for grown in self.grown {
            // The array's header and rows, stock and inserted, move to its copy.
            let end = grown.built_header + HEADER_SIZE + grown.built_count * grown.stride;
            if (grown.built_header..end).contains(&built) {
                return grown.copy_header + (built - grown.built_header);
            }
            if built >= grown.inserted + grown.size {
                removed += grown.size;
            }
        }
        built - removed
    }
}

/// `built` with each array the recipe lists that grew in place moved to the end, or `None` when
/// nothing grew in place or the entry isn't in the shape the recipe describes. The caller keeps
/// `built` then.
pub fn relayout(stock: &[u8], built: &[u8], recipe: &Recipe) -> Option<Vec<u8>> {
    let mut grown = inserted(stock, built, recipe)?;
    let tail = stock.len() + grown.iter().map(|array| array.size).sum::<usize>();
    if built.len() < tail {
        return None;
    }
    let mut output = compose(stock, built, &mut grown, tail)?;
    let map = Map {
        grown: &grown,
        tail,
    };
    rebase(&mut output, built, &map, recipe)?;
    // Every array the recipe lists reads back whole.
    for array in recipe.arrays {
        let Ok(Some((count, header))) = header_at(&output, array.descriptor) else {
            return None;
        };
        if header + HEADER_SIZE + count * array.row.stride > output.len() {
            return None;
        }
    }
    Some(output)
}

/// Every array the recipe lists, in stock order: its header and count in each copy, and its row
/// stride.
fn arrays(stock: &[u8], built: &[u8], recipe: &Recipe) -> Option<Vec<[usize; 5]>> {
    let mut arrays = Vec::new();
    for array in recipe.arrays {
        let (Ok(Some((stock_count, stock_header))), Ok(Some((built_count, built_header)))) = (
            header_at(stock, array.descriptor),
            header_at(built, array.descriptor),
        ) else {
            return None;
        };
        // A sorted array takes its rows in key order, so it never grows at its end alone.
        let grows_at_end = built_count == stock_count || array.kind == OpKind::Append;
        if built_count < stock_count || built_header < stock_header || !grows_at_end {
            return None;
        }
        let stock_end = stock_header
            .checked_add(HEADER_SIZE)?
            .checked_add(stock_count.checked_mul(array.row.stride)?)?;
        let built_end = built_header
            .checked_add(HEADER_SIZE)?
            .checked_add(built_count.checked_mul(array.row.stride)?)?;
        if stock_end > stock.len() || built_end > built.len() {
            return None;
        }
        arrays.push([
            stock_header,
            stock_count,
            built_header,
            built_count,
            array.row.stride,
        ]);
    }
    arrays.sort_by_key(|array| array[0]);
    Some(arrays)
}

/// The arrays that grew in place and what was inserted after each. Each array sits where the
/// bytes inserted before it moved it. What was inserted after an array's rows reaches to where
/// the next array moved, alignment included, or is its added rows alone when nothing the recipe
/// lists follows it. `None` when nothing grew or the arrays didn't move that way.
fn inserted(stock: &[u8], built: &[u8], recipe: &Recipe) -> Option<Vec<Grown>> {
    let arrays = arrays(stock, built, recipe)?;
    let mut grown = Vec::new();
    let mut total = 0;
    for (index, &[stock_header, stock_count, built_header, built_count, stride]) in
        arrays.iter().enumerate()
    {
        let rows = (built_count - stock_count) * stride;
        let size = match arrays.get(index + 1) {
            Some(&[next_stock, _, next_built, ..]) => {
                (next_built - next_stock).checked_sub(total)?
            }
            None => rows,
        };
        let unchanged = built_count == stock_count;
        if built_header - stock_header != total || (unchanged && size != 0) {
            return None;
        }
        // Added rows can occupy old alignment padding without moving the following array.
        // Their complete extent still has to end before that array's marker in both copies.
        if let Some(&[next_stock, _, next_built, ..]) = arrays.get(index + 1)
            && (stock_header + HEADER_SIZE + stock_count * stride
                > next_stock.checked_sub(MARKER_BACK)?
                || built_header + HEADER_SIZE + built_count * stride
                    > next_built.checked_sub(MARKER_BACK)?)
        {
            return None;
        }
        if unchanged {
            continue;
        }
        grown.push(Grown {
            stock_header,
            built_header,
            stock_count,
            built_count,
            stride,
            inserted: stock_header + HEADER_SIZE + stock_count * stride + total,
            size,
            copy_header: 0,
        });
        total += size;
    }
    (!grown.is_empty()).then_some(grown)
}

/// The output's bytes: the stock region without the inserted rows, a gap that keeps the bytes
/// appended after `tail` at their offsets, those bytes, then each grown array's copy, its header
/// and every row behind its marker at a 16-byte boundary. The rows a copy replaces keep their
/// stock bytes, and so does the padding after them up to the next array's marker, which a
/// rebuild that realigned the arrays may have written at another length.
fn compose(stock: &[u8], built: &[u8], grown: &mut [Grown], tail: usize) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(built.len() + grown.iter().map(|g| g.size).sum::<usize>());
    let mut at = 0;
    for array in grown.iter() {
        output.extend_from_slice(&built[at..array.inserted]);
        at = array.inserted + array.size;
    }
    output.extend_from_slice(&built[at..tail]);
    if output.len() != stock.len() {
        return None;
    }
    output.resize(tail, 0);
    output.extend_from_slice(&built[tail..]);
    for array in grown.iter_mut() {
        let marker_end = (output.len() + MARKER_BACK).next_multiple_of(16);
        output.resize(marker_end - MARKER_BACK, 0);
        output.extend_from_slice(&ARRAY_MARKER.to_le_bytes());
        array.copy_header = output.len();
        let end = array.built_header + HEADER_SIZE + array.built_count * array.stride;
        output.extend_from_slice(&built[array.built_header..end]);
    }
    for array in grown.iter() {
        let rows_end = array.stock_header + HEADER_SIZE + array.stock_count * array.stride;
        let marker = (rows_end + MARKER_BACK).next_multiple_of(16) - MARKER_BACK;
        let marked =
            marker + MARKER_BACK <= stock.len() && u32_at(stock, marker) == Some(ARRAY_MARKER);
        let end = if marked {
            marker + MARKER_BACK
        } else {
            rows_end
        };
        output[array.stock_header..end].copy_from_slice(&stock[array.stock_header..end]);
    }
    Some(output)
}

/// Points each descriptor at its array's copy, or where the array now sits, and every pointer its
/// rows hold at its target's new place.
fn rebase(output: &mut [u8], built: &[u8], map: &Map<'_>, recipe: &Recipe) -> Option<()> {
    let mut done = HashSet::new();
    for array in recipe.arrays {
        let Ok(Some((count, header))) = header_at(built, array.descriptor) else {
            return None;
        };
        rewrite(output, map, array.descriptor + 8, header, &mut done)?;
        let rows = header + HEADER_SIZE;
        for row in 0..count {
            let start = rows + row * array.row.stride;
            rebase_shape(output, built, map, start, array.row, &mut done)?;
        }
    }
    Some(())
}
/// Points the self-relative slot at built `slot` at built `target`, both in their output places.
fn rewrite(
    output: &mut [u8],
    map: &Map<'_>,
    slot: usize,
    target: usize,
    done: &mut HashSet<usize>,
) -> Option<()> {
    let place = map.place(slot);
    if !done.insert(place) {
        return Some(());
    }
    let relative = i64::try_from(map.place(target)).ok()? - i64::try_from(place).ok()?;
    put(output, place, &relative.to_le_bytes());
    Some(())
}

/// Rebases every pointer of the row or element at built `start` that `shape` lists, and those of
/// the elements and owned objects they lead to.
fn rebase_shape(
    output: &mut [u8],
    built: &[u8],
    map: &Map<'_>,
    start: usize,
    shape: &Shape,
    done: &mut HashSet<usize>,
) -> Option<()> {
    for field in shape.fields {
        let at = start + field.offset;
        match field.kind {
            FieldKind::OwnedArray => {
                let Ok(found) = header_at(built, at) else {
                    return None;
                };
                let Some((count, header)) = found else {
                    continue;
                };
                rewrite(output, map, at + 8, header, done)?;
                if let Some(element) = field.element.filter(|_| field.element_stride > 0) {
                    for index in 0..count {
                        let element_start = header + HEADER_SIZE + index * field.element_stride;
                        rebase_shape(output, built, map, element_start, element, done)?;
                    }
                }
            }
            FieldKind::RowPointer | FieldKind::ObjectPointer => {
                let relative = i64_at(built, at)?;
                if relative == 0 {
                    continue;
                }
                let target = usize::try_from(i64::try_from(at).ok()? + relative).ok()?;
                rewrite(output, map, at, target, done)?;
            }
        }
    }
    for local in shape.local {
        let at = start + local.offset;
        let relative = i64_at(built, at)?;
        if relative == 0 {
            continue;
        }
        let target = usize::try_from(i64::try_from(at).ok()? + relative).ok()?;
        rewrite(output, map, at, target, done)?;
        rebase_shape(output, built, map, target, local.target, done)?;
    }
    Some(())
}
