//! A map's own object registry: the scripted objects its slice set loads, as the donor's activity
//! places them, with every point a player starts or respawns at moved onto the map's own spawn
//! points, the donor's kill volumes refilled with the map's own, its out-of-bounds volumes left
//! out, and the donor's barriers, props and effects in its object lists moved far below the map.
//! The donor's music sensor is left out, and objective records that place something on the map
//! with no role move far below it. The donor's AI spawners are left out too, and when the level
//! has squads, the copy adds an encounter object of its own whose squads stand where the level's
//! do, built from the donor's ([`super::encounters`]).
//!
//! A slice set names a registry at `+0x14`, whose three arrays list objects. Each object lists,
//! per bubble, handles whose redirects name placed records. A placed record's descriptors each
//! name the record and its component class, and the descriptor that carries the object's key
//! holds a point's rotation, position and scale at `+0x60`. Distant Shore's Crucible object holds
//! 34 respawn points and 44 named points (Rumble starts and each mode's landing points) that way.
//! A handle may instead name an object list, a placement table. Distant Shore's Crucible object
//! names one of 873 rows: invisible walls and kill boxes (entities with only a physical body),
//! rocks and other props, ambient effects, and 25 logical nodes that draw and collide with
//! nothing, at the donor's objective positions. The copy names its own copy of such a list, whose
//! rows with those entities move far below the map and keep their indices, and whose remaining
//! rows move onto map spawns as the points do. An object that names the donor's bubble names the
//! map's in its copy.
use std::path::Path;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

mod loading;

use parhelion_import::halo_reach::level::{
    objectives::Objective, spawn::Spawn, squads::Squad, volumes::Volume,
};
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult, NewTagSpec, NewTagStorageMode,
    error::invalid,
    shared_tag_dependency_index::native::Companions,
    tag_payload::{
        append_native_array, array_at, read_tag, read_u32, read_u64, synchronize_payload_size,
        write_u32,
    },
};

use super::{
    chain,
    encounters::{Encounter, Templates},
    placed::words,
    world::{BubbleName, Owner},
};

const REGISTRY_CLASS: u32 = 0x8080_925E;
/// A registry's three arrays of object tags.
const REGISTRY_OBJECTS: [usize; 3] = [0x08, 0x18, 0x28];
const OBJECT_CLASS: u32 = 0x8080_9462;
const OBJECT_KEY: usize = 0x0C;
/// An object's per-bubble blocks, each naming its handles at `+0x08`.
const OBJECT_BUBBLES: usize = 0x38;
const BUBBLE_SIZE: usize = 24;
const BUBBLE_HANDLES: usize = 0x08;
/// A handle names a list at `+0x08`, or none, and its redirects at `+0x10`.
const HANDLE_CLASS: u32 = 0x8080_9468;
const HANDLE_LIST: usize = 0x08;
const HANDLE_REDIRECTS: usize = 0x10;
/// A redirect names its placed record twice.
const REDIRECT_CLASS: u32 = 0x8080_9B14;
const REDIRECT_RECORD: [usize; 2] = [0x0C, 0x10];
const PLACED_CLASS: u32 = 0x8080_9C36;
/// A descriptor names its record, then its component class, then holds this mark.
const DESCRIPTOR_CLASS: usize = 0x04;
const DESCRIPTOR_MARK: usize = 0x08;
const MARK: u32 = 0x70;
const DESCRIPTOR_KEY: usize = 0x30;
const DESCRIPTOR_SIZE: usize = 0x80;
/// A placed record's default instance and its definition, the descriptor the runtime reads its
/// slot from. The definition is an 88-byte slot header whose `+0x08` holds the default instance's
/// offset, whose `+0x36` holds the slot index and whose `+0x44` and `+0x48` hold its schemas.
const RECORD_INSTANCE: usize = 0x10;
const RECORD_DEFINITION: usize = 0x18;
const SLOT_HEADER: usize = 88;
const DESCRIPTOR_SLOT: usize = 0x34;
const DESCRIPTOR_SENSE: usize = 0x44;
const DESCRIPTOR_AUTH: usize = 0x48;
/// A point's rotation quaternion, position and scale.
const POINT_ROTATION: usize = 0x60;
const POINT_POSITION: usize = 0x70;
const POINT_SCALE: usize = 0x7C;
/// An object list, a placement table, whose rows are at `+0x08`.
const LIST_CLASS: u32 = 0x8080_99D6;
const TABLE_ROWS: usize = 0x08;
/// Component classes: a respawn point, a named point, a trigger volume and an objective.
const RESPAWN_POINT: u32 = 0x8080_94D1;
const NAMED_POINT: u32 = 0x8080_94D5;
const VOLUME: u32 = 0x8080_99C8;
const OBJECTIVE: u32 = 0x8080_9927;
/// The component class of a mission's music sensor (slot type 11, `m_music_sensor`), which plays
/// the donor's score. A map plays its own level's ambience, so its copy leaves the sensor out:
/// Lone Wolf and Tip of the Spear played The Other Side's music over Reach's sound.
const MUSIC_SENSOR: u32 = 0x8080_4E8E;
/// The component class of an AI spawner, which spawns the donor's combatants as its encounter
/// script asks. The Other Side's registry places 112 at its own rooms, and every one is an object
/// the client creates, so the copy leaves them out and builds its own squads from them.
const AI_SPAWNER: u32 = 0x8080_9A3B;
/// An objective record embeds one placement row, the row class of an object list, behind an
/// array marker and a 16-byte header.
const ARRAY_MARKER: u32 = 0x8080_9FBD;
const PLACEMENT_ROW_CLASS: u32 = 0x8080_99D8;
/// The name an objective record carries, which gives its role.
const OBJECTIVE_NAME: &[u8] = b"pvp_controller.";

/// What an objective is to a game mode, from the name its record carries. Each role prefers
/// certain Reach labels for its positions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Role {
    /// A capture zone or an Iron Banner firepit.
    Zone,
    /// A bomb arming site.
    Plant,
    /// A team's home base.
    Base,
    /// A neutral base or a tiebreaker.
    Center,
}

