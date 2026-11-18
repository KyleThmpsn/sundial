//! The activity SDK pack a mod ships beside its manifest. Sunrise builds its own activity SDK from
//! installed packages alone, so a mod's maps resolve only through rows the mod supplies. At boot
//! the runtime maps `activity_sdk.pack` in each loaded mod's folder and finds a mod activity by
//! its internal name, which exactly one row of every loaded mod pack may carry.
//!
//! Each map takes its donor scenario's rows from the installed pack, as Sunrise Studio's
//! destination copies do: the scenario, its bubbles and states, the occurrences of those states
//! and each object they name with its slots and scene resources. Tags move to the copies the
//! build made, and a tag the map did not copy stays installed. Object keys stay.
//!
//! A map that keeps its donor's bubbles takes them in their positions. A converted level rebuilds
//! its donor's one bubble as its own connected areas, each with a copy of the donor's registry,
//! so each area takes that bubble's rows at its own place, under its own name and with its own
//! registry's copies.
//!
//! Sunrise Studio copies an object without changing what its handles reach, so the installed
//! rows describe its copy as they stand. A map's copy leaves out some of the placed records its
//! donor's handles reach, such as the donor's music sensor, and the runtime lists a slot in an
//! object's roster only while a handle reaches the record that fills it. So each slot whose
//! record a copy leaves out is declared without a component, as the runtime reads the copy, and
//! its scene resources stay out. A row that still named the component would give the object a
//! roster the runtime's own reading of the copy lacks, and it refuses such a mission.
//!
//! The rows are copied, never derived, so an object the map adds has none. The runtime leaves a
//! pack out when it moved the mod's package ids, since the rows name the ids the build chose.
#![cfg_attr(not(feature = "d2-model-importer"), allow(dead_code))]

