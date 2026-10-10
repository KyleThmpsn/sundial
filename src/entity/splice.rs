//! One gameplay component copied from another weapon onto this weapon's own objects.
//!
//! A weapon's trigger, barrel and magazine are resources of one gameplay owner, so replacing the
//! owner moves all of them together. Across every stock gameplay owner each of these components is
//! the same graph of objects: the same classes, spans and reference positions, differing only in
//! the values they hold, optional spread patterns and the lengths of some arrays. A splice keeps the target's
//! objects and every reference between them, which keeps the event wiring that addresses them,
//! and copies the donor's values over them. An array of equal length is copied in place. A plain
//! array of another length gets the donor's elements appended to the owner and its descriptor
//! pointed there. Anything that does not match, or an array whose elements could hold pointers,
//! refuses the splice rather than guessing.
use super::*;
use crate::package_runtime::reader::PackageManager;

/// Element sizes of the array classes a component graph carries, measured from the stock owners:
/// one byte for `80800009`, sixteen for `80800090` and `808045C7`, and twenty for spread rings.
const ARRAY_STRIDES: [(u32, usize); 4] = [
    (0x8080_0009, 1),
    (0x8080_0090, 16),
    (0x8080_888F, 20),
    (0x8080_45C7, 16),
];

/// Classes whose elements carry only values. `808045C7` rows can hold a relative pointer, so they
/// are copied only where neither side's rows look like one.
const PLAIN_ARRAYS: [u32; 3] = [0x8080_0009, 0x8080_0090, 0x8080_888F];

/// A planned splice of one component into the target's gameplay owner.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComponentSplice {
    /// The target's owner the edits apply to.
    pub owner_tag: u32,
    /// The component's resource offset in that owner, which every edit is measured from.
    pub resource_offset: usize,
    /// Bytes to write over the owner, as absolute offset and the donor's values.
    pub writes: Vec<(usize, Vec<u8>)>,
    /// Arrays given another length: the descriptor's absolute offset, the new header and
    /// elements to append, and the element count.
    pub arrays: Vec<(usize, Vec<u8>, u64)>,
    /// Optional native records with explicit pointer and self-reference relocation.
    pub records: Vec<ComponentSpliceRecord>,
}

/// A self-contained native record to append on a 16-byte boundary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComponentSpliceRecord {
    pub bytes: Vec<u8>,
    /// Absolute owner slots and their target offsets within `bytes`.
    pub pointers: Vec<(usize, usize)>,
    /// Absolute-offset words inside `bytes` and the targets they name within `bytes`.
    pub references: Vec<(usize, usize)>,
}

fn is_class(value: u32) -> bool {
    value & 0xFFFF_0000 == 0x8080_0000
}

/// One owner's payload with where its objects start, to bound each object's span.
struct Owner {
    tag: u32,
    bytes: Vec<u8>,
    /// Every resource the entity binds in this owner.
    resources: BTreeSet<usize>,
    /// Every object start: resources, reference targets and array headers.
    starts: BTreeSet<usize>,
}

impl Owner {
    fn read(manager: &PackageManager, entity: &[u8], tag: u32) -> Result<Self, String> {
        let bytes = manager.read_tag(tag)?;
        if usize::try_from(read_u64(&bytes, 0)?).ok() != Some(bytes.len()) {
            return Err(format!(
                "Component owner 0x{tag:08X} has an inconsistent file-size field"
            ));
        }
        let mut resources = BTreeSet::new();
        for hash in weapon_component_binding_hashes(entity)? {
            for binding in weapon_component_bindings(entity, hash)? {
                if binding.owner_tag == tag {
                    resources.insert(
                        usize::try_from(binding.resource_offset)
                            .map_err(|_| "Component resource offset does not fit")?,
                    );
                }
            }
        }
        let mut owner = Self {
            tag,
            bytes,
            starts: resources.clone(),
            resources,
        };
        let mut at = 0;
        while at + 16 <= owner.bytes.len() {
            if let Some(target) = owner.reference(at) {
                owner.starts.insert(target);
            } else if let Some((header, _, _)) = owner.array(at) {
                owner.starts.insert(header);
            }
            at += 8;
        }
        Ok(owner)
    }