impl Role {
    /// The role an objective record's name gives, or none for a record the copy keeps as it is.
    fn of(name: &[u8]) -> Option<Self> {
        let has = |part: &str| {
            name.windows(part.len())
                .any(|window| window == part.as_bytes())
        };
        if has("capture_zones") || has("firepits") {
            Some(Self::Zone)
        } else if has("bomb_plant_zones") {
            Some(Self::Plant)
        } else if has("home_bases") {
            Some(Self::Base)
        } else if has("tiebreaker") || has("neutral_base") {
            Some(Self::Center)
        } else {
            None
        }
    }

    /// The Reach labels whose placements may hold the role, most fitting first.
    fn labels(self) -> &'static [&'static str] {
        match self {
            Self::Zone => &["terr_object", "koth_hill", "stp_goal", "ctf_flag_return"],
            Self::Plant => &["as_goal", "stp_goal", "as_bomb", "ctf_flag_return"],
            Self::Base => &["ctf_flag_return", "as_bomb", "stp_flag", "ctf_res_zone"],
            Self::Center => &[
                "oddball_ball",
                "koth_hill",
                "terr_object",
                "ctf_flag_return",
            ],
        }
    }

    /// The placements of the first of the role's labels that `objectives` carry.
    fn candidates(self, objectives: &[Objective]) -> Vec<Spawn> {
        self.labels()
            .iter()
            .map(|label| {
                objectives
                    .iter()
                    .filter(|objective| objective.label == *label)
                    .map(|objective| Spawn {
                        position: objective.position,
                        yaw: objective.yaw,
                    })
                    .collect::<Vec<_>>()
            })
            .find(|candidates| !candidates.is_empty())
            .unwrap_or_default()
    }
}

/// One handle of a donor object, as the map's copy keeps it.
#[derive(Clone)]
enum Handle {
    Kept(u32),
    /// A point the copy places at the map spawn `spawn`, from the donor's handle, redirect and
    /// record, whose point descriptor is at `descriptor`.
    Moved {
        handle: TagHash,
        redirect: TagHash,
        record: TagHash,
        payloads: [Vec<u8>; 3],
        descriptor: usize,
        spawn: usize,
    },
    /// A volume the copy leaves out.
    Dropped,
    /// A record of kill volumes whose entries the copy fills with the map's own, from the donor's
    /// handle, redirect and record and the packfile the record names.
    Volumes {
        handle: TagHash,
        redirect: TagHash,
        record: TagHash,
        packfile: TagHash,
        payloads: [Vec<u8>; 4],
    },
    /// An objective, such as a capture zone or a bomb site, whose record places it in the
    /// placement row at `row`. Records of one `group` share a name and are placed together. The
    /// copy places it at `place`, chosen later.
    Objective {
        handle: TagHash,
        redirect: TagHash,
        record: TagHash,
        payloads: [Vec<u8>; 3],
        row: usize,
        role: Role,
        group: Vec<u8>,
        place: Option<Spawn>,
    },
    /// A handle whose object list `list` keeps donor leftovers. The copy names a copy of the list,
    /// `list_payload`, with their rows moved far below the map, or with no rows when none stays,
    /// and each row at an offset in `kept` moved onto the map spawn at the same index of
    /// `spawns`, chosen later.
    Listed {
        handle: TagHash,
        payload: Vec<u8>,
        list: TagHash,
        list_payload: Vec<u8>,
        kept: Vec<usize>,
        spawns: Vec<usize>,
    },
}

/// One donor object and, when any of its handles change or it names the donor's bubble, what its
/// copy lists per bubble.
struct Object {
    tag: TagHash,
    payload: Vec<u8>,
    bubbles: Option<Vec<Vec<Handle>>>,
}

/// A donor object, its payload and its handles per bubble.
type DonorObject = (TagHash, Vec<u8>, Vec<Vec<Handle>>);

/// What a donor registry gives every map area that copies it: its objects with their handles
/// resolved, and its encounter templates and neutral trigger pair once an area needs them. A
/// campaign map's connected areas all copy one donor registry, and reading its objects and walking
/// their residency again took about 20 seconds for each of Nightfall's six later areas.
#[derive(Default)]
pub(super) struct Donor {
    registry: Option<TagHash>,
    /// Each array's objects, their payloads and their handles per bubble.
    objects: [Vec<DonorObject>; 3],
    /// The slots of each object, by its tag, whose placed record every copy leaves out.
    lost: BTreeMap<u32, BTreeSet<u32>>,
    dependencies: Arc<BTreeSet<u32>>,
    templates: Option<Option<Templates>>,
    triggers: Option<Option<super::triggers::Triggers>>,
}

impl Donor {
    /// Resolves the objects of the registry `tag`, unless they already are.
    fn resolve(
        &mut self,
        manager: &PackageManager,
        tag: TagHash,
        owner: &[u8],
    ) -> AuthoringResult<()> {
        if self.registry == Some(tag) {
            return Ok(());
        }
        *self = Self::default();
        for (list, &descriptor) in self.objects.iter_mut().zip(&REGISTRY_OBJECTS) {
            for object in words(owner, descriptor)? {
                if class(manager, object) != Some(OBJECT_CLASS) {
                    return Err(invalid(format!(
                        "Registry {tag} lists {object:08X}, which is not an object"
                    )));
                }
                let payload = read_tag(manager, TagHash(object), "registry object")?;
                let key = read_u32(&payload, OBJECT_KEY)?;
                let mut bubbles = Vec::new();
                if read_u64(&payload, OBJECT_BUBBLES)? != 0 {
                    let (count, _, rows, _) = array_at(&payload, OBJECT_BUBBLES)?;
                    for bubble in 0..count {
                        let tags = words(&payload, rows + bubble * BUBBLE_SIZE + BUBBLE_HANDLES)?;
                        let mut handles = Vec::with_capacity(tags.len());
                        for tag in tags {
                            let kept = handle(manager, tag, key)?;
                            // A handle the copy leaves out takes its record's slot with it.
                            if matches!(kept, Handle::Dropped)
                                && let Some(slot) = dropped_slot(manager, tag, key)?
                            {
                                self.lost.entry(object).or_default().insert(slot);
                            }
                            handles.push(kept);
                        }
                        bubbles.push(handles);
                    }
                }
                // An object that places no point is not a Crucible object, whose lists hold
                // logical nodes the map's spawns take, but the donor activity's own: its lists
                // place the donor's scripted scenery, which the copy leaves out ([`empty_list`]).
                // The Other Side's place the Leviathan's doors and pots at its trophy hall, 91
                // rows of whose entities hold no render, body or effect component, and moved onto
                // the level's spawns they stood in Lone Wolf and Tip of the Spear.
                if !bubbles
                    .iter()
                    .flatten()
                    .any(|handle| matches!(handle, Handle::Moved { .. }))
                {
                    for handle in bubbles.iter_mut().flatten() {
                        empty_list(manager, handle)?;
                    }
                }
                list.push((TagHash(object), payload, bubbles));
            }
        }
        self.dependencies = Arc::new(loading::dependencies(
            manager,
            REGISTRY_OBJECTS
                .iter()
                .map(|&field| words(owner, field))
                .collect::<AuthoringResult<Vec<_>>>()?
                .into_iter()
                .flatten(),
        )?);
        self.registry = Some(tag);
        Ok(())
    }

