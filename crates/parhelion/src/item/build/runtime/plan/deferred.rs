//! Private relations load their groups with them, as stock firing presentations and weapon sounds
//! do.
//!
//! A resident component names a relation (class `8080744A`, type 16), and the client loads the
//! relation's companion group when the component asks for it. Stock leaves the relation, its
//! companion and the entries only that group needs out of the investment index, so a weapon's
//! firing presentation and sounds stay out of the tag heap until the weapon uses them. A private
//! relation with a companion of its own gets the same treatment here. Each authored entry that
//! only private relations reach joins the group of every relation that reaches it and leaves the
//! investment index.
//!
//! An entry stays resident when a resident entry names it, by a payload word or an entry
//! reference. A relation that no resident entry names stays resident with its whole group, since
//! nothing resident would ask for it, and so does every relation with an entry in a package that
//! would otherwise keep none in the index, since the client registers a package only when the
//! index names one of its entries. Payload words are read as candidate references, so an unrelated
//! word that equals an authored tag can only keep more resident. Native entries the groups reach
//! keep the enrollment the native prerequisites give them.
//!
//! Keeping named entries resident is required, not only cautious. Stock never has a resident record
//! name an entry that loads later through anything but a type-16 owner, and none of its resident
//! first-person banks names a clip that loads later. A Shadowkeep client under Dawn 1.7.2 loaded a
//! weapon whose resident bank named clips listed only by the weapon's gear-art groups, and it
//! played them when the weapon was equipped at activity load. Equipping that weapon mid-activity
//! stopped the main loop instead: the main thread waited on an event that was never signalled.
use super::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const RELATION_CLASS: u32 = 0x8080_744A;
const COMPANION_CLASS: u32 = 0x8080_9EF9;

#[derive(Clone, Copy)]
enum Place {
    Weapon(usize),
    Asset(usize, usize),
    /// Private perk copies are read only as resident referrers.
    PrivatePerk(usize),
}

struct Entry {
    place: Place,
    class: u32,
    file_type: u8,
    size: usize,
}

/// Every authored entry of the runtime stage by its final tag, and the authored entries each
/// names.
struct Graph {
    entries: HashMap<u32, Entry>,
    names: HashMap<u32, Vec<u32>>,
}

impl Graph {
    fn read(payloads: &Payloads, manager: &PackageManager) -> AuthoringResult<Self> {
        let mut entries = HashMap::new();
        let mut add = |tag: TagHash, place: Place, spec: &NewTagSpec| -> AuthoringResult<()> {
            let entry = manager
                .get_entry(spec.template_tag)
                .ok_or_else(|| invalid("On-demand loading: an entry's template is missing"))?;
            entries.insert(
                tag.0,
                Entry {
                    place,
                    class: entry.reference,
                    file_type: entry.file_type,
                    size: spec.payload.len(),
                },
            );
            Ok(())
        };
        for (index, spec) in payloads.weapon_tags.iter().enumerate() {
            let tag = payloads.weapon_allocator.assigned_tag(
                index,
                "On-demand loading",
                "weapon runtime",
            )?;
            add(tag, Place::Weapon(index), spec)?;
        }
        for (index, spec) in payloads.private_perk_tags.iter().enumerate() {
            let tag = payloads.private_perk_allocator.assigned_tag(
                index,
                "On-demand loading",
                "private perk",
            )?;
            add(tag, Place::PrivatePerk(index), spec)?;
        }
        for (package_index, package) in payloads.asset_packages.packages.iter().enumerate() {
            let allocator = AppendedTagAllocator::new(package.id, 0);
            for (index, spec) in package.tags.iter().enumerate() {
                let tag = allocator.assigned_tag(index, "On-demand loading", "asset")?;
                add(tag, Place::Asset(package_index, index), spec)?;
            }
        }
        // Only structured records hold tag words. Media, shader bytecode and texture data do not.
        let mut names = HashMap::with_capacity(entries.len());
        for (&tag, entry) in &entries {
            if !matches!(entry.file_type, 8 | 16) {
                continue;
            }
            let mut named = payloads
                .payload(entry.place)
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                .filter(|word| *word != tag && entries.contains_key(word))
                .collect::<Vec<_>>();
            named.sort_unstable();
            named.dedup();
            names.insert(tag, named);
        }
        // An entry reference is a dependency the payload does not show.
        for package in &payloads.asset_packages.packages {
            let allocator = AppendedTagAllocator::new(package.id, 0);
            for reference in &package.references {
                let target = match reference.reference {
                    crate::NewTagReference::Tag(tag) => tag,
                    crate::NewTagReference::Appended(ordinal) => {
                        allocator.assigned_tag(ordinal, "On-demand loading", "reference")?
                    }
                    crate::NewTagReference::Template => continue,
                };
                let source = allocator.assigned_tag(
                    reference.new_tag_ordinal,
                    "On-demand loading",
                    "referrer",
                )?;
                if target != source && entries.contains_key(&target.0) {
                    let named = names.entry(source.0).or_default();
                    if !named.contains(&target.0) {
                        named.push(target.0);
                    }
                }
            }
        }
        Ok(Self { entries, names })
    }

