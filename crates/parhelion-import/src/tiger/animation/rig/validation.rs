//! Validate appended native prefixes before linking private rig owners.
use super::*;

pub fn validate_weapon_pair(equipment: &Payload, first_person: &Payload) -> Result<usize> {
    let e = equipment.pointer(24)?;
    let f = first_person.pointer(24)?;
    ensure!(
        equipment.u32(e - 4)? == 0x80808546 && first_person.u32(f - 4)? == 0x80808546,
        "Weapon palette pair requires native FK skeletons"
    );
    let equipment_rows = equipment.array(e + 0x80, 16, Some(0x80808A08))?;
    let full = first_person.array(f + 0x80, 16, Some(0x80808A08))?;
    let start = full
        .len()
        .checked_sub(equipment_rows.len())
        .context("Weapon palette prefix is outside the arm rig")?;
    ensure!(
        start > 0 && !equipment_rows.is_empty(),
        "Empty weapon palette pair"
    );
    for (i, &row) in equipment_rows.iter().enumerate() {
        ensure!(
            equipment.u32(row)? == first_person.u32(full[start + i])?,
            "Equipment and first-person weapon names differ"
        );
        if i > 0 {
            let parent = equipment.u32(row + 4)? as i32;
            ensure!(
                parent >= 0
                    && first_person.u32(full[start + i] + 4)? as usize == start + parent as usize,
                "Equipment and first-person weapon parents differ"
            );
        }
    }
    // Shadowkeep `00577040` finds the first-person range that holds the weapon root, then
    // requires each equipment range in native list order to hold as many bones as the
    // first-person range at the same distance after it.
    let equipment_ranges = ranges::Layout::read(equipment)?.native()?;
    let first_person_ranges = ranges::Layout::read(first_person)?.native()?;
    let bound = first_person_ranges
        .iter()
        .position(|&(first, _)| first == start)
        .context("Weapon root does not begin a first-person range")?;
    ensure!(
        equipment_ranges.len() <= NATIVE_RANGES
            && first_person_ranges.len() <= NATIVE_RANGES
            && first_person_ranges[bound..].len() == equipment_ranges.len()
            && equipment_ranges
                .iter()
                .zip(&first_person_ranges[bound..])
                .all(|(equipment, first_person)| equipment.1 == first_person.1),
        "Equipment ranges do not bind to the first-person ranges"
    );
    Ok(start)
}

/// Where each native bone of `original` sits in `converted`, by name, and where the appended
/// bones sit. Native names are unique, and the only layouts are appending at the end or
/// inserting one contiguous block before moved native bones.
fn renumbering(original: &Payload, converted: &Payload) -> Result<Renumbering> {
    let before = original.pointer(24)?;
    let after = converted.pointer(24)?;
    let a = original.array(before + 0x80, 16, Some(0x80808A08))?;
    let b = converted.array(after + 0x80, 16, Some(0x80808A08))?;
    ensure!(b.len() > a.len(), "Extended skeleton has no appended bones");
    let mut positions = std::collections::BTreeMap::new();
    for (index, &row) in b.iter().enumerate() {
        ensure!(
            positions.insert(converted.u32(row)?, index).is_none(),
            "Extended skeleton repeats a bone name"
        );
    }
    let mut map = Vec::with_capacity(b.len());
    for &row in &a {
        map.push(
            *positions
                .get(&original.u32(row)?)
                .context("Extended skeleton drops a native bone")?,
        );
    }
    let added = b.len() - a.len();
    let first_appended = (0..a.len()).find(|&i| map[i] != i).unwrap_or(a.len());
    let expected = Renumbering::inserted(a.len(), added, first_appended);
    ensure!(
        map == expected.map[..a.len()],
        "Extended skeleton moves native bones other than one block behind the appended bones"
    );
    Ok(expected)
}