    /// The templates of the donor's encounter objects, with the additional encounters' squads.
    fn templates(&mut self, manager: &PackageManager) -> AuthoringResult<Option<&Templates>> {
        if self.templates.is_none() {
            let mut templates: Option<Templates> = None;
            for (object, payload, _) in self.objects.iter().flatten() {
                if let Some(found) = Templates::find(manager, object.0, payload)? {
                    match &mut templates {
                        Some(templates) => templates.merge(found),
                        None => templates = Some(found),
                    }
                }
            }
            if let Some(templates) = &mut templates {
                templates.supplement(manager)?;
            }
            self.templates = Some(templates);
        }
        Ok(self.templates.as_ref().and_then(Option::as_ref))
    }

    /// The first neutral trigger pair among the donor's objects.
    fn triggers(
        &mut self,
        manager: &PackageManager,
    ) -> AuthoringResult<Option<&super::triggers::Triggers>> {
        if self.triggers.is_none() {
            let mut triggers = None;
            for (object, payload, _) in self.objects.iter().flatten() {
                triggers = super::triggers::Triggers::find(manager, *object, payload)?;
                if triggers.is_some() {
                    break;
                }
            }
            self.triggers = Some(triggers);
        }
        Ok(self.triggers.as_ref().and_then(Option::as_ref))
    }
}

/// Where a registry's copy sits and how it differs from the donor's, for the rows that describe
/// it to the runtime.
pub(super) struct Copies {
    /// How many entries the registry takes.
    pub entries: usize,
    /// Each donor tag the registry copies with its copy: the registry itself and every object
    /// whose lists change. An object the copy still lists by its donor tag is not among them.
    pub moved: Vec<(TagHash, TagHash)>,
    /// The slots of each donor object, by its tag, whose placed record the copy leaves out. The
    /// runtime lists a slot in an object's roster only while a handle reaches its record.
    pub lost: BTreeMap<u32, BTreeSet<u32>>,
    /// The encounter object the copy adds, as the activity SDK pack describes it.
    pub encounter: Option<super::pack::Encounter>,
}

pub(super) struct Registry {
    owner: Owner,
    dependencies: Arc<BTreeSet<u32>>,
    /// The slots of each donor object, by its tag, whose placed record the copy leaves out.
    lost: BTreeMap<u32, BTreeSet<u32>>,
    /// The objects of each of the registry's arrays.
    objects: [Vec<Object>; 3],
    spawns: Vec<Spawn>,
    volumes: Vec<Volume>,
    bubble: BubbleName,
    /// The copy's own squads, when the level has squads the donor's can stand in for.
    encounter: Option<Encounter>,
    triggers: Option<super::triggers::Triggers>,
}

fn class(manager: &PackageManager, tag: u32) -> Option<u32> {
    manager.get_entry(TagHash(tag)).map(|entry| entry.reference)
}

/// The offsets of `record`'s descriptors in its payload.
fn descriptors(payload: &[u8], record: u32) -> AuthoringResult<Vec<usize>> {
    let mut found = Vec::new();
    for at in (0..payload.len().saturating_sub(DESCRIPTOR_SIZE - 1)).step_by(4) {
        if read_u32(payload, at)? == record && read_u32(payload, at + DESCRIPTOR_MARK)? == MARK {
            found.push(at);
        }
    }
    Ok(found)
}

fn floats<const N: usize>(payload: &[u8], at: usize) -> AuthoringResult<[f32; N]> {
    let mut values = [0f32; N];
    for (k, value) in values.iter_mut().enumerate() {
        *value = f32::from_bits(read_u32(payload, at + k * 4)?);
    }
    Ok(values)
}

/// The slot of the object keyed `key` that the placed record `tag` fills, as the runtime finds
/// it: the record's definition names the record and its default instance, carries the key, and
/// holds a component class and schemas of class shape.
fn slot_of(record: &[u8], tag: u32, key: u32) -> AuthoringResult<Option<u32>> {
    let target = |field: usize| -> AuthoringResult<Option<usize>> {
        let relative = read_u64(record, field)? as i64;
        Ok((relative != 0)
            .then(|| field.checked_add_signed(relative as isize))
            .flatten())
    };
    let (Some(definition), Some(instance)) = (target(RECORD_DEFINITION)?, target(RECORD_INSTANCE)?)
    else {
        return Ok(None);
    };
    if definition
        .checked_add(SLOT_HEADER)
        .is_none_or(|end| end > record.len())
    {
        return Ok(None);
    }
    let class = |value: u32| value >> 16 == 0x8080;
    let schema = |value: u32| value == u32::MAX || class(value);
    let named = read_u32(record, definition)? == tag
        && read_u64(record, definition + DESCRIPTOR_MARK)? == instance as u64
        && read_u32(record, definition + DESCRIPTOR_KEY)? == key
        && class(read_u32(record, definition + DESCRIPTOR_CLASS)?)
        && schema(read_u32(record, definition + DESCRIPTOR_SENSE)?)
        && schema(read_u32(record, definition + DESCRIPTOR_AUTH)?);
    // The slot type is the low half of the word, the slot index the high half.
    Ok(if named {
        Some(read_u32(record, definition + DESCRIPTOR_SLOT)? >> 16)
    } else {
        None
    })
}