use super::MapRecipe;
use crate::{
    AuthoringResult,
    artifact::{ArtifactMetadata, digest_bytes},
    error::invalid,
    tag_payload::{read_u32, read_u64, write_u32, write_u64},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use sundial::package_authoring::mod_content::{
    self,
    activity_pack::{BYTE_LIMIT, FILE_NAME},
};

const MAGIC: &[u8; 8] = b"SRSDKP01";
/// The pack schema versions this build copies rows from, each with its section count. The
/// runtime reads only the version it was built with, so a mod pack takes the version, the
/// sections and the row sizes of the pack that runtime generated. Version 53 adds the actor
/// temperament and actor channel sections after version 48's 52, and lets a slot count its object
/// channels in [`SLOT_COUNT`]. The rows this build copies keep their fields in both.
const LAYOUTS: [(u32, usize); 2] = [(48, 52), (53, 54)];
/// The header holds its fixed fields and one 16-byte row for each of `sections`.
const fn header(sections: usize) -> usize {
    SECTION_TABLE + sections * SECTION_ROW
}
const VERSION: usize = 8;
const HEADER_LENGTH: usize = 12;
const FILE_LENGTH: usize = 16;
/// The SHA-256 of every byte after the header. A mod pack leaves the three installed-build
/// digests that follow it zero.
const PAYLOAD_DIGEST: usize = 24;
const SECTION_COUNT: usize = 152;
const SECTION_TABLE: usize = 160;
const SECTION_ROW: usize = 16;

/// The out-of-band row index. Sunrise numbers a mod's activity rows when the tables merge, so a
/// mod pack names no activity index.
const ABSENT: u32 = u32::MAX;
/// The root, scenario and content joins and the extraction, which every mod activity must carry.
const EXACT: u32 = 0xF;
/// A bubble's slice sets start at eight times its row in the scenario.
const SLICE_SETS_PER_BUBBLE: u32 = 8;

/// A section's place in the header and the size of its rows.
#[derive(Clone, Copy)]
struct Section {
    index: usize,
    size: usize,
}

const STRINGS: usize = 0;
const ACTIVITIES: Section = Section {
    index: 1,
    size: 124,
};
const SCENARIOS: Section = Section { index: 2, size: 48 };
const BUBBLES: Section = Section { index: 3, size: 40 };
const STATES: Section = Section { index: 4, size: 64 };
const OBJECTS: Section = Section { index: 5, size: 52 };
const OCCURRENCES: Section = Section { index: 6, size: 56 };
const SLOTS: Section = Section { index: 7, size: 80 };
const SQUADS: Section = Section {
    index: 16,
    size: 52,
};
const SQUAD_MEMBERS: Section = Section {
    index: 17,
    size: 48,
};
const SCENES: Section = Section {
    index: 19,
    size: 40,
};

// Activity rows.
const ACTIVITY_INDEX: usize = 0;
const ACTIVITY_ID: usize = 8;
const ACTIVITY_INTERNAL_NAME: usize = 16;
const ACTIVITY_DISPLAY_NAME: usize = 24;
const ACTIVITY_SCENARIO: usize = 32;
const ACTIVITY_FLAGS: usize = 36;
const ACTIVITY_ROOT_TAG: usize = 56;
const ACTIVITY_SCENARIO_TAG: usize = 60;
const ACTIVITY_MATCHMAKING_TAG: usize = 64;

// Scenario rows.
const SCENARIO_TAG: usize = 0;
const SCENARIO_ID: usize = 8;
const SCENARIO_NAME: usize = 16;
const SCENARIO_BUBBLES: usize = 24;
const SCENARIO_STATES: usize = 32;
const SCENARIO_OCCURRENCES: usize = 40;

// Bubble rows.
const BUBBLE_ID: usize = 0;
const BUBBLE_NAME: usize = 8;
const BUBBLE_SCENARIO: usize = 16;
const BUBBLE_ORDINAL: usize = 20;
const BUBBLE_NAME_HASH: usize = 24;
const BUBBLE_STATES: usize = 32;

// State rows.
const STATE_ID: usize = 0;
const STATE_ENTRY_ID: usize = 8;
const STATE_REGISTRY_ID: usize = 16;
const STATE_SCENARIO: usize = 24;
const STATE_BUBBLE: usize = 28;
const STATE_ORDINAL: usize = 32;
const STATE_ENTRY: usize = 36;
const STATE_SLICE_SET: usize = 40;
const STATE_MAP_BUBBLE: usize = 44;
const STATE_HASH: usize = 48;
const STATE_REGISTRY_TAG: usize = 60;

// Object rows.
const OBJECT_ID: usize = 0;
const OBJECT_TAG: usize = 8;
const OBJECT_SLOTS: usize = 16;

// Occurrence rows, whose first four fields are strings.
const OCCURRENCE_TEXTS: [usize; 4] = [0, 8, 16, 24];
const OCCURRENCE_SCENARIO: usize = 32;
const OCCURRENCE_BUBBLE: usize = 36;
const OCCURRENCE_STATE: usize = 40;
const OCCURRENCE_OBJECT: usize = 44;

// Slot rows, whose first four fields are strings. Their alias and capability ranges name rows a
// mod pack does not carry.
const SLOT_TEXTS: [usize; 4] = [0, 8, 16, 24];
const SLOT_OBJECT: usize = 32;
const SLOT_INDEX: usize = 36;
/// A slot's component class and its sense and auth schemas, absent where no placed record
/// fills the slot, then its flags.
const SLOT_COMPONENT: [usize; 3] = [44, 48, 52];
const SLOT_FLAGS: usize = 56;
/// The dialogue cues or object channels a slot holds, which only a flag that vouches for the
/// count may leave above zero.
const SLOT_COUNT: usize = 60;
const SLOT_RANGES: std::ops::Range<usize> = 64..80;

// Fields of the rows this build writes whole, for the objects a map adds. A slot's four strings
// are its id, its name and the ids of its sense and auth schemas. An occurrence's first is its id.
const OBJECT_KEY: usize = 12;
/// An object's seven counts: its placed configs, their descriptors, its handle groups, its
/// handles, the hops to its records, its handles without a record and its replicated placements.
const OBJECT_COUNTS: usize = 24;
const OCCURRENCE_FIELD: usize = 48;
const OCCURRENCE_ORDINAL: usize = 52;
const SLOT_TYPE: usize = 40;
/// The slot's component, sense and auth classes joined one to one, which a squad's slot must
/// state before the runtime places the squad.
const SLOT_JOINED: u32 = 0x2;

// Squad rows.
const SQUAD_ID: usize = 0;
const SQUAD_SCENARIO: usize = 8;
const SQUAD_OBJECT: usize = 12;
const SQUAD_SLOT: usize = 16;
const SQUAD_SPAWNER: usize = 20;
const SQUAD_RULE: usize = 24;
const SQUAD_FLAGS: usize = 28;
const SQUAD_OCCURRENCE: usize = 32;
const SQUAD_MEMBERS_RANGE: usize = 36;
/// A squad's descriptor, its spawner's rule references and its occurrence in the scenario are
/// read, and the rule resolves inside the squad's own object, which lets the client read the
/// squad's points from the rule. An added squad is built to hold all four.
const SQUAD_BUILT: u32 = 0x1 | 0x2 | 0x4 | 0x80;
/// What a squad takes from the installed squad it copies: its points, member count and
/// candidate counts as the runtime read them there.
const SQUAD_KEPT: u32 = 0x8 | 0x10 | 0x20;
/// The first schema version whose runtime lets a squad resolve its rule inside its own object,
/// the flag every added squad carries. An older runtime gets no rows for an added object.
const OWN_RULE_VERSION: u32 = 53;

// Squad member rows.
const MEMBER_ID: usize = 0;
const MEMBER_SQUAD: usize = 8;
const MEMBER_ORDINAL: usize = 12;
const MEMBER_ACTOR: usize = 20;
const MEMBER_FLAGS: usize = 24;
/// The member names one actor class row. A mod pack carries no actor classes, and a member
/// whose own spawn profile is certified needs none.
const MEMBER_ACTOR_EXACT: u32 = 0x1;

/// The slot type of an AI spawner and of a spawn rule, each with its component, sense and auth
/// classes.
const SQUAD_SLOT_TYPE: u32 = 1;
const SQUAD_CLASSES: [u32; 3] = [0x8080_9A3B, 0x8080_7ECC, 0x8080_7EC9];
const RULE_SLOT_TYPE: u32 = 66;
const RULE_CLASSES: [u32; 3] = [0x8080_94CF, ABSENT, ABSENT];

// Authored scene resource rows.
const SCENE_ID: usize = 0;
const SCENE_SLOT: usize = 8;
const SCENE_CONFIG_TAG: usize = 12;
const SCENE_RESOURCE_TAG: usize = 24;

/// One map's place in the pack: the installed scenario whose rows it takes and where the build
/// put the map's copies.
#[derive(Clone, Debug)]
pub(crate) struct Destination {
    /// The activity's internal name, which the runtime matches a launch against.
    pub name: String,
    /// The donor's scenario, as the installed pack lists it.
    pub donor_scenario: u32,
    pub scenario: u32,
    pub root: u32,
    /// Each donor tag a map that keeps its donor's bubbles copies, with its copy.
    pub moved: BTreeMap<u32, u32>,
    /// The slots of each donor object such a map copies, by the donor's tag, whose placed record
    /// the copy leaves out.
    pub lost: BTreeMap<u32, BTreeSet<u32>>,
    /// Each donor bubble name hash such a map renames, with the map's.
    pub bubbles: BTreeMap<u32, u32>,
    /// The encounter object such a map's registry adds.
    pub encounter: Option<Encounter>,
    /// The map's own bubbles in scenario order, when it rebuilds its donor's one bubble. Empty
    /// for a map that keeps its donor's bubbles.
    pub areas: Vec<Area>,
}

impl Destination {
    /// Whether any of the map's registries adds an encounter object.
    fn adds_squads(&self) -> bool {
        self.encounter.is_some() || self.areas.iter().any(|area| area.encounter.is_some())
    }
}

/// An encounter object a map's registry adds, with squads cloned from installed ones. The
/// installed pack has no rows for it, so its rows are written from what the build made: an
/// object of two handle groups whose slots are its squads' spawners, then their spawn rules.
#[derive(Clone, Debug)]
pub(crate) struct Encounter {
    pub object: u32,
    pub key: u32,
    /// The registry array that lists the object, by its field, and the object's place in it.
    pub field: u32,
    pub ordinal: u32,
    pub squads: Vec<Squad>,
}

/// One squad of an added encounter object.
#[derive(Clone, Debug)]
pub(crate) struct Squad {
    pub name: String,
    pub rule_name: String,
    /// The squad's spawner and spawn rule records.
    pub spawner: u32,
    pub rule: u32,
    /// The installed spawner record the squad's is a copy of, whose squad row and members it
    /// takes.
    pub template: u32,
}

/// One bubble of a map that rebuilds its donor's one bubble as connected areas.
#[derive(Clone, Debug)]
pub(crate) struct Area {
    /// The bubble's name hash.
    pub name: u32,
    /// The hash of the area's one state, or `None` where it keeps the donor's.
    pub state: Option<u32>,
    /// Each donor tag the area's registry copies, with its copy.
    pub moved: BTreeMap<u32, u32>,
    /// The slots of each donor object the area copies, by the donor's tag, whose placed record
    /// the copy leaves out.
    pub lost: BTreeMap<u32, BTreeSet<u32>>,
    /// The encounter object the area's registry adds.
    pub encounter: Option<Encounter>,
}

/// The installed pack and where each of its sections starts.
struct Installed<'a> {
    bytes: &'a [u8],
    /// The schema version the runtime that generated the pack reads.
    version: u32,
    /// Each section's first byte, row count and row size.
    sections: Vec<(usize, usize, usize)>,
}