    /// The target of an `(owner, class, offset)` reference at `at`.
    fn reference(&self, at: usize) -> Option<usize> {
        if read_u32(&self.bytes, at).ok()? != self.tag
            || !is_class(read_u32(&self.bytes, at + 4).ok()?)
        {
            return None;
        }
        let target = usize::try_from(read_u64(&self.bytes, at + 8).ok()?).ok()?;
        (target < self.bytes.len()).then_some(target)
    }

    /// The array descriptor at `at`: its header, element count and element class. A descriptor's
    /// relative pointer reaches a header that repeats the count, and an empty array keeps one too.
    fn array(&self, at: usize) -> Option<(usize, u64, u32)> {
        let count = read_u64(&self.bytes, at).ok()?;
        let relative = i64::from_le_bytes(self.bytes.get(at + 8..at + 16)?.try_into().ok()?);
        if relative <= 0 || count > 100_000 {
            return None;
        }
        let header = (at + 8).checked_add(usize::try_from(relative).ok()?)?;
        if header + 16 > self.bytes.len() || read_u64(&self.bytes, header).ok()? != count {
            return None;
        }
        let class = read_u32(&self.bytes, header + 8).ok()?;
        is_class(class).then_some((header, count, class))
    }

    /// Where the object starting at `start` ends: the next object start.
    fn end(&self, start: usize) -> usize {
        self.starts
            .range(start + 1..)
            .next()
            .copied()
            .unwrap_or(self.bytes.len())
    }

    /// Whether a 16-byte array row could hold a relative pointer or a reference.
    fn pointer_like(&self, at: usize) -> bool {
        let words = (0..4)
            .map(|word| read_u32(&self.bytes, at + word * 4).unwrap_or(0))
            .collect::<Vec<_>>();
        let relative = i64::from_le_bytes(self.bytes[at + 8..at + 16].try_into().unwrap_or([0; 8]));
        words.contains(&self.tag)
            || (read_u64(&self.bytes, at).unwrap_or(0) > 0
                && relative > 0
                && relative < self.bytes.len() as i64)
    }
}