/// The slot the placed record behind donor handle `handle` fills in the object keyed `key`, for a
/// handle [`handle`] leaves out, read along the same handle, redirect and record.
fn dropped_slot(manager: &PackageManager, handle: u32, key: u32) -> AuthoringResult<Option<u32>> {
    let handle_payload = read_tag(manager, TagHash(handle), "object handle")?;
    let redirects = words(&handle_payload, HANDLE_REDIRECTS)?;
    let [redirect] = redirects[..] else {
        return Ok(None);
    };
    let redirect_payload = read_tag(manager, TagHash(redirect), "object redirect")?;
    let record = read_u32(&redirect_payload, REDIRECT_RECORD[0])?;
    let record_payload = read_tag(manager, TagHash(record), "placed record")?;
    slot_of(&record_payload, record, key)
}

/// What becomes of donor handle `handle` of an object keyed `key`: a point moves, a volume drops,
/// an object list loses its leftovers and anything else stays. The spawn of a moved point is
/// chosen later.
fn handle(manager: &PackageManager, handle: u32, key: u32) -> AuthoringResult<Handle> {
    if class(manager, handle) != Some(HANDLE_CLASS) {
        return Ok(Handle::Kept(handle));
    }
    let handle_payload = read_tag(manager, TagHash(handle), "object handle")?;
    let list = read_u32(&handle_payload, HANDLE_LIST)?;
    if list != u32::MAX && class(manager, list) == Some(LIST_CLASS) {
        let mut list_payload = read_tag(manager, TagHash(list), "object list")?;
        if let Some(kept) = chain::bury_leftovers(manager, &mut list_payload)? {
            return Ok(Handle::Listed {
                handle: TagHash(handle),
                payload: handle_payload,
                list: TagHash(list),
                list_payload,
                kept,
                spawns: Vec::new(),
            });
        }
    }
    let redirects = words(&handle_payload, HANDLE_REDIRECTS)?;
    let [redirect] = redirects[..] else {
        return Ok(Handle::Kept(handle));
    };
    if read_u32(&handle_payload, HANDLE_LIST)? != u32::MAX
        || class(manager, redirect) != Some(REDIRECT_CLASS)
    {
        return Ok(Handle::Kept(handle));
    }
    let redirect_payload = read_tag(manager, TagHash(redirect), "object redirect")?;
    let record = read_u32(&redirect_payload, REDIRECT_RECORD[0])?;
    if read_u32(&redirect_payload, REDIRECT_RECORD[1])? != record
        || class(manager, record) != Some(PLACED_CLASS)
    {
        return Ok(Handle::Kept(handle));
    }
    let record_payload = read_tag(manager, TagHash(record), "placed record")?;
    let found = descriptors(&record_payload, record)?;
    let Some(&first) = found.first() else {
        return Ok(Handle::Kept(handle));
    };
    let component = read_u32(&record_payload, first + DESCRIPTOR_CLASS)?;
    if component == MUSIC_SENSOR || component == AI_SPAWNER {
        return Ok(Handle::Dropped);
    }
    if component == VOLUME {
        // The copy fills a record of kill volumes with the map's own, and leaves any other out.
        let Some(packfile) = super::volumes::packfile_of(&record_payload)? else {
            return Ok(Handle::Dropped);
        };
        let packfile_payload = read_tag(manager, packfile, "volume packfile")?;
        return Ok(Handle::Volumes {
            handle: TagHash(handle),
            redirect: TagHash(redirect),
            record: TagHash(record),
            packfile,
            payloads: [
                handle_payload,
                redirect_payload,
                record_payload,
                packfile_payload,
            ],
        });
    }
    if component == OBJECTIVE {
        return objective(
            [TagHash(handle), TagHash(redirect), TagHash(record)],
            [handle_payload, redirect_payload, record_payload],
        );
    }
    if component != RESPAWN_POINT && component != NAMED_POINT {
        return Ok(Handle::Kept(handle));
    }
    let mut keyed = Vec::new();
    for &at in &found {
        if read_u32(&record_payload, at + DESCRIPTOR_KEY)? == key {
            keyed.push(at);
        }
    }
    let [descriptor] = keyed[..] else {
        return Err(invalid(format!(
            "Placed point {record:08X} has {} descriptors for its object",
            keyed.len()
        )));
    };
    // The move writes a unit rotation, the position and a unit scale. A donor's own point need only
    // read as a rotation and a scale there: The Dead Cliffs places points that are not unit.
    let rotation = floats::<4>(&record_payload, descriptor + POINT_ROTATION)?;
    let length = rotation
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    let scale = f32::from_bits(read_u32(&record_payload, descriptor + POINT_SCALE)?);
    if !(length.is_finite() && length > 1e-3 && scale.is_finite() && scale > 0.) {
        return Err(invalid(format!(
            "Placed point {record:08X} has no rotation and scale"
        )));
    }
    Ok(Handle::Moved {
        handle: TagHash(handle),
        redirect: TagHash(redirect),
        record: TagHash(record),
        payloads: [handle_payload, redirect_payload, record_payload],
        descriptor,
        spawn: usize::MAX,
    })
}

/// Leaves `handle`'s object list with no rows, making a handle that names a list with rows into a
/// copy of it. A handle naming no list stays as it is. A buried row is still an object the client
/// creates from its small lease of entity IDs, and each of a campaign map's areas lists them again.
fn empty_list(manager: &PackageManager, handle: &mut Handle) -> AuthoringResult<()> {
    match handle {
        Handle::Listed {
            list_payload, kept, ..
        } => {
            append_native_array(list_payload, TABLE_ROWS, LIST_CLASS, 0, &[])?;
            kept.clear();
        }
        Handle::Kept(tag) => {
            let tag = *tag;
            if class(manager, tag) != Some(HANDLE_CLASS) {
                return Ok(());
            }
            let payload = read_tag(manager, TagHash(tag), "object handle")?;
            let list = read_u32(&payload, HANDLE_LIST)?;
            // A copy of a type-16 owner needs a shared-tag companion of its own, which a list
            // copy does not build, so such a handle or list stays as it is.
            let owner = |tag: u32| {
                manager.get_entry(TagHash(tag)).is_some_and(|entry| {
                    entry.file_type == crate::package::extend::SHARED_TAG_OWNER_FILE_TYPE
                })
            };
            if list == u32::MAX
                || class(manager, list) != Some(LIST_CLASS)
                || owner(tag)
                || owner(list)
            {
                return Ok(());
            }
            let mut list_payload = read_tag(manager, TagHash(list), "object list")?;
            if read_u64(&list_payload, TABLE_ROWS)? == 0 {
                return Ok(());
            }
            append_native_array(&mut list_payload, TABLE_ROWS, LIST_CLASS, 0, &[])?;
            *handle = Handle::Listed {
                handle: TagHash(tag),
                payload,
                list: TagHash(list),
                list_payload,
                kept: Vec::new(),
                spawns: Vec::new(),
            };
        }
        _ => {}
    }
    Ok(())
}