impl<'a> Installed<'a> {
    fn read(bytes: &'a [u8]) -> AuthoringResult<Self> {
        let layout = || invalid("The installed activity SDK pack has another layout");
        if bytes.len() < header(0) || &bytes[..MAGIC.len()] != MAGIC {
            return Err(layout());
        }
        let version = read_u32(bytes, VERSION)?;
        let Some(&(_, count)) = LAYOUTS.iter().find(|(known, _)| *known == version) else {
            return Err(invalid(format!(
                "The installed activity SDK pack has schema version {version}, which this build \
                 does not copy map rows from"
            )));
        };
        if bytes.len() < header(count)
            || read_u32(bytes, HEADER_LENGTH)? as usize != header(count)
            || read_u32(bytes, SECTION_COUNT)? as usize != count
            || read_u64(bytes, FILE_LENGTH)? != bytes.len() as u64
        {
            return Err(layout());
        }
        let mut sections = Vec::with_capacity(count);
        let mut end = header(count);
        for index in 0..count {
            let at = SECTION_TABLE + index * SECTION_ROW;
            let offset = usize::try_from(read_u64(bytes, at)?).map_err(|_| layout())?;
            let count = read_u32(bytes, at + 8)? as usize;
            let stride = read_u32(bytes, at + 12)? as usize;
            let length = count.checked_mul(stride).ok_or_else(layout)?;
            if stride == 0 || offset != end || length > bytes.len() - end {
                return Err(layout());
            }
            sections.push((offset, count, stride));
            end += length;
        }
        if end != bytes.len() {
            return Err(layout());
        }
        Ok(Self {
            bytes,
            version,
            sections,
        })
    }

    /// How many rows `section` holds. Its rows must have the size this build writes.
    fn count(&self, section: Section) -> AuthoringResult<usize> {
        let (_, count, stride) = self.sections[section.index];
        if stride != section.size {
            return Err(invalid(format!(
                "The installed activity SDK stores section {} rows in {stride} bytes, not {}",
                section.index, section.size
            )));
        }
        Ok(count)
    }

    fn row(&self, section: Section, index: usize) -> AuthoringResult<&'a [u8]> {
        if index >= self.count(section)? {
            return Err(invalid(format!(
                "The installed activity SDK has no row {index} in section {}",
                section.index
            )));
        }
        let start = self.sections[section.index].0 + index * section.size;
        Ok(&self.bytes[start..start + section.size])
    }

    /// The bytes the string reference at `field` of `row` names.
    fn text(&self, row: &[u8], field: usize) -> AuthoringResult<&'a [u8]> {
        let (start, length, _) = self.sections[STRINGS];
        let offset = read_u32(row, field)? as usize;
        let end = offset + read_u32(row, field + 4)? as usize;
        if end > length {
            return Err(invalid(
                "An installed activity SDK row names text outside the pack",
            ));
        }
        Ok(&self.bytes[start + offset..start + end])
    }
}

/// The rows a mod pack carries. Every other section is written empty.
#[derive(Default)]
struct Tables {
    strings: Vec<u8>,
    interned: BTreeMap<Vec<u8>, [u8; 8]>,
    /// Each written section's rows, by its place in the header.
    rows: BTreeMap<usize, Vec<u8>>,
}

