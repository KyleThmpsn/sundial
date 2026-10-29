//! Private, keyed copies of native sword attack profiles.
use super::*;

const BINDING: u32 = 0xCD2B_CEAC;
const SWORD_CLASS: u32 = 0x8080_43D2;
const PROFILE_CLASS: u32 = 0x8080_2D7B;
const PROFILE_SIZE: usize = 0x220;
const ARRAY_MARKER: u32 = 0x8080_9FBD;
const EMPTY_KEY: u32 = 0x811C_9DC5;
const KEY_CLASS: u32 = 0x8080_43D5;
const KEY_DESCRIPTOR: usize = 0x1F0;
const KEY_ACTIVE_COUNT: usize = 0x228;

#[cfg(feature = "d2-model-importer")]
fn powered_row(owner: &[u8], row: usize) -> AuthoringResult<bool> {
    if read_u64(owner, row + 0x18)? == 0 {
        return Ok(false);
    }
    let (count, gates) = checked_array(owner, row + 0x18, 0x8080_2D6E, 8)?;
    let mut ammo = false;
    for index in 0..count {
        let gate = relative_target(owner, gates + index * 8)?;
        match read_u32(owner, gate.saturating_sub(4))? {
            0x8080_2D70 => {
                let minimum = f32::from_bits(read_u32(owner, gate)?);
                let spend = f32::from_bits(read_u32(owner, gate + 4)?);
                if minimum <= 0. || !minimum.is_finite() || spend >= 0. || !spend.is_finite() {
                    return Err(invalid("Sword ammo gate has unsupported limits"));
                }
                ammo = true;
            }
            0x8080_2D6F => (),
            _ => {
                return Err(invalid(
                    "Sword attack has an unsupported eligibility condition",
                ));
            }
        }
    }
    if !ammo {
        return Err(invalid("Sword attack has no qualified ammo condition"));
    }
    Ok(true)
}

#[cfg(feature = "d2-model-importer")]
pub(super) struct Imported {
    owner: u32,
    digest: String,
    powered: String,
    depleted: String,
    effect: String,
}