/// What becomes of an objective record: a record that places its objective somewhere moves,
/// when its name gives it a role, onto the level's objectives. One that places something somewhere
/// without a role is left out, whatever its entity holds: the placement belongs to the donor's own
/// rooms, and moved below the map it was still an object the client created. The Other Side's records place the Leviathan's door banks (`815B53C4`, whose
/// components hold no render model, body or effect) and its hallway door at its trophy hall, which
/// stood in Lone Wolf's valley and on Tip of the Spear's beach. A record at the origin, which its
/// activity places elsewhere, stays. `tags` and `payloads` are the donor's handle, redirect and
/// record.
fn objective(tags: [TagHash; 3], payloads: [Vec<u8>; 3]) -> AuthoringResult<Handle> {
    let record = &payloads[2];
    let mut row = None;
    for at in (0..record.len().saturating_sub(0x90 + 20)).step_by(4) {
        if read_u32(record, at)? == ARRAY_MARKER
            && read_u64(record, at + 4)? == 1
            && read_u32(record, at + 12)? == PLACEMENT_ROW_CLASS
        {
            row = Some(at + 20);
            break;
        }
    }
    let name = record
        .windows(OBJECTIVE_NAME.len())
        .position(|window| window == OBJECTIVE_NAME)
        .map(|start| {
            let end = record[start..]
                .iter()
                .position(|&byte| byte == 0)
                .map_or(record.len(), |length| start + length);
            record[start..end].to_vec()
        });
    let Some(row) = row else {
        return Ok(Handle::Kept(tags[0].0));
    };
    let placed = chain::row_translation(record, row)?;
    let role = name
        .as_deref()
        .and_then(Role::of)
        .filter(|_| placed.iter().any(|&value| value != 0.));
    let (role, name) = match (role, name) {
        (Some(role), Some(name)) => (role, name),
        _ if placed.iter().any(|&value| value != 0.) => return Ok(Handle::Dropped),
        _ => return Ok(Handle::Kept(tags[0].0)),
    };
    Ok(Handle::Objective {
        handle: tags[0],
        redirect: tags[1],
        record: tags[2],
        payloads,
        row,
        role,
        group: name,
        place: None,
    })
}

/// The horizontal bounds of `positions`.
fn bounds(positions: impl Iterator<Item = [f32; 3]>) -> ([f32; 2], [f32; 2]) {
    positions.fold(([f32::MAX; 2], [f32::MIN; 2]), |(low, high), p| {
        (
            [low[0].min(p[0]), low[1].min(p[1])],
            [high[0].max(p[0]), high[1].max(p[1])],
        )
    })
}

/// Places `points` (their donor positions) on `spawns`. Both sets are scaled into the unit square,
/// the points over `frame`, the donor's bounds, and the spawns over `spawn_frame`, the level's,
/// with the map's long axis turned onto the donor's. Each point in turn takes the nearest spawn
/// not yet `taken`, or the nearest of all once none is left, so a level with fewer spawn points
/// than the donor places shares them. The map's sides and spread follow the donor's: a team's
/// landing points stay together at its end.
fn assign(
    points: &[[f32; 3]],
    frame: ([f32; 2], [f32; 2]),
    spawns: &[Spawn],
    spawn_frame: ([f32; 2], [f32; 2]),
    taken: &mut [bool],
) -> AuthoringResult<Vec<usize>> {
    let (point_low, point_high) = frame;
    let (spawn_low, spawn_high) = spawn_frame;
    let long = |low: [f32; 2], high: [f32; 2]| usize::from(high[1] - low[1] > high[0] - low[0]);
    let turn = long(point_low, point_high) != long(spawn_low, spawn_high);
    let unit = |p: [f32; 2], low: [f32; 2], high: [f32; 2]| {
        [0, 1].map(|k| (p[k] - low[k]) / (high[k] - low[k]).max(1e-3))
    };
    let placed = spawns
        .iter()
        .map(|s| {
            let [x, y] = unit([s.position[0], s.position[1]], spawn_low, spawn_high);
            if turn { [y, x] } else { [x, y] }
        })
        .collect::<Vec<_>>();
    let mut chosen = Vec::with_capacity(points.len());
    for point in points {
        let target = unit([point[0], point[1]], point_low, point_high);
        let distance =
            |k: usize| (placed[k][0] - target[0]).powi(2) + (placed[k][1] - target[1]).powi(2);
        let nearest = |free: bool| {
            (0..spawns.len())
                .filter(|&k| !free || !taken[k])
                .min_by(|&a, &b| distance(a).total_cmp(&distance(b)))
        };
        let best = nearest(true)
            .or_else(|| nearest(false))
            .ok_or_else(|| invalid("The map has no spawn points"))?;
        taken[best] = true;
        chosen.push(best);
    }
    Ok(chosen)
}

impl Registry {
    pub(super) fn encounter(&self) -> Option<super::mission::Encounter> {
        self.encounter.as_ref().map(Encounter::bindings)
    }

    pub(super) fn triggers(&self) -> Option<super::mission::Triggers> {
        self.triggers
            .as_ref()
            .map(super::triggers::Triggers::bindings)
    }