impl Tables {
    /// A reference to `value`, added once. The empty string is the empty reference.
    fn text(&mut self, value: &[u8]) -> AuthoringResult<[u8; 8]> {
        if value.is_empty() {
            return Ok([0; 8]);
        }
        if let Some(known) = self.interned.get(value) {
            return Ok(*known);
        }
        let size = || invalid("The activity SDK pack's text exceeds 32 bits");
        let offset = u32::try_from(self.strings.len()).map_err(|_| size())?;
        let length = u32::try_from(value.len()).map_err(|_| size())?;
        let mut reference = [0; 8];
        reference[..4].copy_from_slice(&offset.to_le_bytes());
        reference[4..].copy_from_slice(&length.to_le_bytes());
        self.strings.extend_from_slice(value);
        self.interned.insert(value.to_vec(), reference);
        Ok(reference)
    }

    fn count(&self, section: Section) -> usize {
        self.rows
            .get(&section.index)
            .map_or(0, |rows| rows.len() / section.size)
    }

    /// Adds `row` to `section` and returns its index.
    fn push(&mut self, section: Section, row: &[u8]) -> AuthoringResult<u32> {
        if row.len() != section.size {
            return Err(invalid("An activity SDK row has the wrong size"));
        }
        let index = u32::try_from(self.count(section))
            .map_err(|_| invalid("The activity SDK pack has too many rows"))?;
        self.rows
            .entry(section.index)
            .or_default()
            .extend_from_slice(row);
        Ok(index)
    }

    /// Points the string reference at `field` of `row` at `value`.
    fn name(&mut self, row: &mut [u8], field: usize, value: &str) -> AuthoringResult<()> {
        let reference = self.text(value.as_bytes())?;
        row[field..field + 8].copy_from_slice(&reference);
        Ok(())
    }

    /// Carries the installed text each of `fields` names in `row` over to this pack.
    fn carry(
        &mut self,
        installed: &Installed<'_>,
        row: &mut [u8],
        fields: &[usize],
    ) -> AuthoringResult<()> {
        for &field in fields {
            let reference = self.text(installed.text(row, field)?)?;
            row[field..field + 8].copy_from_slice(&reference);
        }
        Ok(())
    }
}

/// The first row and row count of the range at `field`.
fn range(row: &[u8], field: usize) -> AuthoringResult<(usize, usize)> {
    Ok((
        read_u32(row, field)? as usize,
        read_u32(row, field + 4)? as usize,
    ))
}

/// Writes the range from row `first` to the section's current end.
fn close(row: &mut [u8], field: usize, first: usize, end: usize) -> AuthoringResult<()> {
    let word = |value: usize| {
        u32::try_from(value).map_err(|_| invalid("The activity SDK pack has too many rows"))
    };
    write_u32(row, field, word(first)?)?;
    write_u32(row, field + 4, word(end - first)?)
}

/// One map's rows as they are added, and the objects it has copied so far.
struct Copier<'a, 'b> {
    installed: &'b Installed<'a>,
    /// The installed scene resource rows of each installed slot row.
    scenes: &'b BTreeMap<u32, Vec<usize>>,
    map: &'b Destination,
    /// The map's scenario row in this pack.
    scenario: u32,
    /// The bubble being copied: its place in the map's scenario and its row in this pack.
    ordinal: u32,
    bubble: u32,
    /// The donor bubble it takes its rows from, by its place in the donor's scenario.
    source: u32,
    /// Its area, when the map rebuilds its donor's bubble.
    area: Option<&'b Area>,
    /// Each donor tag it copies, with its copy.
    moved: &'b BTreeMap<u32, u32>,
    /// The slots of each donor object it copies whose placed record the copy leaves out.
    lost: &'b BTreeMap<u32, BTreeSet<u32>>,
    /// Each installed object row the map has copied under a tag, with its row in this pack.
    objects: BTreeMap<(u32, u32), u32>,
    /// The installed squad row of each installed spawner record, or `None` where the runtime
    /// that reads this pack takes no added squads.
    squads: Option<&'b BTreeMap<u32, usize>>,
    /// The encounter object the registry being copied adds, until its rows are written.
    encounter: Option<&'b Encounter>,
}

