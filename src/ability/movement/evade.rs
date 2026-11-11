//! Evade profiles of a movement controller, and adding one of its own.
//!
//! Read from the Shadowkeep movement controller `80BC3572` (definition `808044E0`, instance
//! `808043AE`, build 86657.20.08.23.1800.d2_rc___release). The definition's configuration
//! (`8080410F`, named by a relative pointer at definition `+0x800`) holds the evade profiles at
//! `+0x10` as a native array of `0x80`-byte `80804106` rows. Its instance twin (`8080410D`) holds
//! their runtime rows at `+0x20` as `0x60`-byte `80804105` rows, one for each profile in the same
//! order. A movement bank's `80804121` selector row enables the profile whose identity is at
//! profile `+0x10`.
//!
//! A profile row: `+0x00` a typed reference to its runtime row, `+0x10` identity, `+0x14..+0x1C`
//! energy spent and recovery, `+0x20` an inline condition object (header class `80803BFA`), `+0x30` its
//! condition list, `+0x40/+0x44` readiness, `+0x50` the movement effect pattern, `+0x58` the
//! eligibility mask, `+0x60..+0x6C` the four directional human actions it plays, `+0x70` the
//! input it answers and `+0x78` the preparation (`808044BF`) that moves the body. A runtime row:
//! `+0x00` a typed reference to its profile, `+0x10` a pointer to the configuration's instance,
//! `+0x20` the preparation's instance (`808044BE`), `+0x30` the condition object's instance
//! (header class `80803BFB`) and
//! `+0x40` a pointer back at the row, `+0x50` its condition list. The pointer fields follow the
//! native reference schemas of both classes. Each paired object begins with a typed reference that
//! names its twin: the owner, the twin's class and the twin's offset.
//!
//! An added profile copies a template profile and its runtime row, under its own identity and
//! actions. Both arrays are rebuilt one row longer: the runtime rows at the end of the instance
//! region, which the owner header sizes at `+0x48` and the client copies for each instance, and
//! the profiles at the end of the payload. The definition region moves down by the inserted
//! bytes, so no instance object moves and the resource map displacements of the graph stay
//! correct. Every typed reference into the moved bytes or the old rows follows them, in the
//! controller and in each referrer. The new profile shares the template's preparation, which
//! the graph registers as an interface resource by its offsets. The old rows remain as
//! unreachable bytes.
use crate::package_payload::{i64_at, relative_offset, u32_at, u64_at, write_bytes};
use std::collections::BTreeSet;

const INSTANCE_POINTER: usize = 0x10;
const DEFINITION_POINTER: usize = 0x18;
/// The owner header's instance region size, a length that can read like a pointer.
const INSTANCE_SIZE: usize = 0x48;
const INSTANCE_CLASS: u32 = 0x8080_43AE;
const DEFINITION_CLASS: u32 = 0x8080_44E0;
const CONFIGURATION_POINTER: usize = 0x800;
const CONFIGURATION_CLASS: u32 = 0x8080_410F;
const CONFIGURATION_INSTANCE_CLASS: u32 = 0x8080_410D;
const PROFILES: usize = 0x10;
const RUNTIME_ROWS: usize = 0x20;
const PROFILE_CLASS: u32 = 0x8080_4106;
const RUNTIME_CLASS: u32 = 0x8080_4105;
const PROFILE_SIZE: usize = 0x80;
const RUNTIME_SIZE: usize = 0x60;
/// The header words of the profile's inline condition object and of its instance.
const PROFILE_CONDITION_HEADER: u32 = 0x8080_3BFA;
const RUNTIME_CONDITION_HEADER: u32 = 0x8080_3BFB;
const ARRAY_MARKER: u32 = 0x8080_9FBD;
const IDENTITY: usize = 0x10;
const PROFILE_CONDITION: usize = 0x20;
const PROFILE_CONDITIONS: usize = 0x30;
const ACTIONS: usize = 0x60;
const RUNTIME_CONDITION: usize = 0x30;
const RUNTIME_CONDITIONS: usize = 0x50;
/// The relative pointers each row declares, from schemas `80804106` and `80804105`.
const PROFILE_POINTERS: [usize; 3] = [0x38, 0x48, 0x78];
const RUNTIME_POINTERS: [usize; 4] = [0x10, 0x20, 0x40, 0x58];

/// An evade profile of a controller's own.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    /// The identity a selector row names.
    pub identity: u32,
    /// The identity of the profile it copies.
    pub template: u32,
    /// The four directional human actions, in the template's order.
    pub actions: [u32; 4],
    /// Leave out the template's eligibility conditions. Without it a template with conditions is
    /// refused, since its runtime condition objects would need copies of their own.
    pub ungated: bool,
}