    /// The registry the donor slice set `slice_set` names at `field`, with its points placed on
    /// `spawns`, its objectives on the level's labelled `objectives`, its kill volumes refilled
    /// with `volumes`, or left out when there are none, the map's name for its `bubble`, and an
    /// encounter of the level's `squads` named after the area `area`. `donor` keeps the donor's
    /// share for the map's other areas.
    #[allow(clippy::too_many_arguments, clippy::cognitive_complexity)]
    pub(super) fn load(
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
        donor: &mut Donor,
        slice_set: &[u8],
        field: usize,
        spawns: &[Spawn],
        squads: &[Squad],
        objectives: &[Objective],
        volumes: &[Volume],
        trigger_volumes: &[Volume],
        bubble: BubbleName,
        area: &str,
    ) -> AuthoringResult<Self> {
        let tag = TagHash(read_u32(slice_set, field)?);
        let owner = Owner::load_tag(
            directory,
            manager,
            companions,
            tag,
            REGISTRY_CLASS,
            "object registry",
        )?;
        donor.resolve(manager, tag, &owner.payload)?;
        let mut objects: [Vec<Object>; 3] = Default::default();
        for (list, resolved) in objects.iter_mut().zip(&donor.objects) {
            for (object, payload, bubbles) in resolved {
                let changed = bubble.named(payload)
                    || bubbles
                        .iter()
                        .flatten()
                        .any(|handle| !matches!(handle, Handle::Kept(_)));
                list.push(Object {
                    tag: *object,
                    payload: payload.clone(),
                    bubbles: changed.then(|| bubbles.clone()),
                });
            }
        }
        let mut lost = donor.lost.clone();
        if volumes.is_empty() {
            for object in objects.iter_mut().flatten() {
                let key = read_u32(&object.payload, OBJECT_KEY)?;
                for handle in object.bubbles.iter_mut().flatten().flatten() {
                    if let Handle::Volumes {
                        record, payloads, ..
                    } = handle
                    {
                        // The record's slot leaves the object with it.
                        if let Some(slot) = slot_of(&payloads[2], record.0, key)? {
                            lost.entry(object.tag.0).or_default().insert(slot);
                        }
                        *handle = Handle::Dropped;
                    }
                }
            }
        }
        let mut points = Vec::new();
        let mut nodes = Vec::new();
        for object in objects.iter().flatten() {
            for handle in object.bubbles.iter().flatten().flatten() {
                match handle {
                    Handle::Moved {
                        payloads,
                        descriptor,
                        ..
                    } => points.push(floats::<3>(&payloads[2], descriptor + POINT_POSITION)?),
                    Handle::Listed {
                        list_payload, kept, ..
                    } => {
                        for &row in kept {
                            nodes.push(chain::row_translation(list_payload, row)?);
                        }
                    }
                    Handle::Kept(_)
                    | Handle::Dropped
                    | Handle::Volumes { .. }
                    | Handle::Objective { .. } => {}
                }
            }
        }
        // Points take distinct spawns while any remain. The list rows that stay follow in the
        // same frame and take spawns no point took while any remain. Objective records of one
        // name take the level's placements their role prefers, in the same frame, or spawns
        // when the level has none.
        let frame = bounds(points.iter().chain(&nodes).copied());
        let spawn_frame = bounds(spawns.iter().map(|spawn| spawn.position));
        let mut taken = vec![false; spawns.len()];
        let mut chosen = assign(&points, frame, spawns, spawn_frame, &mut taken)?.into_iter();
        let mut chosen_nodes = assign(&nodes, frame, spawns, spawn_frame, &mut taken)?.into_iter();
        let mut groups: BTreeMap<Vec<u8>, (Role, Vec<[f32; 3]>)> = BTreeMap::new();
        for object in objects.iter().flatten() {
            for handle in object.bubbles.iter().flatten().flatten() {
                if let Handle::Objective {
                    payloads,
                    row,
                    role,
                    group,
                    place: None,
                    ..
                } = handle
                {
                    groups
                        .entry(group.clone())
                        .or_insert((*role, Vec::new()))
                        .1
                        .push(chain::row_translation(&payloads[2], *row)?);
                }
            }
        }
        let mut placed = BTreeMap::new();
        for (group, (role, positions)) in &groups {
            let candidates = role.candidates(objectives);
            let targets = if candidates.is_empty() {
                spawns
            } else {
                candidates.as_slice()
            };
            let mut taken = vec![false; targets.len()];
            let chosen = assign(positions, frame, targets, spawn_frame, &mut taken)?
                .into_iter()
                .map(|index| targets[index])
                .collect::<Vec<_>>();
            placed.insert(group.clone(), chosen.into_iter());
        }
        for object in objects.iter_mut().flatten() {
            for handle in object.bubbles.iter_mut().flatten().flatten() {
                match handle {
                    Handle::Moved { spawn, .. } => {
                        *spawn = chosen
                            .next()
                            .ok_or_else(|| invalid("Spawn assignment ended early"))?;
                    }
                    Handle::Listed { kept, spawns, .. } => {
                        *spawns = kept
                            .iter()
                            .map(|_| {
                                chosen_nodes
                                    .next()
                                    .ok_or_else(|| invalid("Spawn assignment ended early"))
                            })
                            .collect::<AuthoringResult<_>>()?;
                    }
                    Handle::Objective {
                        group,
                        place: place @ None,
                        ..
                    } => {
                        *place = Some(
                            placed
                                .get_mut(group)
                                .and_then(Iterator::next)
                                .ok_or_else(|| invalid("Objective assignment ended early"))?,
                        );
                    }
                    Handle::Kept(_)
                    | Handle::Dropped
                    | Handle::Volumes { .. }
                    | Handle::Objective { .. } => {}
                }
            }
        }
        let encounter = if squads.is_empty() {
            None
        } else {
            match donor.templates(manager)? {
                Some(templates) => Encounter::new(templates, squads, area)?,
                None => None,
            }
        };
        let triggers = if trigger_volumes.is_empty() {
            None
        } else {
            Some(
                donor
                    .triggers(manager)?
                    .ok_or_else(|| {
                        invalid("Mission donor has no paired neutral volume and player sensor")
                    })?
                    .placed(trigger_volumes, area),
            )
        };
        Ok(Self {
            owner,
            dependencies: donor.dependencies.clone(),
            lost,
            objects,
            spawns: spawns.to_vec(),
            volumes: volumes.to_vec(),
            bubble,
            encounter,
            triggers,
        })
    }

    /// The registry's entries starting at `first`, the first being the registry itself.
    pub(super) fn tags(
        &self,
        manager: &PackageManager,
        first: TagHash,
    ) -> AuthoringResult<Vec<NewTagSpec>> {
        let (mut tags, added, _, _) = self.entries(first)?;
        loading::prune(manager, first, &mut tags, &self.dependencies, &added)?;
        Ok(tags)
    }