    fn names(&self, tag: u32) -> &[u32] {
        self.names.get(&tag).map_or(&[], Vec::as_slice)
    }

    /// A type-16 owner or a loading index, which a group walk does not cross.
    fn boundary(&self, tag: u32) -> bool {
        self.entries
            .get(&tag)
            .is_some_and(|entry| entry.file_type == 16 || entry.class == COMPANION_CLASS)
    }

    /// Each private relation in the weapon runtime by its companion there, which names it.
    fn relations(&self, payloads: &Payloads) -> AuthoringResult<BTreeMap<u32, u32>> {
        let mut relations = BTreeMap::new();
        for (&tag, entry) in &self.entries {
            if entry.class != COMPANION_CLASS
                || entry.file_type != 8
                || !matches!(entry.place, Place::Weapon(_))
            {
                continue;
            }
            let payload = payloads.payload(entry.place);
            let (own, owner) = (read_u32(payload, 8)?, read_u32(payload, 12)?);
            let is_relation = self.entries.get(&owner).is_some_and(|relation| {
                relation.class == RELATION_CLASS
                    && relation.file_type == 16
                    && matches!(relation.place, Place::Weapon(_))
            });
            if own != tag || !is_relation {
                continue;
            }
            if relations.insert(owner, tag).is_some() {
                return Err(invalid(format!(
                    "Private relation 0x{owner:08X} has two loading indexes"
                )));
            }
        }
        Ok(relations)
    }

    /// The authored entries `relation` reaches without crossing another boundary.
    fn reach(&self, relation: u32) -> BTreeSet<u32> {
        let mut reached = BTreeSet::new();
        let mut pending = self.names(relation).to_vec();
        while let Some(tag) = pending.pop() {
            if self.boundary(tag) || !reached.insert(tag) {
                continue;
            }
            pending.extend_from_slice(self.names(tag));
        }
        reached
    }
}

/// Which entries stay resident once the relations in `relations` load on demand.
struct Selection<'g> {
    graph: &'g Graph,
    reach: &'g BTreeMap<u32, BTreeSet<u32>>,
    relations: BTreeMap<u32, u32>,
    grouped: BTreeSet<u32>,
    resident: BTreeSet<u32>,
    pending: Vec<u32>,
}