impl Profile {
    fn validate(self) -> Result<(), String> {
        if [0, u32::MAX, 0x811C_9DC5].contains(&self.identity)
            || self
                .actions
                .iter()
                .any(|&action| [0, u32::MAX].contains(&action))
        {
            return Err("An evade profile needs an identity and four actions".into());
        }
        Ok(())
    }
}

struct Layout {
    owner: u32,
    instance: usize,
    definition: usize,
    configuration: usize,
    configuration_instance: usize,
    count: usize,
    profiles: usize,
    runtime: usize,
}

impl Layout {
    fn read(data: &[u8]) -> Result<Self, String> {
        let instance = target(data, INSTANCE_POINTER)?;
        let definition = target(data, DEFINITION_POINTER)?;
        if instance < 4
            || definition < instance
            || u32_at(data, instance - 4)? != INSTANCE_CLASS
            || u32_at(data, definition - 4)? != DEFINITION_CLASS
            || u64_at(data, INSTANCE_SIZE)? != (definition - instance) as u64
        {
            return Err("The payload is not a movement controller with evade profiles".into());
        }
        let owner = u32_at(data, instance)?;
        let configuration = target(data, definition + CONFIGURATION_POINTER)?;
        let configuration_instance =
            typed(data, configuration, owner, CONFIGURATION_INSTANCE_CLASS)?;
        if u32_at(data, configuration - 4)? != CONFIGURATION_CLASS
            || typed(data, configuration_instance, owner, CONFIGURATION_CLASS)? != configuration
        {
            return Err("The movement controller's profile configuration is not paired".into());
        }
        let (count, profiles) = array(data, configuration + PROFILES, PROFILE_CLASS)?;
        let (runtime_count, runtime) =
            array(data, configuration_instance + RUNTIME_ROWS, RUNTIME_CLASS)?;
        if count == 0 || count != runtime_count || count > 32 {
            return Err("The movement controller's profile arrays differ".into());
        }
        let layout = Self {
            owner,
            instance,
            definition,
            configuration,
            configuration_instance,
            count,
            profiles,
            runtime,
        };
        for index in 0..count {
            let (profile, row) = (layout.profile(index), layout.row(index));
            if runtime >= definition
                || profiles < definition
                || typed(data, profile, owner, RUNTIME_CLASS)? != row
                || typed(data, row, owner, PROFILE_CLASS)? != profile
                || typed(
                    data,
                    profile + PROFILE_CONDITION,
                    owner,
                    PROFILE_CONDITION_HEADER,
                )? != row + RUNTIME_CONDITION
                || typed(
                    data,
                    row + RUNTIME_CONDITION,
                    owner,
                    RUNTIME_CONDITION_HEADER,
                )? != profile + PROFILE_CONDITION
            {
                return Err(format!(
                    "Movement controller profile {index} is not paired with its runtime row"
                ));
            }
        }
        Ok(layout)
    }

    fn profile(&self, index: usize) -> usize {
        self.profiles + index * PROFILE_SIZE
    }

    fn row(&self, index: usize) -> usize {
        self.runtime + index * RUNTIME_SIZE
    }

    fn identities(&self, data: &[u8]) -> Result<Vec<u32>, String> {
        (0..self.count)
            .map(|index| u32_at(data, self.profile(index) + IDENTITY))
            .collect()
    }
}

/// Where the bytes of a controller go when its profile arrays are rebuilt: the definition
/// region moves down by `delta`, and each old row's bytes are read at its new row.
struct Relocation {
    cut: usize,
    delta: usize,
    old: Layout,
    profiles: usize,
    runtime: usize,
}

impl Relocation {
    fn moved(&self, offset: usize) -> usize {
        if offset >= self.cut {
            offset + self.delta
        } else {
            offset
        }
    }

    /// Where an offset names after the rebuild. An offset inside an old row names the same
    /// bytes of its new row.
    fn named(&self, offset: usize) -> usize {
        let old = &self.old;
        let runtime = old.runtime..old.runtime + old.count * RUNTIME_SIZE;
        let profiles = old.profiles..old.profiles + old.count * PROFILE_SIZE;
        if runtime.contains(&offset) {
            self.runtime + (offset - old.runtime)
        } else if profiles.contains(&offset) {
            self.profiles + (offset - old.profiles)
        } else {
            self.moved(offset)
        }
    }