/// Appended bones either join the range that ends at the last native bone or form one FK
/// section after it. Before a trailing procedural range they join the FK section it follows,
/// and the range's bones move after them. A new section must follow the last procedural range
/// in native list order, and the list must still fit its native capacity.
fn growth(
    old: &ranges::Layout,
    new: &ranges::Layout,
    count: usize,
    added: usize,
    first_appended: usize,
) -> Result<()> {
    if first_appended < count {
        let (first, length) = *old
            .procedural
            .last()
            .context("Inserted bones require a trailing procedural range")?;
        let section = old.sections.len() - 1;
        let (section_first, slots) = old.sections[section];
        ensure!(
            first == first_appended
                && first + length == count
                && section_first + slots == first
                && new.procedural.len() == old.procedural.len()
                && new.procedural[..section] == old.procedural[..old.procedural.len() - 1]
                && new.procedural[old.procedural.len() - 1] == (first + added, length)
                && new.sections.len() == old.sections.len()
                && new.sections[..section] == old.sections[..section]
                && new.sections[section] == (section_first, slots + added),
            "Extended skeleton inserts bones out of native list order"
        );
        return Ok(());
    }
    let retained = |old: &[(usize, usize)], new: &[(usize, usize)]| -> Result<usize> {
        ensure!(
            new.len() >= old.len(),
            "Extended skeleton removes a native range"
        );
        let mut grown = 0;
        for (&(first, length), &(new_first, new_length)) in old.iter().zip(new) {
            ensure!(
                new_first == first
                    && (new_length == length
                        || (first + length == count && new_length == length + added)),
                "Extended skeleton changes a native range"
            );
            grown += usize::from(new_length != length);
        }
        Ok(grown)
    };
    let grown =
        retained(&old.sections, &new.sections)? + retained(&old.procedural, &new.procedural)?;
    ensure!(
        new.procedural.len() == old.procedural.len(),
        "Extended skeleton adds a procedural range"
    );
    match (grown, &new.sections[old.sections.len()..]) {
        (1, []) => Ok(()),
        (0, [(first, length)]) => {
            ensure!(
                *first == count
                    && *length == added
                    && old.sections.len() == old.procedural.len()
                    && old
                        .procedural
                        .last()
                        .is_some_and(|&(first, length)| first + length == count),
                "Extended skeleton appends a range out of native list order"
            );
            ensure!(
                new.sections.len() + new.procedural.len() <= NATIVE_RANGES,
                "Skeleton has more than four bone ranges, which crashes Destiny when the weapon \
                 is equipped. Import the weapon again"
            );
            Ok(())
        }
        _ => {
            anyhow::bail!("Extended skeleton must grow its trailing range or append one FK section")
        }
    }
}