impl Copier<'_, '_> {
    /// Writes the rows of the `encounter` object the copy's registry adds: the object, a slot for
    /// each squad's spawner and then for each spawn rule, its occurrence in the state at row
    /// `state`, and each squad with its members. `pattern` is an occurrence that state already
    /// holds, whose registry context the object shares.
    ///
    /// The object's rows follow what the runtime's own pack holds for an installed encounter
    /// object: every placed record is a config with a descriptor, the handle of the spawn point
    /// list reaches no record, each config costs three hops and the list one, and an AI spawner
    /// is not a replicated placement, so the object stays out of a mission's seed. A squad takes
    /// the row and members of the installed squad its spawner is a copy of. One whose spawner
    /// the installed pack lists no squad for gets no row, and the runtime cannot place it.
    fn encounter_rows(
        &mut self,
        tables: &mut Tables,
        encounter: &Encounter,
        squads: &BTreeMap<u32, usize>,
        state: u32,
        pattern: &[u8],
    ) -> AuthoringResult<()> {
        let word = |value: usize| {
            u32::try_from(value).map_err(|_| invalid("The activity SDK pack has too many rows"))
        };
        let tag = encounter.object;
        let object_row = word(tables.count(OBJECTS))?;
        let first_slot = tables.count(SLOTS);
        let spawners = encounter
            .squads
            .iter()
            .map(|squad| (squad.name.as_str(), SQUAD_SLOT_TYPE, SQUAD_CLASSES));
        let rules = encounter
            .squads
            .iter()
            .map(|squad| (squad.rule_name.as_str(), RULE_SLOT_TYPE, RULE_CLASSES));
        for (index, (name, kind, classes)) in spawners.chain(rules).enumerate() {
            let mut slot = vec![0; SLOTS.size];
            let id = format!("slot/{tag:08x}/{index:06x}/{index:04x}/{kind:04x}");
            tables.name(&mut slot, SLOT_TEXTS[0], &id)?;
            tables.name(&mut slot, SLOT_TEXTS[1], name)?;
            for (field, class) in SLOT_TEXTS[2..].iter().zip(&classes[1..]) {
                if *class != ABSENT {
                    tables.name(&mut slot, *field, &format!("schema/{class:08x}"))?;
                }
            }
            write_u32(&mut slot, SLOT_OBJECT, object_row)?;
            write_u32(&mut slot, SLOT_INDEX, word(index)?)?;
            write_u32(&mut slot, SLOT_TYPE, kind)?;
            for (field, class) in SLOT_COMPONENT.into_iter().zip(classes) {
                write_u32(&mut slot, field, class)?;
            }
            write_u32(&mut slot, SLOT_FLAGS, SLOT_JOINED)?;
            tables.push(SLOTS, &slot)?;
        }
        let mut object = vec![0; OBJECTS.size];
        tables.name(&mut object, OBJECT_ID, &format!("object/{tag:08x}"))?;
        write_u32(&mut object, OBJECT_TAG, tag)?;
        write_u32(&mut object, OBJECT_KEY, encounter.key)?;
        close(&mut object, OBJECT_SLOTS, first_slot, tables.count(SLOTS))?;
        let configs = 2 * encounter.squads.len();
        let counts = [configs, configs, 2, configs + 1, 3 * configs + 1, 1, 0];
        for (index, count) in counts.into_iter().enumerate() {
            write_u32(&mut object, OBJECT_COUNTS + index * 4, word(count)?)?;
        }
        tables.push(OBJECTS, &object)?;

        let mut occurrence = pattern.to_vec();
        let id = format!(
            "object-occurrence/{:08x}/{:04x}/{state:04x}/{:02x}/{:06x}/{tag:08x}",
            self.map.scenario, self.ordinal, encounter.field, encounter.ordinal
        );
        tables.name(&mut occurrence, OCCURRENCE_TEXTS[0], &id)?;
        write_u32(&mut occurrence, OCCURRENCE_OBJECT, object_row)?;
        write_u32(&mut occurrence, OCCURRENCE_FIELD, encounter.field)?;
        write_u32(&mut occurrence, OCCURRENCE_ORDINAL, encounter.ordinal)?;
        let occurrence_row = tables.push(OCCURRENCES, &occurrence)?;

        for (index, squad) in encounter.squads.iter().enumerate() {
            let Some(&source) = squads.get(&squad.template) else {
                continue;
            };
            let installed = self.installed.row(SQUADS, source)?;
            let own = word(tables.count(SQUADS))?;
            let first_member = tables.count(SQUAD_MEMBERS);
            let (members, count) = range(installed, SQUAD_MEMBERS_RANGE)?;
            for member in members..members + count {
                let mut row = self.installed.row(SQUAD_MEMBERS, member)?.to_vec();
                let id = format!(
                    "squad-member/{tag:08x}/{index:04x}/{:06x}",
                    read_u32(&row, MEMBER_ORDINAL)?
                );
                tables.name(&mut row, MEMBER_ID, &id)?;
                write_u32(&mut row, MEMBER_SQUAD, own)?;
                write_u32(&mut row, MEMBER_ACTOR, ABSENT)?;
                let flags = read_u32(&row, MEMBER_FLAGS)? & !MEMBER_ACTOR_EXACT;
                write_u32(&mut row, MEMBER_FLAGS, flags)?;
                tables.push(SQUAD_MEMBERS, &row)?;
            }
            // The row's anchors stay empty: the client reads the squad's points from its rule.
            let mut row = vec![0; SQUADS.size];
            tables.name(&mut row, SQUAD_ID, &format!("squad/{tag:08x}/{index:04x}"))?;
            write_u32(&mut row, SQUAD_SCENARIO, self.scenario)?;
            write_u32(&mut row, SQUAD_OBJECT, object_row)?;
            write_u32(&mut row, SQUAD_SLOT, word(first_slot + index)?)?;
            write_u32(&mut row, SQUAD_SPAWNER, squad.spawner)?;
            write_u32(&mut row, SQUAD_RULE, squad.rule)?;
            let flags = read_u32(installed, SQUAD_FLAGS)? & SQUAD_KEPT | SQUAD_BUILT;
            write_u32(&mut row, SQUAD_FLAGS, flags)?;
            write_u32(&mut row, SQUAD_OCCURRENCE, occurrence_row)?;
            close(
                &mut row,
                SQUAD_MEMBERS_RANGE,
                first_member,
                tables.count(SQUAD_MEMBERS),
            )?;
            tables.push(SQUADS, &row)?;
        }
        Ok(())
    }

    /// The copy of an installed tag in the bubble being copied, or the tag itself.
    fn tag(&self, installed: u32) -> u32 {
        self.moved.get(&installed).copied().unwrap_or(installed)
    }