    /// The relocation from `before` to `after`, read from their layouts.
    fn between(before: &[u8], after: &[u8]) -> Result<Option<Self>, String> {
        let old = Layout::read(before)?;
        let new = Layout::read(after)?;
        if new.owner != old.owner || new.definition < old.definition || new.count < old.count {
            return Err("The edited movement controller does not extend its stock layout".into());
        }
        if new.definition == old.definition {
            return Ok(None);
        }
        Ok(Some(Self {
            cut: old.definition - 4,
            delta: new.definition - old.definition,
            profiles: new.profiles,
            runtime: new.runtime,
            old,
        }))
    }
}

/// Whether `data` is a movement controller with evade profiles.
#[must_use]
pub fn is_controller(data: &[u8]) -> bool {
    Layout::read(data).is_ok()
}

/// The identities of a controller's evade profiles, in order.
pub fn identities(data: &[u8]) -> Result<Vec<u32>, String> {
    let layout = Layout::read(data)?;
    layout.identities(data)
}

/// The four directional human actions of the profile `identity`.
pub fn actions(data: &[u8], identity: u32) -> Result<[u32; 4], String> {
    let layout = Layout::read(data)?;
    let index = find(&layout, data, identity)?;
    let mut actions = [0; 4];
    for (lane, action) in actions.iter_mut().enumerate() {
        *action = u32_at(data, layout.profile(index) + ACTIONS + lane * 4)?;
    }
    Ok(actions)
}

fn find(layout: &Layout, data: &[u8], identity: u32) -> Result<usize, String> {
    let found = layout
        .identities(data)?
        .iter()
        .enumerate()
        .filter(|(_, value)| **value == identity)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match found.as_slice() {
        [index] => Ok(*index),
        [] => Err(format!(
            "The movement controller has no profile {identity:08X}"
        )),
        _ => Err(format!(
            "The movement controller repeats profile {identity:08X}"
        )),
    }
}