/// Plans a splice of the component `binding` from `donor` into `target`, both complete weapon
/// runtime entities, reading their gameplay owners through `manager`.
pub fn plan_component_splice(
    manager: &PackageManager,
    target: &[u8],
    donor: &[u8],
    binding: u32,
) -> Result<ComponentSplice, String> {
    let single = |entity: &[u8], which: &str| {
        let bindings = weapon_component_bindings(entity, binding)?;
        match bindings.as_slice() {
            [one] => Ok(*one),
            _ => Err(format!(
                "The {which} weapon does not have one component 0x{binding:08X}"
            )),
        }
    };
    let host_binding = single(target, "base")?;
    let donor_binding = single(donor, "donor")?;
    if host_binding.concrete_class != donor_binding.concrete_class
        || host_binding.resource_offset != donor_binding.resource_offset
    {
        return Err(format!(
            "Component 0x{binding:08X} has another layout on the donor weapon"
        ));
    }
    let host = Owner::read(manager, target, host_binding.owner_tag)?;
    let other = Owner::read(manager, donor, donor_binding.owner_tag)?;
    let start = usize::try_from(host_binding.resource_offset)
        .map_err(|_| "Component resource offset does not fit")?;
    let record_end = |owner: &Owner| {
        owner
            .resources
            .range(start + 1..)
            .next()
            .copied()
            .unwrap_or(owner.bytes.len())
    };
    if record_end(&host) != record_end(&other) {
        return Err(format!(
            "Component 0x{binding:08X} has another size on the donor weapon"
        ));
    }
    let mut splice = ComponentSplice {
        owner_tag: host.tag,
        resource_offset: start,
        ..ComponentSplice::default()
    };
    let mut copied = Vec::<(usize, u8)>::new();
    let spread_pointers = if binding == WEAPON_BARREL_COMPONENT_KEY {
        spread_records(&host, host_binding, &other, donor_binding, &mut splice)?
    } else {
        BTreeMap::new()
    };
    let mut queue = vec![(start, start, record_end(&host))];
    let mut visited = BTreeSet::new();
    while let Some((at_host, at_donor, end_host)) = queue.pop() {
        if !visited.insert(at_host) {
            continue;
        }
        let span = end_host - at_host;
        let end_donor = if at_host == start {
            at_donor + span
        } else {
            other.end(at_donor)
        };
        if end_donor - at_donor != span {
            return Err(format!(
                "Component 0x{binding:08X} has an object of another size on the donor weapon"
            ));
        }
        let mut offset = 0;
        while offset < span {
            let (h, d) = (at_host + offset, at_donor + offset);
            if let Some(&donor_slot) = spread_pointers.get(&h) {
                if donor_slot != d {
                    return Err("Barrel spread pointer moved outside its paired object".into());
                }
                offset += 8;
                continue;
            }
            if offset + 16 <= span {
                match (host.reference(h), other.reference(d)) {
                    (Some(host_target), Some(donor_target)) => {
                        if read_u32(&host.bytes, h + 4)? != read_u32(&other.bytes, d + 4)? {
                            return Err(format!(
                                "Component 0x{binding:08X} references another class on the donor weapon"
                            ));
                        }
                        // Into a resource or within this object, the target's own reference
                        // stays. A reference into trailing data is followed in both.
                        let inside = host_target >= at_host && host_target < end_host;
                        if !host.resources.contains(&host_target) && !inside {
                            if other.resources.contains(&donor_target) {
                                return Err(format!(
                                    "Component 0x{binding:08X} references another object on the donor weapon"
                                ));
                            }
                            queue.push((host_target, donor_target, host.end(host_target)));
                        }
                        offset += 16;
                        continue;
                    }
                    (None, None) => {}
                    _ => {
                        return Err(format!(
                            "Component 0x{binding:08X} has a reference where the donor weapon has none"
                        ));
                    }
                }
                // An empty optional array can be sixteen zero bytes with no header at all, so
                // zeros stand in for an empty array where the other side has a real one.
                let empty = |owner: &Owner, at: usize| {
                    owner.bytes[at..at + 16]
                        .iter()
                        .all(|byte| *byte == 0)
                        .then_some((None, 0, 0))
                };
                let host_array = host
                    .array(h)
                    .map(|(header, count, class)| (Some(header), count, class));
                let donor_array = other
                    .array(d)
                    .map(|(header, count, class)| (Some(header), count, class));
                match (host_array, donor_array) {
                    (None, None) => {}
                    (host_array, donor_array) => {
                        let (Some(host_array), Some(donor_array)) = (
                            host_array.or_else(|| empty(&host, h)),
                            donor_array.or_else(|| empty(&other, d)),
                        ) else {
                            return Err(format!(
                                "Component 0x{binding:08X} has an array where the donor weapon has none"
                            ));
                        };
                        array(
                            &host,
                            &other,
                            h,
                            host_array,
                            donor_array,
                            &mut copied,
                            &mut splice,
                        )
                        .map_err(|error| format!("Component 0x{binding:08X}: {error}"))?;
                        offset += 16;
                        continue;
                    }
                }
            }
            let width = (span - offset).min(8);
            for byte in 0..width {
                copied.push((h + byte, other.bytes[d + byte]));
            }
            offset += width;
        }
    }
    // Contiguous changed bytes become one write each.
    copied.sort_unstable_by_key(|(at, _)| *at);
    copied.dedup_by_key(|(at, _)| *at);
    for (at, value) in copied {
        if host.bytes[at] == value {
            continue;
        }
        match splice.writes.last_mut() {
            Some((start, bytes)) if *start + bytes.len() == at => bytes.push(value),
            _ => splice.writes.push((at, vec![value])),
        }
    }
    Ok(splice)
}