#[cfg(feature = "d2-model-importer")]
impl Imported {
    pub(super) fn load(graph: &serde_json::Value) -> AuthoringResult<Option<Self>> {
        let Some(value) = graph.get("sword") else {
            return Ok(None);
        };
        if value["schema"] != 1 || value["installable"] != true {
            return Err(invalid("Reimport this sword's mechanics"));
        }
        let text = |key: &str| {
            value[key]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("Imported sword lacks {key}")))
        };
        Ok(Some(Self {
            owner: value["source_owner"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| invalid("Imported sword lacks its source owner"))?,
            digest: text("source_sha256")?,
            powered: text("powered_contact")?,
            depleted: text("depleted_contact")?,
            effect: text("root")?,
        }))
    }

    pub(super) fn root(&self) -> &str {
        &self.effect
    }

    pub(super) fn validate<M>(&self, nodes: &[super::assets::Node<M>]) -> AuthoringResult<()> {
        let node = |symbol: &str| {
            nodes
                .iter()
                .find(|node| node.symbol == symbol)
                .filter(|node| {
                    node.reference.is_none()
                        && node.storage == crate::NewTagStorageMode::InheritTemplate
                })
                .ok_or_else(|| {
                    invalid(format!(
                        "Imported sword asset {symbol} is absent or changes its class"
                    ))
                })
        };
        if self.powered == self.depleted
            || self.digest.len() != 64
            || !self.digest.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid("Imported sword identity is invalid"));
        }
        for symbol in [&self.powered, &self.depleted] {
            let payload = &node(symbol)?.payload;
            let definition = relative_target(payload, 24)?;
            if read_u32(payload, definition.saturating_sub(4))? != 0x8080_4B02
                || payload.get(definition + 0x10) != Some(&0)
            {
                return Err(invalid(
                    "Imported sword contact is not an absolute damage profile",
                ));
            }
        }
        let root = node(&self.effect)?;
        if crate::tag_payload::read_u16(&root.payload, 0x96)? != 17 {
            return Err(invalid(
                "Imported sword blast is not a native effect entity",
            ));
        }
        super::assets::replication::validate(nodes, root)
    }

    /// Patch only the cloned weapon owner. Eligibility, timing, cancellation and
    /// ammo consumption remain on the native controller's selected attack row.
    pub(super) fn patches(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        symbols: &BTreeMap<String, TagHash>,
    ) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
        use sha2::{Digest, Sha256};
        let bindings = weapon_component_bindings(entity, BINDING).map_err(invalid)?;
        let [binding] = bindings.as_slice() else {
            return Err(invalid("Imported sword requires one sword controller"));
        };
        if binding.owner_tag != self.owner || binding.concrete_class != SWORD_CLASS {
            return Err(invalid(
                "Imported sword mechanics do not match the selected gameplay donor",
            ));
        }
        let owner = read_tag(manager, TagHash(self.owner), "imported sword donor")?;
        if hex::encode(Sha256::digest(&owner)) != self.digest {
            return Err(invalid(
                "Sword donor changed since import. Reimport this weapon",
            ));
        }
        let resolve = |name: &str| {
            symbols
                .get(name)
                .copied()
                .ok_or_else(|| invalid(format!("Imported sword asset {name} was not allocated")))
        };
        let powered = resolve(&self.powered)?;
        let depleted = resolve(&self.depleted)?;
        let effect = resolve(&self.effect)?;
        let resource = usize::try_from(binding.resource_offset)
            .map_err(|_| invalid("Sword resource offset overflows"))?;
        if resource < 4
            || read_u32(&owner, resource - 4)? != SWORD_CLASS
            || read_u32(&owner, resource)? != self.owner
        {
            return Err(invalid(
                "Imported sword resource does not match its binding",
            ));
        }
        let mut values = BTreeMap::new();
        let mut counts = [0usize; 2];
        for bank in profile_banks(&owner, resource)? {
            for index in 0..bank.count {
                let row = bank.rows + index * PROFILE_SIZE;
                let powered_row = powered_row(&owner, row)?;
                let strike = f32::from_bits(read_u32(&owner, row + 0x1A0)?);
                if powered_row && (!strike.is_finite() || strike < 0.) {
                    return Err(invalid("Powered sword attack has no timed strike"));
                }
                counts[usize::from(powered_row)] += 1;
                let contact = relative_target(&owner, row + 0x1A8)?;
                if read_u32(&owner, contact.saturating_sub(4))? != 0x8080_4455 {
                    return Err(invalid("Imported sword requires a native contact attack"));
                }
                for (at, value) in [
                    (row + 0x198, if powered_row { effect.0 } else { u32::MAX }),
                    (
                        contact + 0x28,
                        if powered_row { powered.0 } else { depleted.0 },
                    ),
                ] {
                    if values.insert(at, value).is_some_and(|old| old != value) {
                        return Err(invalid(
                            "Powered and depleted sword attacks share mutable contact data",
                        ));
                    }
                }
            }
        }
        if counts.contains(&0) {
            return Err(invalid(
                "Imported sword requires powered and depleted attacks",
            ));
        }
        values
            .into_iter()
            .map(|(at, value)| {
                let offset = at
                    .checked_sub(resource)
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| invalid("Sword mechanics patch lies outside its resource"))?;
                Ok(WeaponRuntimeResourcePatch {
                    binding_hash: BINDING,
                    resource_index: u16::try_from(binding.resource_index)
                        .map_err(|_| invalid("Sword resource index overflows"))?,
                    offset,
                    bytes: value.to_le_bytes().to_vec(),
                    graph_values: Vec::new(),
                    graph_removals: Vec::new(),
                    graph_trajectories: None,
                })
            })
            .collect()
    }
}

// These are the checked relative fields of a native 80802D7B profile. Each
// array field has its count in the preceding eight bytes. The typed object at
// 1A8 is a direct relative reference. 110 is an optional typed reference.
const ARRAY_FIELDS: &[(usize, u32, usize)] = &[
    (0x20, 0x8080_2D6E, 8),
    (0x78, 0x8080_0009, 1),
    (0x88, 0x8080_0090, 0x10),
    (0xB0, 0x8080_0009, 1),
    (0xC0, 0x8080_0090, 0x10),
    (0xE8, 0x8080_0009, 1),
    (0xF8, 0x8080_0090, 0x10),
    (0x120, 0x8080_4446, 0x10),
    (0x1C8, 0x8080_0009, 1),
    (0x1D8, 0x8080_0090, 0x10),
    // The four stock 2D6D arrays end at header + 16 + count * 0xE8, before
    // the next typed marker. Their rows remain preserved by reference.
    (0x1F8, 0x8080_2D6D, 0xE8),
];

struct Bank {
    descriptor: usize,
    rows: usize,
    count: usize,
}

pub(super) struct Edits {
    pub appends: Vec<WeaponRuntimeResourceAppend>,
    pub patches: Vec<WeaponRuntimeResourcePatch>,
}