/// `data` with `profile` added after its other profiles, copied from its template. A profile
/// with the same identity already present is left as it is when it plays the same actions.
pub fn with_profile(data: &[u8], profile: Profile) -> Result<Vec<u8>, String> {
    let old = Layout::read(data)?;
    let identities = old.identities(data)?;
    if identities.contains(&profile.identity) {
        return unchanged_profile(data, profile);
    }
    profile.validate()?;
    let template = find(&old, data, profile.template)?;
    let (template_profile, template_row) = (old.profile(template), old.row(template));
    // The template's conditions would need copies of their own, unless the copy has none.
    if !profile.ungated
        && (u64_at(data, template_profile + PROFILE_CONDITIONS)? != 0
            || u64_at(data, template_row + RUNTIME_CONDITIONS)? != 0)
    {
        return Err("The template evade profile has conditions of its own".into());
    }
    let count = old.count + 1;
    // The runtime rows at the end of the instance region, before the definition's marker.
    let cut = old.definition - 4;
    let mut out = data[..cut].to_vec();
    let runtime_header = open_array(&mut out, count, RUNTIME_CLASS);
    let runtime = runtime_header + 16;
    out.resize(runtime + count * RUNTIME_SIZE, 0);
    // The definition keeps its alignment.
    out.resize(cut + (out.len() - cut).next_multiple_of(16), 0);
    let delta = out.len() - cut;
    out.extend_from_slice(&data[cut..]);
    let profile_header = open_array(&mut out, count, PROFILE_CLASS);
    let profiles = profile_header + 16;
    out.resize((profiles + count * PROFILE_SIZE).next_multiple_of(16), 0);
    let relocation = Relocation {
        cut,
        delta,
        profiles,
        runtime,
        old,
    };
    let old = &relocation.old;

    relocate_shared(data, &mut out, &relocation)?;

    // Each row copies its old row, the added one the template's, and names its own twins.
    for index in 0..count {
        let source = if index < old.count { index } else { template };
        let (from_profile, from_row) = (old.profile(source), old.row(source));
        let (to_profile, to_row) = (
            profiles + index * PROFILE_SIZE,
            runtime + index * RUNTIME_SIZE,
        );
        let own = |offset: usize| {
            if (from_row..from_row + RUNTIME_SIZE).contains(&offset) {
                to_row + (offset - from_row)
            } else if (from_profile..from_profile + PROFILE_SIZE).contains(&offset) {
                to_profile + (offset - from_profile)
            } else {
                relocation.named(offset)
            }
        };
        out[to_row..to_row + RUNTIME_SIZE]
            .copy_from_slice(&data[from_row..from_row + RUNTIME_SIZE]);
        out[to_profile..to_profile + PROFILE_SIZE]
            .copy_from_slice(&data[from_profile..from_profile + PROFILE_SIZE]);
        for (from, to, fields) in [
            (from_row, to_row, &RUNTIME_POINTERS[..]),
            (from_profile, to_profile, &PROFILE_POINTERS[..]),
        ] {
            for &field in fields {
                if i64_at(data, from + field)? != 0 {
                    put_pointer(&mut out, to + field, own(target(data, from + field)?))?;
                }
            }
        }
        for (from, to, fields) in [
            (from_row, to_row, [0, RUNTIME_CONDITION]),
            (from_profile, to_profile, [0, PROFILE_CONDITION]),
        ] {
            for field in fields {
                let named = usize::try_from(u64_at(data, from + field + 8)?)
                    .map_err(|_| "Offset overflow")?;
                put_u64(&mut out, to + field + 8, own(named) as u64)?;
            }
        }
        if index == old.count && profile.ungated {
            // An empty condition list is a zero count and pointer, as stock profiles without
            // conditions store it.
            for at in [to_profile + PROFILE_CONDITIONS, to_row + RUNTIME_CONDITIONS] {
                write_bytes(&mut out, at, &[0; 16])?;
            }
        }
        if index == old.count {
            write_bytes(
                &mut out,
                to_profile + IDENTITY,
                &profile.identity.to_le_bytes(),
            )?;
            for (lane, action) in profile.actions.iter().enumerate() {
                write_bytes(
                    &mut out,
                    to_profile + ACTIONS + lane * 4,
                    &action.to_le_bytes(),
                )?;
            }
        }
    }
    let configuration = relocation.moved(old.configuration);
    put_u64(&mut out, configuration + PROFILES, count as u64)?;
    put_pointer(&mut out, configuration + PROFILES + 8, profile_header)?;
    put_u64(
        &mut out,
        old.configuration_instance + RUNTIME_ROWS,
        count as u64,
    )?;
    put_pointer(
        &mut out,
        old.configuration_instance + RUNTIME_ROWS + 8,
        runtime_header,
    )?;
    put_u64(
        &mut out,
        INSTANCE_SIZE,
        (old.definition + delta - old.instance) as u64,
    )?;
    let length = out.len() as u64;
    put_u64(&mut out, 0, length)?;

    let layout = Layout::read(&out)?;
    if layout.identities(&out)? != [identities, vec![profile.identity]].concat()
        || actions(&out, profile.identity)? != profile.actions
    {
        return Err("The extended movement controller does not read back".into());
    }
    Ok(out)
}

fn relocate_shared(data: &[u8], out: &mut [u8], relocation: &Relocation) -> Result<(), String> {
    let old = &relocation.old;
    let typed_slots = typed_references(data, old.owner);
    let skipped = typed_slots
        .iter()
        .flat_map(|&at| [at, at + 8])
        .chain([0, INSTANCE_SIZE])
        .collect::<BTreeSet<_>>();
    let named_by_typed = typed_slots
        .iter()
        .filter_map(|&at| usize::try_from(u64_at(data, at + 8).ok()?).ok())
        .chain(typed_slots.iter().copied())
        .collect::<BTreeSet<_>>();
    let old_rows = (
        old.runtime..old.runtime + old.count * RUNTIME_SIZE,
        old.profiles..old.profiles + old.count * PROFILE_SIZE,
    );
    let dead = |at: usize| old_rows.0.contains(&at) || old_rows.1.contains(&at);

    for at in (0..data.len().saturating_sub(7)).step_by(8) {
        if skipped.contains(&at) || dead(at) {
            continue;
        }
        let Ok(named) = target(data, at) else {
            continue;
        };
        if named % 8 != 0 {
            continue;
        }
        let (slot, moved) = (relocation.moved(at), relocation.named(named));
        if moved as i64 - slot as i64 == named as i64 - at as i64 {
            continue;
        }
        let marked = named >= 4 && u32_at(data, named - 4)? & 0xFFFF_0000 == 0x8080_0000;
        if !marked && !named_by_typed.contains(&named) && at >= 0x60 {
            return Err(format!(
                "The movement controller has an unrecognized reference at 0x{at:X} across its rebuilt rows"
            ));
        }
        put_pointer(out, slot, moved)?;
    }
    for &at in &typed_slots {
        if dead(at) {
            continue;
        }
        let named = usize::try_from(u64_at(data, at + 8)?).map_err(|_| "Offset overflow")?;
        put_u64(
            out,
            relocation.moved(at) + 8,
            relocation.named(named) as u64,
        )?;
    }

    Ok(())
}

