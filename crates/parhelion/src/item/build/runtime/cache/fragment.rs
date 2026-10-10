//! Decode, plan and relocate on candidate state, then commit the complete fragment once.
use super::*;
use crate::asset_packages::GroupPlan;

fn pack(
    manager: &PackageManager,
    tags: &[NewTagSpec],
    allocator: AppendedTagAllocator,
    start: usize,
    bytes: &mut Vec<u8>,
) -> Option<Vec<Tag>> {
    tags.iter()
        .enumerate()
        .map(|(index, tag)| {
            Some(Tag {
                assigned: allocator
                    .assigned_tag(
                        start.checked_add(index)?,
                        "Cached runtime",
                        "runtime resource",
                    )
                    .ok()?
                    .0,
                template: tag.template_tag.0,
                storage: match tag.storage {
                    crate::NewTagStorageMode::InheritTemplate => 0,
                    crate::NewTagStorageMode::AudioMedia => 1,
                    crate::NewTagStorageMode::AudioBank => 2,
                },
                fields: relocate::fields(manager, tag),
                span: append(bytes, &tag.payload),
            })
        })
        .collect()
}

fn package_id(index: usize) -> Option<u16> {
    crate::package_profile::PARHELION_ASSET_PACKAGE_ID
        .checked_add(u16::try_from(index).ok()?)
        .filter(|id| *id <= crate::package_profile::MAX_AUTHORED_STANDALONE_PACKAGE_ID)
}

/// Records where each tag of `group` moves under `plan`, after checking each is where its cached
/// reservation put it.
fn group_moves(group: &Group, plan: &GroupPlan, moves: &mut BTreeMap<u32, u32>) -> Option<()> {
    let old = AppendedTagAllocator::new(package_id(group.reservation.package_index)?, 0);
    let new = AppendedTagAllocator::new(plan.package_id, 0);
    for (index, tag) in group.tags.iter().enumerate() {
        let cached = old
            .assigned_tag(
                group.reservation.base.checked_add(index)?,
                "Cached assets",
                "asset",
            )
            .ok()?;
        if cached.0 != tag.assigned {
            return None;
        }
        let assigned = new
            .assigned_tag(plan.base.checked_add(index)?, "Cached assets", "asset")
            .ok()?;
        if moves.insert(tag.assigned, assigned.0).is_some() {
            return None;
        }
    }
    Some(())
}

/// One cached asset group's tags unpacked at their new places, with its references between its
/// own tags moved with them. `None` when a reference leaves the group or repeats a tag.
fn restore_group(
    group: &Group,
    plan: &GroupPlan,
    bytes: &[u8],
    (moves, exact): (&BTreeMap<u32, u32>, bool),
) -> Option<(Vec<NewTagSpec>, Vec<crate::NewTagReferenceOverride>)> {
    let tags = group
        .tags
        .iter()
        .map(|tag| relocate::unpack(tag, bytes, moves, exact))
        .collect::<Option<Vec<_>>>()?;
    let mut references = Vec::new();
    let mut seen = BTreeSet::new();
    for (ordinal, target) in &group.references {
        let local = ordinal.checked_sub(group.reservation.base)?;
        if local >= tags.len() || !seen.insert(local) {
            return None;
        }
        let reference = match target {
            None => crate::NewTagReference::Template,
            Some(target) => {
                let old =
                    AppendedTagAllocator::new(package_id(group.reservation.package_index)?, 0)
                        .assigned_tag(*target, "Cached assets", "entry reference")
                        .ok()?;
                let new = TagHash(moves.get(&old.0).copied().unwrap_or(old.0));
                if new.pkg_id() != plan.package_id
                    || usize::from(new.entry_index()) >= plan.base + tags.len()
                {
                    return None;
                }
                crate::NewTagReference::Appended(usize::from(new.entry_index()))
            }
        };
        references.push(crate::NewTagReferenceOverride {
            new_tag_ordinal: plan.base + local,
            reference,
        });
    }
    Some((tags, references))
}

/// The animation assets the fragment added, keyed by their content at their new tags. A key the
/// build already holds means a newly shared asset, which changes deduplication and group sizes,
/// so that case recompiles.
fn restore_animation(
    manager: &PackageManager,
    cached: &[((u32, Hash), u32)],
    (payloads, animation, moves): (
        &BTreeMap<u32, &NewTagSpec>,
        &AnimationCache,
        &BTreeMap<u32, u32>,
    ),
    reason: &mut Reason,
) -> Option<AnimationCache> {
    let mut restored = AnimationCache::new();
    for ((class, _), old) in cached {
        let tag = payloads.get(old)?;
        if manager.get_entry(tag.template_tag)?.reference != *class {
            return None;
        }
        let key = (*class, hash(&tag.payload));
        if animation.contains_key(&key) {
            *reason = Reason::SharedAnimationChanged;
            return None;
        }
        if restored
            .insert(key, moves.get(old).copied().unwrap_or(*old))
            .is_some()
        {
            return None;
        }
    }
    Some(restored)
}