    /// The registry's copy as it sits from `first`.
    pub(super) fn copies(&self, first: TagHash) -> AuthoringResult<Copies> {
        let (tags, _, moved, encounter) = self.entries(first)?;
        Ok(Copies {
            entries: tags.len(),
            moved,
            lost: self.lost.clone(),
            encounter,
        })
    }

    /// The entries before their loading lists are pruned, the entries the copy adds, each donor
    /// tag with its copy, and the encounter object the copy adds as the registry lists it.
    #[allow(clippy::type_complexity)]
    fn entries(
        &self,
        first: TagHash,
    ) -> AuthoringResult<(
        Vec<NewTagSpec>,
        Vec<TagHash>,
        Vec<(TagHash, TagHash)>,
        Option<super::pack::Encounter>,
    )> {
        let at = |offset: usize| {
            u16::try_from(first.entry_index() as usize + offset)
                .map(|index| TagHash::new(first.pkg_id(), index))
                .map_err(|_| invalid("The map package has too many entries"))
        };
        let spec = |template: TagHash, payload: Vec<u8>| NewTagSpec {
            template_tag: template,
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        };
        let (registry, companion) = (at(0)?, at(1)?);
        let mut registry_payload = self.owner.payload.clone();
        let mut next = 2;
        let mut added = Vec::new();
        let mut moved = vec![(self.owner.tag, registry)];
        let mut content = Vec::new();
        for (list, &descriptor) in self.objects.iter().zip(&REGISTRY_OBJECTS) {
            for (index, object) in list.iter().enumerate() {
                let Some(bubbles) = &object.bubbles else {
                    continue;
                };
                let copy = at(next)?;
                next += 1;
                moved.push((object.tag, copy));
                let (_, _, rows, _) = array_at(&registry_payload, descriptor)?;
                write_u32(&mut registry_payload, rows + index * 4, copy.0)?;
                let mut payload = object.payload.clone();
                self.bubble.rename(&mut payload);
                let mut lists = Vec::with_capacity(bubbles.len());
                let mut points = Vec::new();
                for handles in bubbles {
                    let mut list = Vec::with_capacity(handles.len());
                    for handle in handles {
                        match handle {
                            Handle::Kept(tag) => list.push(*tag),
                            Handle::Dropped => {}
                            Handle::Moved {
                                handle,
                                redirect,
                                record,
                                payloads,
                                descriptor,
                                spawn,
                            } => {
                                let (own_handle, own_redirect, own_record) =
                                    (at(next)?, at(next + 1)?, at(next + 2)?);
                                next += 3;
                                list.push(own_handle.0);
                                points.extend(self.point(
                                    [*handle, *redirect, *record],
                                    own_redirect,
                                    own_record,
                                    payloads,
                                    *descriptor,
                                    &self.spawns[*spawn],
                                )?);
                                added.extend([own_handle, own_redirect, own_record]);
                            }
                            Handle::Listed {
                                handle,
                                payload: handle_payload,
                                list: donor_list,
                                list_payload,
                                kept,
                                spawns,
                            } => {
                                let (own_handle, own_list) = (at(next)?, at(next + 1)?);
                                next += 2;
                                list.push(own_handle.0);
                                let mut handle_payload = handle_payload.clone();
                                write_u32(&mut handle_payload, HANDLE_LIST, own_list.0)?;
                                let mut list_payload = list_payload.clone();
                                for (&row, &spawn) in kept.iter().zip(spawns) {
                                    let spawn = &self.spawns[spawn];
                                    chain::place_row(
                                        &mut list_payload,
                                        row,
                                        spawn.yaw,
                                        spawn.position,
                                    )?;
                                }
                                points.push(spec(*handle, handle_payload));
                                points.push(spec(*donor_list, list_payload));
                                added.extend([own_handle, own_list]);
                            }
                            Handle::Volumes {
                                handle,
                                redirect,
                                record,
                                packfile,
                                payloads,
                            } => {
                                let own = [at(next)?, at(next + 1)?, at(next + 2)?, at(next + 3)?];
                                next += 4;
                                list.push(own[0].0);
                                points.extend(self.volume_tags(
                                    [*handle, *redirect, *record, *packfile],
                                    [own[1], own[2], own[3]],
                                    payloads,
                                )?);
                                added.extend(own);
                            }
                            Handle::Objective {
                                handle,
                                redirect,
                                record,
                                payloads,
                                row,
                                place,
                                ..
                            } => {
                                let place = place.ok_or_else(|| {
                                    invalid("An objective was never placed on the map")
                                })?;
                                let own = [at(next)?, at(next + 1)?, at(next + 2)?];
                                next += 3;
                                list.push(own[0].0);
                                points.extend(Self::objective_tags(
                                    [*handle, *redirect, *record],
                                    [own[1], own[2]],
                                    payloads,
                                    *row,
                                    place,
                                )?);
                                added.extend(own);
                            }
                        }
                    }
                    lists.push(list);
                }
                // An object that only names the bubble may list none.
                let bubble_rows = if lists.is_empty() {
                    0
                } else {
                    array_at(&payload, OBJECT_BUBBLES)?.2
                };
                for (bubble, list) in lists.iter().enumerate() {
                    let field = bubble_rows + bubble * BUBBLE_SIZE + BUBBLE_HANDLES;
                    let row_class = if read_u64(&payload, field)? == 0 {
                        0x8080_9466
                    } else {
                        array_at(&payload, field)?.3
                    };
                    let rows = list
                        .iter()
                        .flat_map(|tag| tag.to_le_bytes())
                        .collect::<Vec<_>>();
                    append_native_array(&mut payload, field, row_class, list.len(), &rows)?;
                }
                added.push(copy);
                // Each changed object takes the entry after the previous object's points, and
                // its own points follow it.
                content.push(spec(object.tag, synchronize_payload_size(payload)?));
                content.extend(points);
            }
        }
        let rename = |payload: &mut Vec<u8>| {
            self.bubble.rename(payload);
        };
        let mut authored = Vec::new();
        let mut encounter_rows = None;
        if let Some(encounter) = &self.encounter {
            authored.push(encounter.tags(&at, &mut next, &rename)?);
            // The encounter is the first object appended to the registry's second array.
            let descriptor = REGISTRY_OBJECTS[1];
            let ordinal = words(&registry_payload, descriptor)?.len();
            encounter_rows = Some(encounter.rows(authored[0].1, descriptor, ordinal)?);
        }
        if let Some(triggers) = &self.triggers {
            authored.push(triggers.tags(&at, &mut next, &rename)?);
        }
        for (specs, object, dependencies) in authored {
            let descriptor = REGISTRY_OBJECTS[1];
            let (_, _, _, row_class) = array_at(&registry_payload, descriptor)?;
            let mut listed = words(&registry_payload, descriptor)?;
            listed.push(object.0);
            let rows = listed
                .iter()
                .flat_map(|tag| tag.to_le_bytes())
                .collect::<Vec<_>>();
            append_native_array(
                &mut registry_payload,
                descriptor,
                row_class,
                listed.len(),
                &rows,
            )?;
            registry_payload = synchronize_payload_size(registry_payload)?;
            added.extend(dependencies);
            content.extend(specs);
        }
        let mut tags = vec![
            spec(self.owner.tag, registry_payload),
            spec(
                self.owner.companion_template(),
                self.owner.companion_with(companion, registry, &added)?,
            ),
        ];
        tags.extend(content);
        Ok((tags, added, moved, encounter_rows))
    }