fn unchanged_profile(data: &[u8], profile: Profile) -> Result<Vec<u8>, String> {
    if actions(data, profile.identity)? == profile.actions {
        Ok(data.to_vec())
    } else {
        Err(format!(
            "Movement controller profile {:08X} already plays other actions",
            profile.identity
        ))
    }
}

/// `referrer` with every typed reference into `owner` moved from the layout of `before` to that
/// of `after`, a version of the controller [`with_profile`] extended. `None` when nothing moved.
pub fn retarget_references(
    referrer: &[u8],
    owner: u32,
    before: &[u8],
    after: &[u8],
) -> Result<Option<Vec<u8>>, String> {
    let Some(relocation) = Relocation::between(before, after)? else {
        return Ok(None);
    };
    if relocation.old.owner != owner {
        return Err("The movement controller names another owner".into());
    }
    let mut out = referrer.to_vec();
    let mut moved = false;
    for at in typed_references(referrer, owner) {
        let Ok(named) = usize::try_from(u64_at(referrer, at + 8)?) else {
            continue;
        };
        if named >= before.len() {
            continue;
        }
        let next = relocation.named(named);
        if next != named {
            put_u64(&mut out, at + 8, next as u64)?;
            moved = true;
        }
    }
    Ok(moved.then_some(out))
}

/// Opens a native array of `count` rows of `class` at the next place whose header is 16-byte
/// aligned with its marker in the four bytes before it. Returns the header.
fn open_array(out: &mut Vec<u8>, count: usize, class: u32) -> usize {
    let header = (out.len() + 4).next_multiple_of(16);
    out.resize(header + 16, 0);
    out[header - 4..header].copy_from_slice(&ARRAY_MARKER.to_le_bytes());
    out[header..header + 8].copy_from_slice(&(count as u64).to_le_bytes());
    out[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
    header
}

/// The rows of the native array whose descriptor is at `at`: their count and first row.
fn array(data: &[u8], at: usize, class: u32) -> Result<(usize, usize), String> {
    let count = usize::try_from(u64_at(data, at)?).map_err(|_| "Array count overflow")?;
    let header = target(data, at + 8)?;
    if header < 4
        || u32_at(data, header - 4)? != ARRAY_MARKER
        || u64_at(data, header)? != count as u64
        || u32_at(data, header + 8)? != class
    {
        return Err(format!("Expected an array of 0x{class:08X} at 0x{at:X}"));
    }
    Ok((count, header + 16))
}

/// The offset a typed reference of `class` to `owner` at `at` names.
fn typed(data: &[u8], at: usize, owner: u32, class: u32) -> Result<usize, String> {
    if u32_at(data, at)? != owner || u32_at(data, at + 4)? != class {
        return Err(format!("Expected a reference to 0x{class:08X} at 0x{at:X}"));
    }
    usize::try_from(u64_at(data, at + 8)?).map_err(|_| "Offset overflow".into())
}

/// The aligned typed references to `owner`: the tag, a native class, then an offset.
fn typed_references(data: &[u8], owner: u32) -> Vec<usize> {
    (0..data.len().saturating_sub(15))
        .step_by(8)
        .filter(|&at| {
            u32_at(data, at) == Ok(owner)
                && u32_at(data, at + 4).is_ok_and(|class| class & 0xFFFF_0000 == 0x8080_0000)
        })
        .collect()
}

/// The offset a self-relative pointer at `at` names, when it names one inside the payload.
fn target(data: &[u8], at: usize) -> Result<usize, String> {
    let value = i64_at(data, at)?;
    if value == 0 {
        return Err(format!("No pointer at 0x{at:X}"));
    }
    relative_offset(at, 0, value)
        .ok()
        .filter(|named| *named < data.len())
        .ok_or_else(|| format!("The pointer at 0x{at:X} leaves the payload"))
}

fn put_u64(data: &mut [u8], at: usize, value: u64) -> Result<(), String> {
    write_bytes(data, at, &value.to_le_bytes())
}

fn put_pointer(data: &mut [u8], at: usize, named: usize) -> Result<(), String> {
    let relative = i64::try_from(named)
        .ok()
        .zip(i64::try_from(at).ok())
        .map(|(named, at)| named - at)
        .ok_or("Pointer overflow")?;
    write_bytes(data, at, &relative.to_le_bytes())
}