/// Copy the optional spread separately. Only declared typed pointers can alias its interfaces.
/// Integer values and the absolute-offset halves of typed references are never treated as pointers.
fn spread_records(
    host: &Owner,
    host_binding: WeaponComponentBinding,
    donor: &Owner,
    donor_binding: WeaponComponentBinding,
    splice: &mut ComponentSplice,
) -> Result<BTreeMap<usize, usize>, String> {
    use super::spread::{DEFINITION, INSTANCE, Spread};
    use crate::package_payload::{bytes_at, write_bytes};
    use crate::package_runtime::references::schema::Registry;
    let target = Spread::read(&host.bytes, host_binding)?;
    let source = Spread::read(&donor.bytes, donor_binding)?;
    let mut record = ComponentSpliceRecord::default();
    // Pattern objects begin eight bytes after an aligned boundary. Both the object and its
    // ring header carry a class marker immediately before their address.
    const PATTERN: usize = 8;
    const HEADER: usize = 0x80;
    if let Some(pattern) = &source.pattern {
        record.bytes.resize(HEADER + 16, 0);
        record.bytes[4..PATTERN + 0x68]
            .copy_from_slice(&donor.bytes[pattern.offset - 4..pattern.offset + 0x68]);
        write_bytes(
            &mut record.bytes,
            HEADER - 4,
            &0x8080_9FBD_u32.to_le_bytes(),
        )?;
        write_bytes(&mut record.bytes, HEADER, &pattern.count.to_le_bytes())?;
        write_bytes(
            &mut record.bytes,
            HEADER + 8,
            &0x8080_888F_u32.to_le_bytes(),
        )?;
        write_bytes(
            &mut record.bytes,
            PATTERN + 0x50,
            &((HEADER - PATTERN - 0x50) as i64).to_le_bytes(),
        )?;
        record
            .bytes
            .extend_from_slice(&donor.bytes[pattern.rings.clone()]);
        record
            .bytes
            .resize(record.bytes.len().next_multiple_of(16), 0);
        for ordinal in 0..3 {
            let reference = PATTERN + ordinal * 0x18 + 8;
            write_bytes(&mut record.bytes, reference, &host.tag.to_le_bytes())?;
            record.references.push((reference + 8, PATTERN));
        }
    }
    let mut pointers = BTreeMap::new();
    let mut registry = Registry::new()?;
    let host_instance = host_binding.resource_offset as usize;
    let donor_instance = donor_binding.resource_offset as usize;
    let host_definition = target.slot - 0xE60;
    let donor_definition = source.slot - 0xE60;
    for (class, host_start, donor_start) in [
        (INSTANCE, host_instance, donor_instance),
        (DEFINITION, host_definition, donor_definition),
    ] {
        let schema = registry.record(class, |_| Err("Barrel must use a native schema".into()))?;
        for &(offset, _) in schema.fields.iter().filter(|(_, kind)| *kind == 3) {
            let (h, d) = (host_start + offset, donor_start + offset);
            let interface =
                |owner: &Owner, slot: usize, spread: &Spread| -> Result<Option<usize>, String> {
                    let delta = i64::from_le_bytes(bytes_at(&owner.bytes, slot)?);
                    let Some(pattern) = &spread.pattern else {
                        return Ok(None);
                    };
                    if delta == 0 {
                        return Ok(None);
                    }
                    let named = slot.checked_add_signed(
                        isize::try_from(delta).map_err(|_| "Spread pointer overflow")?,
                    );
                    Ok([0, 0x18, 0x30]
                        .into_iter()
                        .find(|offset| named == Some(pattern.offset + offset)))
                };
            let from = interface(donor, d, &source)?;
            let old = interface(host, h, &target)?;
            if h != target.slot && from.is_none() && old.is_none() {
                continue;
            }
            pointers.insert(h, d);
            if let Some(offset) = from {
                record.pointers.push((h, PATTERN + offset));
            } else if read_u64(&donor.bytes, d)? == 0 {
                if read_u64(&host.bytes, h)? != 0 {
                    splice.writes.push((h, vec![0; 8]));
                }
            } else {
                return Err("Barrel spread interface has an unsupported donor target".into());
            }
        }
    }
    if !record.pointers.is_empty() {
        splice.records.push(record);
    }
    Ok(pointers)
}