pub fn validate_extension(
    original: &Payload,
    converted: &Payload,
    relocation: Option<(usize, usize)>,
) -> Result<()> {
    let before = original.pointer(24)?;
    let after = converted.pointer(24)?;
    let class = original.u32(before - 4)?;
    ensure!(
        converted.u32(after - 4)? == class,
        "Extended rig changes its native resource class"
    );
    let prefix = |field, stride, row_class| -> Result<(Vec<usize>, Vec<usize>)> {
        let a = original.array(before + field, stride, Some(row_class))?;
        let b = converted.array(after + field, stride, Some(row_class))?;
        ensure!(b.len() >= a.len(), "Extended rig removes native records");
        Ok((a, b))
    };
    let unchanged = |field, stride, row_class| -> Result<(usize, usize)> {
        let (a, b) = prefix(field, stride, row_class)?;
        for (&a, &b) in a.iter().zip(&b) {
            ensure!(
                original.0[a..a + stride] == converted.0[b..b + stride],
                "Extended rig changes a retained native record"
            );
        }
        Ok((a.len(), b.len()))
    };
    match class {
        0x80808546 => {
            // A grown procedural range leaves the instance image in place.
            let (cut, delta) = relocation.context("Extended skeleton has no relocation")?;
            ensure!(
                cut.checked_add(8) == Some(before) && before.checked_add(delta) == Some(after),
                "Extended skeleton relocation differs"
            );
            let (a, b) = prefix(0x80, 16, 0x80808A08)?;
            let moved = renumbering(original, converted)?;
            // Every native bone keeps its name, parent and next sibling, read through the
            // renumbering. A parent of appended bones gains a first child, so that word is
            // free to change.
            for (i, &row) in a.iter().enumerate() {
                let at = b[moved.map[i]];
                ensure!(
                    original.u32(row)? == converted.u32(at)?
                        && moved.index(original.u32(row + 4)? as i32)?
                            == converted.u32(at + 4)? as i32
                        && moved.index(original.u32(row + 12)? as i32)?
                            == converted.u32(at + 12)? as i32,
                    "Extended skeleton changes native names, parents or siblings"
                );
            }
            // Bind poses and range maps of the native bones follow them unchanged.
            for (field, stride, kind) in [
                (0x90, 32, 0x80809F75),
                (0xA0, 32, 0x80809F75),
                (0xB0, 2, 0x80800006),
                (0xC0, 2, 0x80800006),
            ] {
                let (x, y) = prefix(field, stride, kind)?;
                ensure!(
                    x.len() == a.len() && y.len() == b.len(),
                    "Extended skeleton maps have inconsistent counts"
                );
                for (i, &x) in x.iter().enumerate() {
                    let y = y[moved.map[i]];
                    ensure!(
                        original.0[x..x + stride] == converted.0[y..y + stride],
                        "Extended rig changes a retained native record"
                    );
                }
            }
            ensure!(
                original.pointer(16)? == converted.pointer(16)?,
                "Extended skeleton moves its instance root"
            );
            // Shadowkeep reads the words after the last procedural row as tags up to a zero
            // word, so only padding may lie between the procedural ranges and the definition.
            let instance = converted.pointer(16)?;
            if let Some(&last) = converted
                .array(instance + 0x40, 8, Some(0x80808A06))?
                .last()
            {
                ensure!(
                    converted
                        .0
                        .get(last + 8..after - 4)
                        .is_some_and(|tail| tail.iter().all(|&byte| byte == 0)),
                    "Extended skeleton keeps data after its procedural ranges"
                );
            }
            let old = ranges::Layout::read(original)?;
            let new = ranges::Layout::read(converted)?;
            growth(&old, &new, a.len(), b.len() - a.len(), moved.first_appended)?;
        }
        0x80808F8F => {
            ensure!(
                relocation.is_none() && before == after,
                "Extended controls relocate native definitions"
            );
            let (controls_before, controls_after) = prefix(0xA0, 52, 0x80809076)?;
            let (forward_before, forward_after) = prefix(0xD8, 2, 0x80800006)?;
            let (reverse_before, reverse_after) = prefix(0xE8, 2, 0x80800006)?;
            let slots = controls_before.len();
            let count = forward_before.len();
            let added = controls_after.len() - slots;
            ensure!(
                added > 0
                    && forward_after.len() == count + added
                    && reverse_before.len() == slots
                    && reverse_after.len() == slots + added
                    && unchanged(0xB0, 32, 0x80809F75)? == (slots, slots + added),
                "Extended control counts disagree"
            );
            // The appended bones' slots are new, so the bone to slot map names them where the
            // appended bones were put, and the native bones from there moved after them.
            let slot = |payload: &Payload, at: usize| -> Result<i32> {
                Ok(i32::from(payload.u16(at)? as i16))
            };
            let first_appended = (0..count)
                .find(|&i| slot(converted, forward_after[i]).is_ok_and(|s| s >= slots as i32))
                .unwrap_or(count);
            let moved = Renumbering::inserted(count, added, first_appended);
            for (i, &at) in forward_before.iter().enumerate() {
                ensure!(
                    slot(original, at)? == slot(converted, forward_after[moved.map[i]])?,
                    "Extended controls change a native bone's slot"
                );
            }
            for k in 0..added {
                ensure!(
                    slot(converted, forward_after[first_appended + k])? == (slots + k) as i32
                        && slot(converted, reverse_after[slots + k])?
                            == (first_appended + k) as i32,
                    "Extended controls map an appended bone to another slot"
                );
            }
            for (s, &at) in reverse_before.iter().enumerate() {
                ensure!(
                    moved.index(slot(original, at)?)? == slot(converted, reverse_after[s])?,
                    "Extended controls change a native slot's bone"
                );
            }
            for (&a, &b) in controls_before.iter().zip(&controls_after) {
                let mut expected = original.0[a..a + 52].to_vec();
                super::renumber_control(&mut expected, &moved)?;
                ensure!(
                    expected == converted.0[b..b + 52],
                    "Extended rig changes a retained native record"
                );
            }
            for (field, stride, kind) in [(0xC0, 2, 0x80800006), (0x110, 8, 0x80808FFE)] {
                let (a, b) = unchanged(field, stride, kind)?;
                ensure!(
                    a == b,
                    "Extended controls change native constraint bindings"
                );
            }
            let (a, b) = prefix(0x90, 8, 0x8080907F)?;
            ensure!(
                !a.is_empty() && a.len() == b.len(),
                "Extended control groups differ"
            );
            let groups = a.len();
            for (index, (&a, &b)) in a.iter().zip(&b).enumerate() {
                let extra = if index + 1 == groups { added } else { 0 };
                ensure!(
                    original.u32(a)? == converted.u32(b)?
                        && original.u32(a + 4)? as usize + extra == converted.u32(b + 4)? as usize,
                    "Extended control group changes its native prefix"
                );
            }
        }
        _ => anyhow::bail!("Unsupported native rig extension class {class:08X}"),
    }
    Ok(())
}