impl Selection<'_> {
    /// Keeps `relation` and its group resident, as every relation was before.
    fn keep(&mut self, relation: u32) {
        let Some(companion) = self.relations.remove(&relation) else {
            return;
        };
        for tag in [relation, companion]
            .into_iter()
            .chain(self.reach[&relation].iter().copied())
        {
            if self.resident.insert(tag) {
                self.pending.push(tag);
            }
        }
    }

    /// Makes resident every grouped entry a resident entry names, until none is left.
    fn close(&mut self) {
        while let Some(tag) = self.pending.pop() {
            for &named in self.graph.names(tag) {
                if !self.relations.contains_key(&named)
                    && self.grouped.contains(&named)
                    && self.resident.insert(named)
                {
                    self.pending.push(named);
                }
            }
        }
    }

    fn deferred(&self) -> BTreeSet<u32> {
        self.grouped
            .iter()
            .copied()
            .filter(|tag| !self.resident.contains(tag))
            .collect()
    }

    fn settle(&mut self) {
        loop {
            self.close();
            // Only a resident entry asks for a relation.
            let asked = self
                .resident
                .iter()
                .flat_map(|&tag| self.graph.names(tag))
                .copied()
                .filter(|tag| self.relations.contains_key(tag))
                .collect::<BTreeSet<_>>();
            let mut unasked = self
                .relations
                .keys()
                .copied()
                .filter(|relation| !asked.contains(relation))
                .collect::<Vec<_>>();
            if unasked.is_empty() {
                let registered = self
                    .resident
                    .iter()
                    .filter(|tag| !matches!(self.graph.entries[tag].place, Place::PrivatePerk(_)))
                    .map(|tag| TagHash(*tag).pkg_id())
                    .collect::<BTreeSet<_>>();
                let unregistered = self
                    .deferred()
                    .into_iter()
                    .map(|tag| TagHash(tag).pkg_id())
                    .filter(|package| !registered.contains(package))
                    .collect::<BTreeSet<_>>();
                unasked = self
                    .relations
                    .iter()
                    .filter(|(relation, companion)| {
                        [**relation, **companion]
                            .iter()
                            .chain(&self.reach[*relation])
                            .any(|tag| unregistered.contains(&TagHash(*tag).pkg_id()))
                    })
                    .map(|(relation, _)| *relation)
                    .collect();
            }
            if unasked.is_empty() {
                return;
            }
            for relation in unasked {
                self.keep(relation);
            }
        }
    }
}

impl Payloads {
    fn payload(&self, place: Place) -> &[u8] {
        match place {
            Place::Weapon(index) => &self.weapon_tags[index].payload,
            Place::Asset(package, index) => {
                &self.asset_packages.packages[package].tags[index].payload
            }
            Place::PrivatePerk(index) => &self.private_perk_tags[index].payload,
        }
    }

    /// Moves what only private relations reach into their loading groups, and records the
    /// entries that leave the investment index for [`Payloads::dependencies`] and the record
    /// packages.
    pub(in crate::item::build) fn defer_relation_groups(
        &mut self,
        manager: &PackageManager,
        timings: &mut DependencyTimings,
    ) -> AuthoringResult<()> {
        let started = std::time::Instant::now();
        let graph = Graph::read(self, manager)?;
        let relations = graph.relations(self)?;
        let reach = relations
            .keys()
            .map(|&relation| (relation, graph.reach(relation)))
            .collect::<BTreeMap<_, _>>();
        let mut grouped = BTreeSet::new();
        for (relation, companion) in &relations {
            grouped.insert(*relation);
            grouped.insert(*companion);
            grouped.extend(reach[relation].iter().copied());
        }
        let resident: BTreeSet<u32> = graph
            .entries
            .keys()
            .copied()
            .filter(|tag| !grouped.contains(tag))
            .collect();
        let mut selection = Selection {
            graph: &graph,
            reach: &reach,
            relations,
            grouped,
            pending: resident.iter().copied().collect(),
            resident,
        };
        selection.settle();
        let deferred = selection.deferred();
        // Each relation's companion lists what its group now carries.
        for (relation, companion) in &selection.relations {
            let members = reach[relation]
                .iter()
                .copied()
                .filter(|tag| deferred.contains(tag))
                .map(TagHash)
                .collect::<Vec<_>>();
            if members.is_empty() {
                continue;
            }
            let Place::Weapon(index) = graph.entries[companion].place else {
                return Err(invalid("On-demand loading: a private companion moved"));
            };
            self.weapon_tags[index].payload =
                crate::shared_tag_dependency_index::enroll_dependencies(
                    &self.weapon_tags[index].payload,
                    TagHash(*companion),
                    TagHash(*relation),
                    &members,
                )?;
        }
        timings.relations = selection.relations.len();
        timings.on_demand_entries = deferred.len();
        timings.on_demand_bytes = deferred.iter().map(|tag| graph.entries[tag].size).sum();
        self.on_demand = deferred.into_iter().map(TagHash).collect();
        timings.on_demand = started.elapsed();
        Ok(())
    }
}