/// One array pair: equal lengths copy in place, plain arrays of another length are appended.
fn array(
    host: &Owner,
    donor: &Owner,
    descriptor: usize,
    (host_header, host_count, host_class): (Option<usize>, u64, u32),
    (donor_header, donor_count, donor_class): (Option<usize>, u64, u32),
    copied: &mut Vec<(usize, u8)>,
    splice: &mut ComponentSplice,
) -> Result<(), String> {
    if host_count == 0 && donor_count == 0 {
        return Ok(());
    }
    if host_count > 0 && donor_count > 0 && host_class != donor_class {
        return Err(format!(
            "array of 0x{host_class:08X} holds 0x{donor_class:08X} on the donor weapon"
        ));
    }
    let class = if donor_count > 0 {
        donor_class
    } else {
        host_class
    };
    let stride = ARRAY_STRIDES
        .iter()
        .find(|(known, _)| *known == class)
        .map(|(_, stride)| *stride)
        .ok_or_else(|| format!("array of 0x{class:08X} has an unknown element size"))?;
    let rows = |owner: &Owner, header: Option<usize>, count: u64| match header {
        None => Some(0..0),
        Some(header) => {
            let start = header + 16;
            let end = start + usize::try_from(count).ok()?.checked_mul(stride)?;
            (end <= owner.bytes.len()).then_some(start..end)
        }
    };
    let host_rows = rows(host, host_header, host_count).ok_or("array rows are out of bounds")?;
    let donor_rows =
        rows(donor, donor_header, donor_count).ok_or("array rows are out of bounds")?;
    // A row that could be a pointer is safe to copy only over the same row, word for word: its
    // offsets are relative to itself, so they stay valid, and only the values beside it change.
    // Austringer's and Sweet Business's magazines hold such rows with different values beside them.
    if !PLAIN_ARRAYS.contains(&class)
        && (host_count != donor_count
            || host_rows
                .clone()
                .step_by(stride)
                .zip(donor_rows.clone().step_by(stride))
                .any(|(at, from)| {
                    (host.pointer_like(at) || donor.pointer_like(from))
                        && host.bytes[at..at + stride] != donor.bytes[from..from + stride]
                }))
        && (host_rows
            .clone()
            .step_by(stride)
            .any(|at| host.pointer_like(at))
            || donor_rows
                .clone()
                .step_by(stride)
                .any(|at| donor.pointer_like(at)))
    {
        return Err(format!(
            "array of 0x{class:08X} holds rows that could be pointers"
        ));
    }
    if host_count == donor_count {
        for (at, from) in host_rows.zip(donor_rows) {
            copied.push((at, donor.bytes[from]));
        }
        return Ok(());
    }
    // Another length: the donor's header and rows go to the end of the owner, padded to whole
    // 16-byte rows, and the descriptor names them with the donor's count.
    let mut appended = match donor_header {
        Some(header) => donor.bytes[header..header + 16].to_vec(),
        None => vec![0; 16],
    };
    appended[..8].copy_from_slice(&donor_count.to_le_bytes());
    appended[8..12].copy_from_slice(&class.to_le_bytes());
    appended.extend_from_slice(&donor.bytes[donor_rows]);
    appended.resize(appended.len().next_multiple_of(16), 0);
    splice.arrays.push((descriptor, appended, donor_count));
    Ok(())
}