fn descriptor_patch(
    resource: usize,
    descriptor: usize,
    target: usize,
    count: usize,
) -> AuthoringResult<WeaponRuntimeResourcePatch> {
    let offset = descriptor
        .checked_sub(resource)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| invalid("Sword array descriptor is outside its resource"))?;
    let pointer = descriptor
        .checked_add(8)
        .ok_or_else(|| invalid("Sword array descriptor overflows"))?;
    let mut bytes = u64::try_from(count)
        .map_err(|_| invalid("Sword array count overflows"))?
        .to_le_bytes()
        .to_vec();
    bytes.extend_from_slice(&relative(pointer, target)?.to_le_bytes());
    Ok(WeaponRuntimeResourcePatch {
        binding_hash: BINDING,
        resource_index: 0,
        offset,
        bytes,
        graph_values: Vec::new(),
        graph_removals: Vec::new(),
        graph_trajectories: None,
    })
}

fn checked_array(
    owner: &[u8],
    descriptor: usize,
    expected_class: u32,
    stride: usize,
) -> AuthoringResult<(usize, usize)> {
    let (count, _, rows, class) = array_at(owner, descriptor)?;
    if class != expected_class || count == 0 || count > 128 {
        return Err(invalid(format!(
            "Sword array at 0x{descriptor:X} has class 0x{class:08X} or count {count}, expected nonempty class 0x{expected_class:08X}"
        )));
    }
    if rows
        .checked_add(
            count
                .checked_mul(stride)
                .ok_or_else(|| invalid("Sword array size overflows"))?,
        )
        .is_none_or(|end| end > owner.len())
    {
        return Err(invalid(format!(
            "Sword array at 0x{descriptor:X} extends outside its owner"
        )));
    }
    Ok((count, rows))
}

fn profile_banks(owner: &[u8], resource: usize) -> AuthoringResult<Vec<Bank>> {
    let mut banks = Vec::new();
    for descriptor in (resource..owner.len().saturating_sub(16)).step_by(8) {
        if read_u64(owner, descriptor).is_err() {
            continue;
        }
        let Ok((count, _, rows, class)) = array_at(owner, descriptor) else {
            continue;
        };
        if class != PROFILE_CLASS {
            continue;
        }
        let (checked_count, checked_rows) =
            checked_array(owner, descriptor, PROFILE_CLASS, PROFILE_SIZE)?;
        if checked_count != count || checked_rows != rows {
            return Err(invalid("Sword profile descriptor changed while validating"));
        }
        banks.push(Bank {
            descriptor,
            rows,
            count,
        });
    }
    if banks.is_empty() || banks.len() > 32 {
        return Err(invalid(format!(
            "Sword component has {} checked profile banks, expected 1 through 32",
            banks.len()
        )));
    }
    Ok(banks)
}

fn profile_pointers(owner: &[u8], row: usize) -> AuthoringResult<Vec<(usize, usize)>> {
    let mut pointers = Vec::new();
    for &(offset, class, stride) in ARRAY_FIELDS {
        let count = read_u64(owner, row + offset - 8)?;
        let relative = read_i64(owner, row + offset)?;
        if count == 0 && relative == 0 {
            continue;
        }
        if count == 0 || relative == 0 {
            return Err(invalid(format!(
                "Sword profile at 0x{row:X} has a partial array at +0x{offset:X}"
            )));
        }
        let target = relative_target(owner, row + offset)?;
        if target < 4
            || read_u32(owner, target - 4)? != ARRAY_MARKER
            || read_u64(owner, target)? != count
            || read_u64(owner, target + 8)? != u64::from(class)
        {
            return Err(invalid(format!(
                "Sword profile at 0x{row:X} has an unsupported array at +0x{offset:X}"
            )));
        }
        let (checked_count, checked_rows) = checked_array(owner, row + offset - 8, class, stride)?;
        if checked_count != usize::try_from(count).unwrap_or(usize::MAX)
            || checked_rows != target + 16
        {
            return Err(invalid(format!(
                "Sword profile at 0x{row:X} has an unsupported array extent at +0x{offset:X}"
            )));
        }
        pointers.push((offset, target));
    }
    let typed = relative_target(owner, row + 0x1A8)?;
    let typed_size = match read_u32(owner, typed.saturating_sub(4))? {
        0x8080_4454 => 0x2C,
        0x8080_4455 => 0x6C,
        _ => 0,
    };
    if typed < 4
        || typed_size == 0
        || typed
            .checked_add(typed_size)
            .is_none_or(|end| end > owner.len())
    {
        return Err(invalid(format!(
            "Sword profile at 0x{row:X} has an unsupported typed resource"
        )));
    }
    pointers.push((0x1A8, typed));
    if read_i64(owner, row + 0x110)? != 0 {
        let optional = relative_target(owner, row + 0x110)?;
        if optional < 4
            || read_u32(owner, optional - 4)? != 0x8080_692C
            || optional
                .checked_add(0x48)
                .is_none_or(|end| end > owner.len())
        {
            return Err(invalid(format!(
                "Sword profile at 0x{row:X} has an unsupported optional typed resource"
            )));
        }
        pointers.push((0x110, optional));
    }
    for &(_, target) in &pointers {
        if (row..row + PROFILE_SIZE).contains(&target) {
            return Err(invalid(format!(
                "Sword profile at 0x{row:X} points into its own row"
            )));
        }
    }
    Ok(pointers)
}