    /// Copies an installed object and its slots the first time a state names it under its tag.
    /// Areas share an object none of them copies.
    fn object(&mut self, tables: &mut Tables, source: u32) -> AuthoringResult<u32> {
        let mut object = self.installed.row(OBJECTS, source as usize)?.to_vec();
        let donor = read_u32(&object, OBJECT_TAG)?;
        let tag = self.tag(donor);
        if let Some(&row) = self.objects.get(&(source, tag)) {
            return Ok(row);
        }
        let lost = self.lost;
        let lost = lost.get(&donor);
        let row = u32::try_from(tables.count(OBJECTS))
            .map_err(|_| invalid("The activity SDK pack has too many rows"))?;
        let (first, count) = range(&object, OBJECT_SLOTS)?;
        let first_slot = tables.count(SLOTS);
        for source_slot in first..first + count {
            let mut slot = self.installed.row(SLOTS, source_slot)?.to_vec();
            tables.carry(self.installed, &mut slot, &SLOT_TEXTS)?;
            write_u32(&mut slot, SLOT_OBJECT, row)?;
            slot[SLOT_RANGES].fill(0);
            // The copy leaves this slot's placed record out, so nothing fills the slot.
            let index = read_u32(&slot, SLOT_INDEX)?;
            let left_out = lost.is_some_and(|lost| lost.contains(&index));
            if left_out {
                for field in SLOT_COMPONENT {
                    write_u32(&mut slot, field, ABSENT)?;
                }
                write_u32(&mut slot, SLOT_FLAGS, 0)?;
                write_u32(&mut slot, SLOT_COUNT, 0)?;
            }
            let own_slot = tables.push(SLOTS, &slot)?;
            if left_out {
                continue;
            }
            for &scene in self.scenes.get(&(source_slot as u32)).into_iter().flatten() {
                let mut resource = self.installed.row(SCENES, scene)?.to_vec();
                tables.carry(self.installed, &mut resource, &[SCENE_ID])?;
                write_u32(&mut resource, SCENE_SLOT, own_slot)?;
                for field in [SCENE_CONFIG_TAG, SCENE_RESOURCE_TAG] {
                    let tag = self.tag(read_u32(&resource, field)?);
                    write_u32(&mut resource, field, tag)?;
                }
                tables.push(SCENES, &resource)?;
            }
        }
        tables.name(&mut object, OBJECT_ID, &format!("object/{tag:08x}"))?;
        write_u32(&mut object, OBJECT_TAG, tag)?;
        close(&mut object, OBJECT_SLOTS, first_slot, tables.count(SLOTS))?;
        tables.push(OBJECTS, &object)?;
        self.objects.insert((source, tag), row);
        Ok(row)
    }

    /// Copies the installed state `source` into the bubble being copied, with its occurrences
    /// among the scenario's `occurrences`.
    fn state(
        &mut self,
        tables: &mut Tables,
        source: usize,
        occurrences: (usize, usize),
    ) -> AuthoringResult<()> {
        let mut state = self.installed.row(STATES, source)?.to_vec();
        if read_u32(&state, STATE_ENTRY)? != self.source
            || read_u32(&state, STATE_SLICE_SET)? != self.source * SLICE_SETS_PER_BUBBLE
        {
            return Err(invalid(format!(
                "Installed state {source} of {} does not start at its bubble's slice sets",
                self.map.name
            )));
        }
        let id = format!(
            "state/{:08x}/{:04x}/{:04x}",
            self.map.scenario,
            self.ordinal,
            read_u32(&state, STATE_ORDINAL)?
        );
        tables.name(&mut state, STATE_ID, &id)?;
        tables.carry(
            self.installed,
            &mut state,
            &[STATE_ENTRY_ID, STATE_REGISTRY_ID],
        )?;
        let bubble = self.bubble;
        write_u32(&mut state, STATE_SCENARIO, self.scenario)?;
        write_u32(&mut state, STATE_BUBBLE, bubble)?;
        write_u32(&mut state, STATE_ENTRY, self.ordinal)?;
        write_u32(
            &mut state,
            STATE_SLICE_SET,
            self.ordinal * SLICE_SETS_PER_BUBBLE,
        )?;
        if let Some(area) = self.area {
            // An area's state names the area's place in the map's own layout, and its own hash
            // where the map gave it one.
            write_u32(&mut state, STATE_MAP_BUBBLE, self.ordinal)?;
            if let Some(hash) = area.state {
                write_u32(&mut state, STATE_HASH, hash)?;
            }
        }
        let registry = self.tag(read_u32(&state, STATE_REGISTRY_TAG)?);
        write_u32(&mut state, STATE_REGISTRY_TAG, registry)?;
        let own = tables.push(STATES, &state)?;
        let mut pattern = None;
        for index in occurrences.0..occurrences.0 + occurrences.1 {
            let installed = self.installed.row(OCCURRENCES, index)?;
            if read_u32(installed, OCCURRENCE_STATE)? as usize != source {
                continue;
            }
            let mut occurrence = installed.to_vec();
            tables.carry(self.installed, &mut occurrence, &OCCURRENCE_TEXTS)?;
            write_u32(&mut occurrence, OCCURRENCE_SCENARIO, self.scenario)?;
            write_u32(&mut occurrence, OCCURRENCE_BUBBLE, bubble)?;
            write_u32(&mut occurrence, OCCURRENCE_STATE, own)?;
            let object = self.object(tables, read_u32(installed, OCCURRENCE_OBJECT)?)?;
            write_u32(&mut occurrence, OCCURRENCE_OBJECT, object)?;
            tables.push(OCCURRENCES, &occurrence)?;
            pattern.get_or_insert(occurrence);
        }
        // The state whose registry the map copies is the one that lists the object the copy adds.
        let copied = self.moved.contains_key(&read_u32(
            self.installed.row(STATES, source)?,
            STATE_REGISTRY_TAG,
        )?);
        if let (true, Some(pattern), Some(encounter), Some(squads)) =
            (copied, pattern, self.encounter, self.squads)
        {
            self.encounter = None;
            self.encounter_rows(tables, encounter, squads, own, &pattern)?;
        }
        Ok(())
    }
}