impl Pending<'_> {
    pub(in crate::item::build::runtime) fn restore(
        &self,
        manager: &PackageManager,
        host: &mut Vec<NewTagSpec>,
        assets: &mut RuntimeAssets<'_>,
        assignments: &mut Vec<u8>,
        animation: &mut AnimationCache,
    ) -> Result<Restored, Reason> {
        let started = std::time::Instant::now();
        let loaded = self.read();
        self.timings.borrow_mut().load += started.elapsed();
        let (entry, bytes) = loaded?;
        let started = std::time::Instant::now();
        let checked = self.check_inputs(manager, &entry);
        self.timings.borrow_mut().inputs += started.elapsed();
        checked?;
        if entry.key != self.key {
            return Err(if entry.identity.is_some() {
                Reason::RecipeChanged
            } else {
                Reason::IdentityChanged
            });
        }
        // A key names one kind of work, so a fragment of another kind under it is damaged.
        if entry.result.is_some() != self.returns {
            return Err(Reason::Damaged(
                "the fragment holds another kind of work".into(),
            ));
        }
        let exact = entry.state == self.state;
        if !exact && !entry.relocatable {
            return Err(Reason::UnsupportedRelocation);
        }
        let started = std::time::Instant::now();
        let restored = self.replay(
            manager,
            entry,
            &bytes,
            exact,
            host,
            assets,
            assignments,
            animation,
        );
        self.timings.borrow_mut().relocation += started.elapsed();
        restored
    }

    fn read(&self) -> Result<(Entry, Vec<u8>), Reason> {
        let file = File::open(&self.path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Reason::Missing
            } else {
                Reason::Damaged(error.to_string())
            }
        })?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|error| Reason::Damaged(error.to_string()))?;
        let bytes = read_member(&mut archive, "entry.json", 8 * 1024 * 1024)
            .ok_or_else(|| Reason::Damaged("missing, oversized or unreadable entry.json".into()))?;
        let entry: Entry =
            serde_json::from_slice(&bytes).map_err(|error| Reason::Damaged(error.to_string()))?;
        if let (Some(previous), Some(current)) = (entry.identity, self.identity) {
            if previous.compiler != current.compiler {
                return Err(Reason::CompilerChanged);
            }
            if previous.source != current.source {
                return Err(Reason::SourceChanged);
            }
        }
        let bytes = read_member(&mut archive, "payload.bin", ENTRY_LIMIT).ok_or_else(|| {
            Reason::Damaged("missing, oversized or unreadable payload.bin".into())
        })?;
        if hash(&bytes) != entry.payload_hash {
            return Err(Reason::Damaged("payload checksum differs".into()));
        }
        Ok((entry, bytes))
    }

    fn check_inputs(&self, manager: &PackageManager, entry: &Entry) -> Result<(), Reason> {
        #[cfg(feature = "d2-model-importer")]
        if !entry.inputs.is_empty() {
            if !entry.inputs.contains_key(Path::new("asset-graph.json")) {
                return Err(Reason::Damaged(
                    "import manifest fingerprint missing".into(),
                ));
            }
            let root = self
                .import_root
                .as_ref()
                .ok_or_else(|| Reason::ImportUnavailable(PathBuf::from("asset-graph.json")))?;
            let root =
                fs::canonicalize(root).map_err(|_| Reason::ImportUnavailable(root.clone()))?;
            for (name, expected) in &entry.inputs {
                let path = custom_runtime::imports::input_path(&root, name)
                    .ok_or_else(|| Reason::ImportUnavailable(name.clone()))?;
                let actual =
                    file_hash(&path).ok_or_else(|| Reason::ImportUnavailable(name.clone()))?;
                if actual != *expected {
                    return Err(Reason::ImportChanged(name.clone()));
                }
            }
        }
        for (tag, expected) in &entry.reads {
            let known = self.verified.borrow().get(tag).copied();
            let actual = match known {
                Some(digest) => digest,
                None => {
                    let payload = manager
                        .read_tag(TagHash(*tag))
                        .map_err(|_| Reason::NativeUnavailable(*tag))?;
                    let digest = hash(&payload);
                    self.verified.borrow_mut().insert(*tag, digest);
                    digest
                }
            };
            if actual != *expected {
                return Err(Reason::NativeChanged(*tag));
            }
        }
        Ok(())
    }

    /// Where each of the fragment's tags moves: its host tags to this build's host places, each
    /// asset group to a place appended to the build's asset packages, and each shared animation it
    /// depends on to the tag this build already holds for it.
    fn moves(
        &self,
        entry: &Entry,
        assets: &RuntimeAssets<'_>,
        animation: &AnimationCache,
        reason: &mut Reason,
    ) -> Option<(BTreeMap<u32, u32>, Vec<GroupPlan>)> {
        let mut moves = BTreeMap::new();
        for (index, tag) in entry.host.iter().enumerate() {
            let assigned = self
                .allocator
                .assigned_tag(
                    self.host_start.checked_add(index)?,
                    "Cached runtime",
                    "runtime resource",
                )
                .ok()?;
            if moves.insert(tag.assigned, assigned.0).is_some() {
                return None;
            }
        }
        let mut layout = assets.packages.layout().ok()?;
        let mut plans = Vec::<GroupPlan>::new();
        for group in &entry.groups {
            let plan = layout
                .append(
                    &group.reservation.bounds,
                    group.tags.iter().map(|tag| tag.span[1]),
                )
                .ok()?;
            group_moves(group, &plan, &mut moves)?;
            plans.push(plan);
        }
        for (key, old) in &entry.dependencies {
            let Some(current) = animation.get(key) else {
                *reason = Reason::SharedAnimationChanged;
                return None;
            };
            if moves.insert(*old, *current).is_some() {
                return None;
            }
        }
        Some((moves, plans))
    }

    #[allow(clippy::too_many_arguments)]
    fn replay(
        &self,
        manager: &PackageManager,
        entry: Entry,
        bytes: &[u8],
        exact: bool,
        host: &mut Vec<NewTagSpec>,
        assets: &mut RuntimeAssets<'_>,
        assignments: &mut Vec<u8>,
        animation: &mut AnimationCache,
    ) -> Result<Restored, Reason> {
        let mut reason = Reason::RelocationRejected;
        let restored = (|| {
            // No live state changes until all allocations, references and assignment rows agree.
            let (moves, plans) = self.moves(&entry, assets, animation, &mut reason)?;
            if exact && moves.iter().any(|(old, new)| old != new) {
                return None;
            }
            let moved = |tag: u32| moves.get(&tag).copied().unwrap_or(tag);
            let restored_host = entry
                .host
                .iter()
                .map(|tag| relocate::unpack(tag, bytes, &moves, exact))
                .collect::<Option<Vec<_>>>()?;
            let restored_groups = entry
                .groups
                .iter()
                .zip(&plans)
                .map(|(group, plan)| restore_group(group, plan, bytes, (&moves, exact)))
                .collect::<Option<Vec<_>>>()?;
            let restored_assignments = match entry.assignment {
                Some((pattern, tag)) => {
                    append_weapon_entity_assignment(assignments.clone(), pattern, moved(tag))
                        .ok()?
                }
                None => assignments.clone(),
            };
            let payloads = entry
                .host
                .iter()
                .zip(&restored_host)
                .chain(
                    entry
                        .groups
                        .iter()
                        .zip(&restored_groups)
                        .flat_map(|(group, (tags, _))| group.tags.iter().zip(tags)),
                )
                .map(|(record, tag)| (record.assigned, tag))
                .collect::<BTreeMap<_, _>>();
            let restored_animation = restore_animation(
                manager,
                &entry.animation,
                (&payloads, animation, &moves),
                &mut reason,
            )?;
            let placed = entry
                .placed
                .iter()
                .copied()
                .map(|tag| TagHash(moved(tag)))
                .collect::<Vec<_>>();
            let impacts = entry
                .impacts
                .into_iter()
                .map(|mut impact| {
                    impact.responses = impact.responses.map(moved);
                    impact
                })
                .collect::<Vec<_>>();
            let result = entry.result.map(|tag| TagHash(moved(tag)));
            host.extend(restored_host);
            *assignments = restored_assignments;
            for (plan, (tags, references)) in plans.into_iter().zip(restored_groups) {
                assets.packages.reservations.push(plan.reservation());
                if plan.package_index == assets.packages.packages.len() {
                    assets.packages.packages.push(AssetPackage {
                        id: plan.package_id,
                        tags: Vec::new(),
                        references: Vec::new(),
                    });
                }
                let package = &mut assets.packages.packages[plan.package_index];
                package.tags.extend(tags);
                package.references.extend(references);
            }
            assets.placed.extend(placed);
            assets.impacts.extend(impacts);
            animation.extend(restored_animation);
            if let Ok(file) = File::options().write(true).open(&self.path) {
                let _ = file.set_modified(std::time::SystemTime::now());
            }
            Some(Restored {
                pattern: entry.assignment.map(|(pattern, _)| pattern),
                result,
            })
        })();
        restored.ok_or(reason)
    }

    pub(in crate::item::build::runtime) fn timings(&self) -> String {
        self.timings.borrow().to_string()
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::item::build::runtime) fn save(
        &self,
        manager: &PackageManager,
        mut reads: BTreeMap<u32, Hash>,
        (pattern, result): (Option<u32>, Option<TagHash>),
        host: &[NewTagSpec],
        assets: &RuntimeAssets<'_>,
        assignments: &[u8],
        animation: &AnimationCache,
        #[cfg(feature = "d2-model-importer")] inputs: Option<custom_runtime::imports::Fingerprints>,
    ) -> Result<(), Reason> {
        #[cfg(feature = "d2-model-importer")]
        let inputs = inputs.ok_or(Reason::UncacheableImport)?;
        let mut reason = Reason::UnsupportedFragment;
        let result = result.map(|tag| tag.0);
        let captured = (|| {
            if result.is_some() != self.returns {
                return None;
            }
            let assignment = match pattern {
                Some(pattern) => Some((
                    pattern,
                    weapon_entity_assignment(assignments, pattern).ok()??,
                )),
                None => None,
            };
            let expected = match assignment {
                Some((pattern, tag)) => {
                    append_weapon_entity_assignment(self.assignments.clone(), pattern, tag).ok()?
                }
                None => self.assignments.clone(),
            };
            if expected != assignments {
                return None;
            }
            let size = self.size(host, assets)?;
            if size as u64 > ENTRY_LIMIT {
                reason = Reason::TooLarge;
                return None;
            }
            // Generated schema reads made while locating relocation fields are dependencies too.
            let Some(trace) = manager.trace_reads() else {
                reason = Reason::UntrackedReads;
                return None;
            };
            let mut bytes = Vec::with_capacity(size);
            let host = pack(
                manager,
                host.get(self.host_start..)?,
                self.allocator,
                self.host_start,
                &mut bytes,
            )?;
            let groups = self.pack_groups(manager, assets, &mut bytes)?;
            let placed = assets
                .placed
                .get(self.placed_start..)?
                .iter()
                .map(|tag| tag.0)
                .collect::<Vec<_>>();
            let impacts = assets.impacts.get(self.impact_start..)?.to_vec();
            let all = host
                .iter()
                .chain(groups.iter().flat_map(|group| &group.tags))
                .collect::<Vec<_>>();
            let owned = all.iter().map(|tag| tag.assigned).collect::<BTreeSet<_>>();
            if owned.len() != all.len() {
                return None;
            }
            let previous = self
                .animation
                .iter()
                .map(|(key, tag)| (*tag, *key))
                .collect::<BTreeMap<_, _>>();
            let mut dependencies = BTreeMap::new();
            let mut relocatable = all.iter().all(|tag| tag.fields.is_some());
            let entry_references = groups
                .iter()
                .flat_map(|group| {
                    group.references.iter().filter_map(move |(_, target)| {
                        target.map(|ordinal| (group.reservation.package_index, ordinal))
                    })
                })
                .map(|(index, ordinal)| {
                    AppendedTagAllocator::new(package_id(index)?, 0)
                        .assigned_tag(ordinal, "Cached asset", "entry reference")
                        .ok()
                        .map(|tag| tag.0)
                })
                .collect::<Option<Vec<_>>>()?;
            for reference in all
                .iter()
                .flat_map(|tag| tag.fields.iter().flatten().map(|(_, tag)| *tag))
                .chain(placed.iter().copied())
                .chain(assignment.map(|(_, tag)| tag))
                .chain(result)
                .chain(impacts.iter().filter_map(|impact| impact.responses))
                .chain(entry_references)
            {
                if owned.contains(&reference) {
                    continue;
                }
                if let Some(key) = previous.get(&reference) {
                    dependencies.insert(*key, reference);
                } else if manager.get_entry(TagHash(reference)).is_none() {
                    relocatable = false;
                }
            }
            let new_animation = animation
                .iter()
                .filter(|(key, value)| self.animation.get(key) != Some(value))
                .map(|(key, value)| (*key, *value))
                .collect::<Vec<_>>();
            if new_animation.iter().any(|(_, tag)| !owned.contains(tag)) {
                return None;
            }
            match trace.finish() {
                Some(extra) => reads.extend(extra),
                // An optional schema probe failing must not remove exact-allocation reuse.
                None => relocatable = false,
            }
            self.verified
                .borrow_mut()
                .extend(reads.iter().map(|(tag, digest)| (*tag, *digest)));
            let entry = Entry {
                identity: self.identity,
                key: self.key,
                state: self.state,
                payload_hash: hash(&bytes),
                reads,
                assignment,
                host,
                groups,
                placed,
                impacts,
                animation: new_animation,
                dependencies: dependencies.into_iter().collect(),
                relocatable,
                result,
                #[cfg(feature = "d2-model-importer")]
                inputs,
            };
            Some((entry, bytes))
        })();
        let (entry, bytes) = captured.ok_or(reason)?;
        self.write(&entry, &bytes).map_err(Reason::Write)
    }

    /// The bytes the work added: its host tags and its tags in every asset package.
    fn size(&self, host: &[NewTagSpec], assets: &RuntimeAssets<'_>) -> Option<usize> {
        let host = host
            .get(self.host_start..)?
            .iter()
            .map(|tag| tag.payload.len())
            .sum::<usize>();
        let packages = assets
            .packages
            .packages
            .iter()
            .enumerate()
            .map(|(i, package)| {
                package
                    .tags
                    .get(self.asset_starts.get(i).copied().unwrap_or(0)..)
                    .map(|tags| tags.iter().map(|tag| tag.payload.len()).sum::<usize>())
            })
            .collect::<Option<Vec<_>>>()?;
        Some(host + packages.into_iter().sum::<usize>())
    }

    /// Each asset group the work reserved, packed with its references between its own tags.
    /// `None` unless the groups cover exactly the tags the work appended to each package.
    fn pack_groups(
        &self,
        manager: &PackageManager,
        assets: &RuntimeAssets<'_>,
        bytes: &mut Vec<u8>,
    ) -> Option<Vec<Group>> {
        let reservations = assets.packages.reservations.get(self.group_start..)?;
        let mut positions = self.asset_starts.clone();
        positions.resize(assets.packages.packages.len(), 0);
        let mut groups = Vec::new();
        for (index, reservation) in reservations.iter().enumerate() {
            let package = assets.packages.packages.get(reservation.package_index)?;
            if reservation.base != *positions.get(reservation.package_index)?
                || package.id != package_id(reservation.package_index)?
            {
                return None;
            }
            let end = reservations
                .get(index + 1)
                .filter(|next| next.package_index == reservation.package_index)
                .map_or(package.tags.len(), |next| next.base);
            let tags = pack(
                manager,
                package.tags.get(reservation.base..end)?,
                AppendedTagAllocator::new(package.id, 0),
                reservation.base,
                bytes,
            )?;
            groups.push(Group {
                reservation: reservation.clone(),
                tags,
                references: package
                    .references
                    .iter()
                    .filter(|reference| {
                        (reservation.base..end).contains(&reference.new_tag_ordinal)
                    })
                    .map(|reference| {
                        (
                            reference.new_tag_ordinal,
                            match reference.reference {
                                crate::NewTagReference::Template => None,
                                crate::NewTagReference::Appended(ordinal) => Some(ordinal),
                            },
                        )
                    })
                    .collect(),
            });
            positions[reservation.package_index] = end;
        }
        positions
            .iter()
            .zip(&assets.packages.packages)
            .all(|(end, package)| *end == package.tags.len())
            .then_some(groups)
    }

    fn write(&self, entry: &Entry, bytes: &[u8]) -> Result<(), String> {
        let mut temporary = tempfile::Builder::new()
            .prefix(".runtime-")
            .suffix(".tmp")
            .tempfile_in(self.path.parent().ok_or("Cache path has no parent")?)
            .map_err(|error| error.to_string())?;
        {
            let mut archive = zip::ZipWriter::new(temporary.as_file_mut());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .compression_level(Some(1));
            archive
                .start_file("entry.json", options)
                .map_err(|error| error.to_string())?;
            archive
                .write_all(&serde_json::to_vec(entry).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
            archive
                .start_file("payload.bin", options)
                .map_err(|error| error.to_string())?;
            archive
                .write_all(bytes)
                .map_err(|error| error.to_string())?;
            archive.finish().map_err(|error| error.to_string())?;
        }
        temporary.flush().map_err(|error| error.to_string())?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        temporary
            .persist(&self.path)
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}