fn scalar(owner: &[u8], row: usize, program: usize) -> AuthoringResult<u32> {
    let (code_count, code_rows) = checked_array(owner, row + program, 0x8080_0009, 1)?;
    if code_count != 4 || owner[code_rows..code_rows + 4] != [0x34, 0, 0x3E, 0] {
        return Err(invalid(format!(
            "Sword profile at 0x{row:X} has an unsupported scalar program"
        )));
    }
    let (constant_count, constant_rows) =
        checked_array(owner, row + program + 16, 0x8080_0090, 16)?;
    if constant_count != 1 {
        return Err(invalid(format!(
            "Sword profile at 0x{row:X} has more than one scalar constant"
        )));
    }
    let bits = read_u32(owner, constant_rows)?;
    if !f32::from_bits(bits).is_finite()
        || (1..4).any(|lane| read_u32(owner, constant_rows + 4 * lane).ok() != Some(bits))
    {
        return Err(invalid(format!(
            "Sword profile at 0x{row:X} has a nonuniform scalar constant"
        )));
    }
    Ok(bits)
}

fn aligned_array(bytes: &mut Vec<u8>, absolute_base: usize, count: usize, class: u32) -> usize {
    let marker_at = absolute_base + bytes.len();
    let padding = (16 - (marker_at + 4) % 16) % 16;
    bytes.resize(bytes.len() + padding, 0);
    let header = bytes.len() + 4;
    bytes.extend_from_slice(&ARRAY_MARKER.to_le_bytes());
    bytes.extend_from_slice(&(count as u64).to_le_bytes());
    bytes.extend_from_slice(&u64::from(class).to_le_bytes());
    header
}

fn relative(from: usize, to: usize) -> AuthoringResult<i64> {
    i64::try_from(to)
        .and_then(|to| i64::try_from(from).map(|from| to - from))
        .map_err(|_| invalid("Sword profile relative reference overflows"))
}