/// Adds `map`'s activity, its scenario and every row the scenario reaches.
fn add(
    tables: &mut Tables,
    installed: &Installed<'_>,
    scenes: &BTreeMap<u32, Vec<usize>>,
    squads: Option<&BTreeMap<u32, usize>>,
    map: &Destination,
) -> AuthoringResult<()> {
    let source = (0..installed.count(SCENARIOS)?)
        .map(|index| installed.row(SCENARIOS, index))
        .find(|row| {
            row.as_ref()
                .is_ok_and(|row| read_u32(row, SCENARIO_TAG).ok() == Some(map.donor_scenario))
        })
        .transpose()?
        .ok_or_else(|| {
            invalid(format!(
                "The installed activity SDK has no scenario {:08X}, which map {} copies",
                map.donor_scenario, map.name
            ))
        })?;
    let scenario = u32::try_from(tables.count(SCENARIOS))
        .map_err(|_| invalid("The activity SDK pack has too many rows"))?;
    let (first_bubble, bubbles) = range(source, SCENARIO_BUBBLES)?;
    let occurrences = range(source, SCENARIO_OCCURRENCES)?;
    let first = (
        tables.count(BUBBLES),
        tables.count(STATES),
        tables.count(OCCURRENCES),
    );
    // A map that keeps its donor's bubbles takes each in its place. One that rebuilds them takes
    // the donor's only bubble once for each of its areas.
    let places: Vec<(usize, Option<&Area>)> = if map.areas.is_empty() {
        (0..bubbles).map(|position| (position, None)).collect()
    } else if bubbles == 1 {
        map.areas.iter().map(|area| (0, Some(area))).collect()
    } else {
        return Err(invalid(format!(
            "Map {} rebuilds the bubbles of a donor that has {bubbles}",
            map.name
        )));
    };
    let mut copy = Copier {
        installed,
        scenes,
        map,
        scenario,
        ordinal: 0,
        bubble: 0,
        source: 0,
        area: None,
        moved: &map.moved,
        lost: &map.lost,
        objects: BTreeMap::new(),
        squads,
        encounter: map.encounter.as_ref(),
    };
    for (position, (source, area)) in places.into_iter().enumerate() {
        let mut bubble = installed.row(BUBBLES, first_bubble + source)?.to_vec();
        // The donor's rows are positional, so its row k is its bubble k.
        if read_u32(&bubble, BUBBLE_ORDINAL)? as usize != source {
            return Err(invalid(format!(
                "Bubble row {source} of {} has no installed scenario row",
                map.name
            )));
        }
        let donor_hash = read_u32(&bubble, BUBBLE_NAME_HASH)?;
        let hash = match area {
            Some(area) => area.name,
            None => map.bubbles.get(&donor_hash).copied().unwrap_or(donor_hash),
        };
        let ordinal = position as u32;
        copy.ordinal = ordinal;
        copy.bubble = u32::try_from(tables.count(BUBBLES))
            .map_err(|_| invalid("The activity SDK pack has too many rows"))?;
        copy.source = source as u32;
        copy.area = area;
        copy.moved = area.map_or(&map.moved, |area| &area.moved);
        copy.lost = area.map_or(&map.lost, |area| &area.lost);
        if let Some(area) = area {
            copy.encounter = area.encounter.as_ref();
        }
        let first_state = tables.count(STATES);
        let (states, count) = range(&bubble, BUBBLE_STATES)?;
        for state in states..states + count {
            copy.state(tables, state, occurrences)?;
        }
        let id = format!("bubble/{:08x}/{ordinal:04x}/{hash:08x}", map.scenario);
        tables.name(&mut bubble, BUBBLE_ID, &id)?;
        tables.carry(installed, &mut bubble, &[BUBBLE_NAME])?;
        write_u32(&mut bubble, BUBBLE_SCENARIO, scenario)?;
        write_u32(&mut bubble, BUBBLE_ORDINAL, ordinal)?;
        write_u32(&mut bubble, BUBBLE_NAME_HASH, hash)?;
        close(
            &mut bubble,
            BUBBLE_STATES,
            first_state,
            tables.count(STATES),
        )?;
        tables.push(BUBBLES, &bubble)?;
    }
    let mut row = vec![0; SCENARIOS.size];
    write_u32(&mut row, SCENARIO_TAG, map.scenario)?;
    tables.name(
        &mut row,
        SCENARIO_ID,
        &format!("scenario/{:08x}", map.scenario),
    )?;
    tables.name(&mut row, SCENARIO_NAME, &map.name)?;
    close(&mut row, SCENARIO_BUBBLES, first.0, tables.count(BUBBLES))?;
    close(&mut row, SCENARIO_STATES, first.1, tables.count(STATES))?;
    close(
        &mut row,
        SCENARIO_OCCURRENCES,
        first.2,
        tables.count(OCCURRENCES),
    )?;
    tables.push(SCENARIOS, &row)?;
    let mut activity = vec![0; ACTIVITIES.size];
    write_u32(&mut activity, ACTIVITY_INDEX, ABSENT)?;
    tables.name(&mut activity, ACTIVITY_ID, &format!("act/{}", map.name))?;
    tables.name(&mut activity, ACTIVITY_INTERNAL_NAME, &map.name)?;
    tables.name(&mut activity, ACTIVITY_DISPLAY_NAME, &map.name)?;
    write_u32(&mut activity, ACTIVITY_SCENARIO, scenario)?;
    write_u32(&mut activity, ACTIVITY_FLAGS, EXACT)?;
    write_u32(&mut activity, ACTIVITY_ROOT_TAG, map.root)?;
    write_u32(&mut activity, ACTIVITY_SCENARIO_TAG, map.scenario)?;
    write_u32(&mut activity, ACTIVITY_MATCHMAKING_TAG, ABSENT)?;
    tables.push(ACTIVITIES, &activity)?;
    Ok(())
}