    /// A moved point's handle, redirect and record from the donor's three `payloads`. The handle
    /// names `redirect`, which names `record`, and the record's point is at `spawn`.
    fn point(
        &self,
        donor: [TagHash; 3],
        redirect: TagHash,
        record: TagHash,
        payloads: &[Vec<u8>; 3],
        descriptor: usize,
        spawn: &Spawn,
    ) -> AuthoringResult<Vec<NewTagSpec>> {
        let mut handle_payload = payloads[0].clone();
        let (_, _, rows, _) = array_at(&handle_payload, HANDLE_REDIRECTS)?;
        write_u32(&mut handle_payload, rows, redirect.0)?;
        let mut redirect_payload = payloads[1].clone();
        for field in REDIRECT_RECORD {
            write_u32(&mut redirect_payload, field, record.0)?;
        }
        let mut record_payload = payloads[2].clone();
        for at in (0..record_payload.len().saturating_sub(3)).step_by(4) {
            if read_u32(&record_payload, at)? == donor[2].0 {
                write_u32(&mut record_payload, at, record.0)?;
            }
        }
        let half = spawn.yaw / 2.;
        let [x, y, z] = spawn.position;
        for (k, value) in [0., 0., half.sin(), half.cos(), x, y, z]
            .into_iter()
            .enumerate()
        {
            write_u32(
                &mut record_payload,
                descriptor + POINT_ROTATION + k * 4,
                f32::to_bits(value),
            )?;
        }
        write_u32(
            &mut record_payload,
            descriptor + POINT_SCALE,
            1f32.to_bits(),
        )?;
        let spec = |template: TagHash, payload: Vec<u8>| NewTagSpec {
            template_tag: template,
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        };
        Ok(vec![
            spec(donor[0], handle_payload),
            spec(donor[1], redirect_payload),
            spec(donor[2], record_payload),
        ])
    }

    /// A moved objective's handle, redirect and record from the donor's three `payloads`. The
    /// handle names `redirect`, which names `record`, and the record's placement row at `row`
    /// stands at `place`.
    fn objective_tags(
        donor: [TagHash; 3],
        own: [TagHash; 2],
        payloads: &[Vec<u8>; 3],
        row: usize,
        place: Spawn,
    ) -> AuthoringResult<Vec<NewTagSpec>> {
        let [redirect, record] = own;
        let mut handle_payload = payloads[0].clone();
        let (_, _, rows, _) = array_at(&handle_payload, HANDLE_REDIRECTS)?;
        write_u32(&mut handle_payload, rows, redirect.0)?;
        let mut redirect_payload = payloads[1].clone();
        for field in REDIRECT_RECORD {
            write_u32(&mut redirect_payload, field, record.0)?;
        }
        let mut record_payload = payloads[2].clone();
        for at in (0..record_payload.len().saturating_sub(3)).step_by(4) {
            if read_u32(&record_payload, at)? == donor[2].0 {
                write_u32(&mut record_payload, at, record.0)?;
            }
        }
        chain::place_row(&mut record_payload, row, place.yaw, place.position)?;
        let spec = |template: TagHash, payload: Vec<u8>| NewTagSpec {
            template_tag: template,
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        };
        Ok(vec![
            spec(donor[0], handle_payload),
            spec(donor[1], redirect_payload),
            spec(donor[2], record_payload),
        ])
    }

    /// A refilled volume record's handle, redirect, record and packfile from the donor's four
    /// `payloads`. The handle names `redirect`, which names `record`, and the record holds the
    /// map's volumes and names `packfile`.
    fn volume_tags(
        &self,
        donor: [TagHash; 4],
        own: [TagHash; 3],
        payloads: &[Vec<u8>; 4],
    ) -> AuthoringResult<Vec<NewTagSpec>> {
        let [redirect, record, packfile] = own;
        let mut handle_payload = payloads[0].clone();
        let (_, _, rows, _) = array_at(&handle_payload, HANDLE_REDIRECTS)?;
        write_u32(&mut handle_payload, rows, redirect.0)?;
        let mut redirect_payload = payloads[1].clone();
        for field in REDIRECT_RECORD {
            write_u32(&mut redirect_payload, field, record.0)?;
        }
        let (mut record_payload, packfile_payload) =
            super::volumes::build(&payloads[2], &payloads[3], &self.volumes)?;
        for at in (0..record_payload.len().saturating_sub(3)).step_by(4) {
            let word = read_u32(&record_payload, at)?;
            if word == donor[2].0 {
                write_u32(&mut record_payload, at, record.0)?;
            } else if word == donor[3].0 {
                write_u32(&mut record_payload, at, packfile.0)?;
            }
        }
        let spec = |template: TagHash, payload: Vec<u8>| NewTagSpec {
            template_tag: template,
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        };
        Ok(vec![
            spec(donor[0], handle_payload),
            spec(donor[1], redirect_payload),
            spec(donor[2], record_payload),
            spec(donor[3], packfile_payload),
        ])
    }
}