fn append_scalar(
    bytes: &mut Vec<u8>,
    absolute_base: usize,
    profile: usize,
    program: usize,
    value: f32,
) -> AuthoringResult<()> {
    if !value.is_finite() {
        return Err(invalid("Sword profile scalar is not finite"));
    }
    let header = aligned_array(bytes, absolute_base, 1, 0x8080_0090);
    for _ in 0..4 {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    let pointer = profile + program + 24;
    write_i64(
        bytes,
        pointer,
        relative(absolute_base + pointer, absolute_base + header)?,
    )
}

/// Plan growth of the selected sword owner's profile and key arrays. The caller
/// appends these records before any unrelated appends to the same owner.
pub(super) fn edits(
    manager: &PackageManager,
    entity: &[u8],
    profile: SwordProfileOverride,
) -> AuthoringResult<Edits> {
    let near = f32::from_bits(profile.near_scale_bits);
    let far = f32::from_bits(profile.far_scale_bits);
    if !near.is_finite()
        || !far.is_finite()
        || near <= 0.0
        || far <= 0.0
        || matches!(profile.key, 0 | u32::MAX | EMPTY_KEY)
    {
        return Err(invalid("Sword profile key or angular scales are invalid"));
    }
    let [binding] = weapon_component_bindings(entity, BINDING)
        .map_err(invalid)?
        .try_into()
        .map_err(|_| invalid("Sword profile binding must select one resource"))?;
    if binding.concrete_class != SWORD_CLASS {
        return Err(invalid(
            "Sword profile binding has an unsupported concrete class",
        ));
    }
    let owner_tag = TagHash(binding.owner_tag);
    let entry = manager
        .get_entry(owner_tag)
        .ok_or_else(|| invalid("Sword profile owner is not live"))?;
    if entry.file_type != 8 {
        return Err(invalid("Sword profile owner is not a structured resource"));
    }
    let owner = read_tag(manager, owner_tag, "sword profile owner")?;
    if usize::try_from(read_u64(&owner, 0)?).ok() != Some(owner.len()) {
        return Err(invalid("Sword profile owner file size differs"));
    }
    let resource = usize::try_from(binding.resource_offset)
        .map_err(|_| invalid("Sword profile resource offset is too large"))?;
    if resource < 4
        || read_u32(&owner, resource - 4)? != SWORD_CLASS
        || read_u32(&owner, resource)? != owner_tag.0
    {
        return Err(invalid("Sword profile resource does not match its binding"));
    }
    let key_slot = resource + KEY_DESCRIPTOR;
    let (key_capacity, key_rows) = checked_array(&owner, key_slot, KEY_CLASS, 8)?;
    if key_capacity >= 128
        || usize::try_from(read_u32(&owner, resource + KEY_ACTIVE_COUNT)?)
            .ok()
            .is_none_or(|active| active > key_capacity)
    {
        return Err(invalid(
            "Sword profile key capacity or active count is invalid",
        ));
    }
    for index in 0..key_capacity {
        if read_u32(&owner, key_rows + index * 8)? == profile.key {
            return Err(invalid("Sword profile key already exists in the donor"));
        }
    }
    let banks = profile_banks(&owner, resource)?;
    let mut appended = Vec::with_capacity(banks.len() + 1);
    let mut patches = Vec::with_capacity(banks.len() + 1);
    let mut next = owner.len();
    for bank in banks {
        let count = bank
            .count
            .checked_mul(2)
            .ok_or_else(|| invalid("Sword profile count overflows"))?;
        let mut bytes = Vec::new();
        let header = aligned_array(&mut bytes, next, count, PROFILE_CLASS);
        let rows = bytes.len();
        bytes.resize(
            rows + count
                .checked_mul(PROFILE_SIZE)
                .ok_or_else(|| invalid("Sword profile growth overflows"))?,
            0,
        );
        for index in 0..bank.count {
            let old = bank.rows + index * PROFILE_SIZE;
            if read_u32(&owner, old + 12)? != EMPTY_KEY {
                return Err(invalid(format!(
                    "Sword profile at 0x{old:X} already has a nonempty gate"
                )));
            }
            let pointers = profile_pointers(&owner, old)?;
            let near_original = f32::from_bits(scalar(&owner, old, 0xA8)?);
            let far_original = f32::from_bits(scalar(&owner, old, 0xE0)?);
            scalar(&owner, old, 0x70)?;
            for variant in 0..2 {
                let new = rows + (index * 2 + variant) * PROFILE_SIZE;
                bytes[new..new + PROFILE_SIZE].copy_from_slice(&owner[old..old + PROFILE_SIZE]);
                for &(offset, target) in &pointers {
                    write_i64(
                        &mut bytes,
                        new + offset,
                        relative(next + new + offset, target)?,
                    )?;
                }
                if variant == 0 {
                    write_u32(&mut bytes, new + 12, profile.key)?;
                    append_scalar(&mut bytes, next, new, 0xA8, near_original * near)?;
                    append_scalar(&mut bytes, next, new, 0xE0, far_original * far)?;
                }
            }
        }
        let target = next
            .checked_add(header)
            .ok_or_else(|| invalid("Sword profile header offset overflows"))?;
        patches.push(descriptor_patch(resource, bank.descriptor, target, count)?);
        next = next
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("Sword profile owner growth overflows"))?;
        appended.push(WeaponRuntimeResourceAppend {
            binding_hash: BINDING,
            resource_index: 0,
            bytes,
            slots: Vec::new(),
            arrays: Vec::new(),
            pointers: Vec::new(),
            references: Vec::new(),
        });
    }
    let mut bytes = Vec::new();
    let header = aligned_array(&mut bytes, next, key_capacity + 1, KEY_CLASS);
    bytes.extend_from_slice(&owner[key_rows..key_rows + key_capacity * 8]);
    bytes.extend_from_slice(&[0; 8]);
    let target = next
        .checked_add(header)
        .ok_or_else(|| invalid("Sword key array header offset overflows"))?;
    patches.push(descriptor_patch(
        resource,
        key_slot,
        target,
        key_capacity + 1,
    )?);
    appended.push(WeaponRuntimeResourceAppend {
        binding_hash: BINDING,
        resource_index: 0,
        bytes,
        slots: Vec::new(),
        arrays: Vec::new(),
        pointers: Vec::new(),
        references: Vec::new(),
    });
    Ok(Edits {
        appends: appended,
        patches,
    })
}