/// The whole file: the header, then every section in order with the installed pack's row sizes,
/// so the two layouts cannot drift.
fn write(installed: &Installed<'_>, tables: &Tables) -> AuthoringResult<Vec<u8>> {
    let start = header(installed.sections.len());
    let mut image = vec![0; start];
    image[..MAGIC.len()].copy_from_slice(MAGIC);
    image[VERSION..VERSION + 4].copy_from_slice(&installed.bytes[VERSION..VERSION + 4]);
    write_u32(&mut image, HEADER_LENGTH, start as u32)?;
    write_u32(&mut image, SECTION_COUNT, installed.sections.len() as u32)?;
    for (index, &(_, _, stride)) in installed.sections.iter().enumerate() {
        let rows = if index == STRINGS {
            tables.strings.as_slice()
        } else {
            tables.rows.get(&index).map_or(&[][..], Vec::as_slice)
        };
        let count = u32::try_from(rows.len() / stride)
            .map_err(|_| invalid("The activity SDK pack has too many rows"))?;
        let at = SECTION_TABLE + index * SECTION_ROW;
        let offset = image.len() as u64;
        write_u64(&mut image, at, offset)?;
        write_u32(&mut image, at + 8, count)?;
        write_u32(&mut image, at + 12, stride as u32)?;
        image.extend_from_slice(rows);
    }
    let length = image.len() as u64;
    write_u64(&mut image, FILE_LENGTH, length)?;
    let digest = Sha256::digest(&image[start..]);
    image[PAYLOAD_DIGEST..PAYLOAD_DIGEST + digest.len()].copy_from_slice(&digest[..]);
    Ok(image)
}

/// The pack for `maps`, whose rows come from the bytes of the `installed` pack.
pub(crate) fn build(installed: &[u8], maps: &[Destination]) -> AuthoringResult<Vec<u8>> {
    if maps.is_empty() {
        return Err(invalid("An activity SDK pack needs a map"));
    }
    // The runtime resolves a mod activity only when one row names it.
    let mut names = BTreeSet::new();
    if let Some(map) = maps.iter().find(|map| !names.insert(map.name.as_str())) {
        return Err(invalid(format!(
            "Two maps share the activity name {}",
            map.name
        )));
    }
    let installed = Installed::read(installed)?;
    let mut scenes = BTreeMap::<u32, Vec<usize>>::new();
    for index in 0..installed.count(SCENES)? {
        let slot = read_u32(installed.row(SCENES, index)?, SCENE_SLOT)?;
        scenes.entry(slot).or_default().push(index);
    }
    // An installed spawner record's first squad row, for the squads the maps add.
    let mut squads = BTreeMap::<u32, usize>::new();
    let added = installed.version >= OWN_RULE_VERSION;
    if added && maps.iter().any(Destination::adds_squads) {
        for index in 0..installed.count(SQUADS)? {
            let spawner = read_u32(installed.row(SQUADS, index)?, SQUAD_SPAWNER)?;
            squads.entry(spawner).or_insert(index);
        }
        installed.count(SQUAD_MEMBERS)?;
    }
    let mut tables = Tables::default();
    for map in maps {
        add(
            &mut tables,
            &installed,
            &scenes,
            added.then_some(&squads),
            map,
        )?;
    }
    write(&installed, &tables)
}

/// What [`stage`] wrote for a build's maps.
pub(crate) enum Staged {
    /// The build holds no map with recorded rows.
    None,
    /// The pack, beside the mod's packages.
    Pack(ArtifactMetadata),
    /// The build holds maps, but Sunrise has not generated its own pack in this installation yet,
    /// so there are no rows to copy. Sunrise writes it the first time it starts.
    NotGenerated,
}

/// Writes the pack of the selected maps the build recorded rows for into `directory`. Their rows
/// come from the pack Sunrise generated in its own folder of the installation that holds
/// `packages`. A build with no such map stages none, and neither does an installation whose
/// Sunrise has not generated its pack yet.
pub(crate) fn stage(
    directory: &Path,
    recipes: &[MapRecipe],
    packages: &Path,
) -> Result<Staged, String> {
    let maps = crate::mod_family::build::map_rows()
        .into_iter()
        .filter(|(name, _)| recipes.iter().any(|recipe| recipe.name == *name))
        .map(|(_, rows)| rows)
        .collect::<Vec<_>>();
    if maps.is_empty() {
        return Ok(Staged::None);
    }
    let install = packages
        .parent()
        .ok_or("The package directory has no install root")?;
    let installed = mod_content::runtime_directory(install).join(FILE_NAME);
    let bytes = match std::fs::read(&installed) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Staged::NotGenerated);
        }
        Err(error) => return Err(format!("Could not read {}: {error}", installed.display())),
    };
    let pack = build(&bytes, &maps).map_err(|error| error.to_string())?;
    if pack.len() as u64 > BYTE_LIMIT {
        return Err("The maps' activity SDK pack exceeds its byte limit".into());
    }
    std::fs::write(directory.join(FILE_NAME), &pack).map_err(|error| error.to_string())?;
    let digest = digest_bytes(&pack);
    Ok(Staged::Pack(ArtifactMetadata {
        file_name: FILE_NAME.into(),
        byte_length: digest.byte_length,
        sha256: digest.sha256,
    }))
}
