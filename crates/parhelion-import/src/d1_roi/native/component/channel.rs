//! Retain inherited inputs, zero values, local expressions and checked stateful callbacks.
use super::*;
mod callback;
mod incoming;
pub(super) mod program;

struct Bank {
    names: Vec<u32>,
    /// Parent lookup namespace, parent channel name and local ordinal, sorted by namespace
    /// then name as the native lookup is.
    inherited: Vec<(u32, u32, usize)>,
    /// First and last default-namespace rows of `inherited`.
    span: [u16; 2],
    constants: BTreeMap<usize, [u8; 16]>,
    callbacks: BTreeMap<usize, callback::Callback>,
    programs: BTreeMap<usize, program::Program>,
    inputs: Vec<incoming::Input>,
    /// Incoming channels that the public lookup also names. Other components read only public
    /// names, and an incoming channel may stay private to its bank.
    public: BTreeSet<usize>,
    dependencies: Vec<Vec<u8>>,
    history: Vec<u8>,
    /// Native carrier declarations appended after the source's own, by name.
    retained: Vec<u32>,
    /// Nonzero declared value types at `+4A` of plain rows, those without code or a callback.
    /// Procedure rows keep theirs in their program.
    types: BTreeMap<usize, u16>,
}

impl Bank {
    /// Local channels, constants and callbacks, by name as the native local lookup sorts them.
    fn locals(&self) -> Vec<(u32, u32, usize)> {
        let mut locals = self
            .constants
            .keys()
            .chain(self.callbacks.keys())
            .chain(self.programs.keys())
            .chain(self.public.iter())
            .map(|&i| (0x811C9DC5, self.names[i], i))
            .collect::<Vec<_>>();
        locals.sort_unstable();
        locals
    }

    /// Append a native carrier's declarations that the source lacks, keeping their native
    /// producers, so the carrier's own consumers still find every name they read. Shared names
    /// keep their source producers, which must be of the same kind apart from the `crossed`
    /// names the caller has qualified, and invalidate the union of what each bank invalidates by
    /// name.
    #[allow(clippy::cognitive_complexity)]
    fn retain(
        &mut self,
        carrier: &Payload,
        omit: &BTreeSet<u32>,
        crossed: &BTreeSet<u32>,
        extra: &[(u32, u32, usize)],
    ) -> Result<()> {
        let schema = carrier.pointer(24)?;
        ensure!(
            carrier.u64(carrier.pointer(16)? + 0x70)? == 0,
            "Native carrier bank has callback history"
        );
        let rows = carrier.array(schema + 0xD8, 112, Some(0x808097A1))?;
        let names = rows
            .iter()
            .map(|at| carrier.u32(*at))
            .collect::<Result<Vec<_>>>()?;
        let lookup = |field| -> Result<Vec<(u32, u32, usize)>> {
            carrier
                .array(schema + field, 12, None)?
                .into_iter()
                .map(|at| {
                    let ordinal = usize::try_from(carrier.u32(at + 8)?)?;
                    ensure!(
                        ordinal < names.len(),
                        "Native carrier lookup addresses no declaration"
                    );
                    Ok((carrier.u32(at)?, carrier.u32(at + 4)?, ordinal))
                })
                .collect()
        };
        let (public, inherited, incoming) = (lookup(0xF8)?, lookup(0x108)?, lookup(0x118)?);
        let getters = carrier.array(schema + 0xC8, 32, Some(0x808097A8))?;
        let carrier_owner = carrier.u32(carrier.pointer(16)?)?;
        let source_count = self.names.len();
        // Carrier ordinal to merged ordinal, and whether the source declares the same name.
        // An omitted carrier declaration takes no ordinal. Nothing kept may depend on it.
        let mut ordinals = Vec::with_capacity(names.len());
        for &name in &names {
            if omit.contains(&name) {
                ensure!(
                    !self.names.contains(&name),
                    "An omitted carrier channel is also the source's"
                );
                ordinals.push((usize::MAX, false));
                continue;
            }
            ordinals.push(match self.names.iter().position(|&v| v == name) {
                Some(ordinal) => (ordinal, true),
                None => {
                    self.names.push(name);
                    self.retained.push(name);
                    (self.names.len() - 1, false)
                }
            });
        }
        // A name another carrier component publishes and neither bank declares becomes an
        // incoming channel that component's getter feeds.
        for &(name, _, _) in extra {
            ensure!(
                !self.names.contains(&name),
                "An added incoming channel is already declared"
            );
            self.names.push(name);
            self.retained.push(name);
        }
        ensure!(
            self.names.len() < usize::from(u16::MAX),
            "Retained channel count exceeds the bank capacity"
        );
        let mut dependents = vec![BTreeSet::new(); self.names.len()];
        for (ordinal, bytes) in self.dependencies.iter().enumerate() {
            for (word, chunk) in bytes.chunks_exact(4).enumerate() {
                let bits = u32::from_le_bytes(chunk.try_into()?);
                dependents[ordinal].extend(
                    (0..32)
                        .filter(|bit| bits & (1 << bit) != 0)
                        .map(|bit| word * 32 + bit),
                );
            }
        }
        for (index, &row) in rows.iter().enumerate() {
            let (ordinal, shared) = ordinals[index];
            if ordinal == usize::MAX {
                continue;
            }
            ensure!(
                carrier.u64(row + 96)? == 0 && carrier.bytes::<2>(row + 104)? == [255, 254],
                "Native carrier channel has callbacks or interpolation"
            );
            for (word, at) in carrier
                .array(row + 80, 4, Some(0x8080000B))?
                .into_iter()
                .enumerate()
            {
                let bits = carrier.u32(at)?;
                for bit in (0..32).filter(|bit| bits & (1 << bit) != 0) {
                    let (dependent, _) = *ordinals
                        .get(word * 32 + bit)
                        .context("Native dependency exceeds channel declarations")?;
                    ensure!(
                        dependent != usize::MAX,
                        "An omitted carrier channel has dependents"
                    );
                    dependents[ordinal].insert(dependent);
                }
            }
            let parents = inherited
                .iter()
                .filter(|entry| entry.2 == index)
                .map(|entry| (entry.0, entry.1))
                .collect::<Vec<_>>();
            let input = incoming.iter().any(|entry| entry.2 == index);
            let local = carrier.u64(row + 8)? != 0;
            if shared {
                let source_parents = self
                    .inherited
                    .iter()
                    .filter(|entry| entry.2 == ordinal)
                    .map(|entry| (entry.0, entry.1))
                    .collect::<Vec<_>>();
                let source_input = self.inputs.iter().any(|i| i.ordinal == ordinal);
                let source_local = self.constants.contains_key(&ordinal)
                    || self.callbacks.contains_key(&ordinal)
                    || self.programs.contains_key(&ordinal);
                // A crossed name keeps its source producer, an input where the carrier inherits or
                // the reverse, as a replaced projectile receiver row moves it.
                let crossed = crossed.contains(&names[index])
                    && !local
                    && !source_local
                    && input != source_input
                    && parents.is_empty() == input
                    && source_parents.is_empty() == source_input;
                ensure!(
                    crossed
                        || (parents == source_parents
                            && input == source_input
                            && local == source_local),
                    "Shared channel {:08X} has another producer kind in the carrier",
                    names[index]
                );
                // A name the carrier publishes stays public for its native readers.
                if input && public.iter().any(|entry| entry.2 == index) {
                    self.public.insert(ordinal);
                }
                continue;
            }
            if input {
                // The getter is rebuilt from the native template like the source's. Its carrier
                // definition identifies the connection that feeds it, such as a named-parameter
                // context's creation value.
                let definition = *getters
                    .get(index)
                    .context("Native incoming getter is not its declaration's")?;
                let kind = carrier.u16(row + 74)?;
                ensure!(
                    !local
                        && parents.is_empty()
                        && carrier.0[row + 16..row + 72].iter().all(|v| *v == 0)
                        && kind <= 5
                        && carrier.u32(definition + 4)? == 0x808097A7
                        && carrier.u64(definition + 16)? == 0
                        && carrier.u64(definition + 24)? == 0x80809AE2,
                    "Native incoming channel has another producer or getter envelope"
                );
                self.inputs.push(incoming::Input {
                    ordinal,
                    name: names[index],
                    definition,
                    owner: carrier_owner,
                });
                if kind != 0 {
                    self.types.insert(ordinal, kind);
                }
                if public.iter().any(|entry| entry.2 == index) {
                    self.public.insert(ordinal);
                }
                continue;
            }
            if local {
                ensure!(
                    public
                        .iter()
                        .any(|entry| entry.2 == index && entry.0 == 0x811C9DC5),
                    "Native carrier procedure is not public"
                );
                let mut program = program::read(carrier, row, names.len())?;
                for input in &mut program.inputs {
                    *input = u32::try_from(ordinals[*input as usize].0)?;
                }
                self.programs.insert(ordinal, program);
            } else {
                let kind = carrier.u16(row + 74)?;
                ensure!(
                    carrier.0[row + 16..row + 72].iter().all(|v| *v == 0) && kind <= 5,
                    "Native carrier channel has unsupported defaults or forwarding"
                );
                if kind != 0 {
                    self.types.insert(ordinal, kind);
                }
                self.inherited.extend(
                    parents
                        .into_iter()
                        .map(|(namespace, parent)| (namespace, parent, ordinal)),
                );
            }
        }
        for &(name, owner, definition) in extra {
            let ordinal = self
                .names
                .iter()
                .position(|&value| value == name)
                .context("Added incoming channel")?;
            self.inputs.push(incoming::Input {
                ordinal,
                name,
                definition,
                owner,
            });
        }
        let dependents = self.lead_inputs(source_count, dependents)?;
        self.settle(dependents)
    }

    /// Native refresh treats declarations below the getter count as imported inputs, so the
    /// carrier's incoming channels move ahead of every derived declaration.
    fn lead_inputs(
        &mut self,
        source_count: usize,
        mut dependents: Vec<BTreeSet<usize>>,
    ) -> Result<Vec<BTreeSet<usize>>> {
        let inputs = self
            .inputs
            .iter()
            .map(|input| input.ordinal)
            .collect::<BTreeSet<_>>();
        let mut order = Vec::with_capacity(self.names.len());
        for leading in [true, false] {
            for range in [0..source_count, source_count..self.names.len()] {
                order.extend(range.filter(|ordinal| inputs.contains(ordinal) == leading));
            }
        }
        let mut place = vec![0; order.len()];
        for (new, &old) in order.iter().enumerate() {
            place[old] = new;
        }
        if order.iter().enumerate().any(|(new, &old)| new != old) {
            ensure!(
                self.callbacks.is_empty() && self.history.is_empty(),
                "Reordered channel bank has callbacks"
            );
            let names = order.iter().map(|&old| self.names[old]).collect();
            self.names = names;
            for entry in &mut self.inherited {
                entry.2 = place[entry.2];
            }
            self.constants = std::mem::take(&mut self.constants)
                .into_iter()
                .map(|(ordinal, value)| (place[ordinal], value))
                .collect();
            let mut programs = BTreeMap::new();
            for (ordinal, mut program) in std::mem::take(&mut self.programs) {
                for input in &mut program.inputs {
                    *input = u32::try_from(place[*input as usize])?;
                }
                programs.insert(place[ordinal], program);
            }
            self.programs = programs;
            for input in &mut self.inputs {
                input.ordinal = place[input.ordinal];
            }
            self.public = self.public.iter().map(|&ordinal| place[ordinal]).collect();
            self.types = std::mem::take(&mut self.types)
                .into_iter()
                .map(|(ordinal, kind)| (place[ordinal], kind))
                .collect();
            let mut moved = vec![BTreeSet::new(); dependents.len()];
            for (old, set) in dependents.into_iter().enumerate() {
                moved[place[old]] = set.into_iter().map(|ordinal| place[ordinal]).collect();
            }
            dependents = moved;
        }
        self.inputs.sort_by_key(|input| input.ordinal);
        Ok(dependents)
    }

    /// Sort the merged inherited lookup and write dependency masks over the merged ordinals.
    fn settle(&mut self, dependents: Vec<BTreeSet<usize>>) -> Result<()> {
        self.inherited.sort_unstable();
        ensure!(
            self.inherited
                .windows(2)
                .all(|pair| (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1)),
            "Retained inherited channel names collide"
        );
        self.span = crate::tiger::channel::inherited::default_span(
            self.inherited.iter().map(|(namespace, _, _)| *namespace),
        )?;
        let width = self.names.len().div_ceil(32);
        self.dependencies = dependents
            .into_iter()
            .map(|set| {
                if set.is_empty() {
                    return Vec::new();
                }
                let mut words = vec![0u32; width];
                for bit in set {
                    words[bit / 32] |= 1 << (bit % 32);
                }
                words.into_iter().flat_map(u32::to_le_bytes).collect()
            })
            .collect();
        program::validate_graph(&self.programs)
    }
}

impl Bank {
    fn read(source: &Payload) -> Result<Self> {
        let schema = source.pointer(24)?;
        let rows = source.array(schema + 0xD8, 112, Some(0x80800883))?;
        // 342 Shadowkeep stock banks hold more than 64 declarations, one 255, so 64 is not a
        // native capacity. Slots and lookup positions are UInt16 with FFFF as their sentinel.
        ensure!(
            !rows.is_empty() && rows.len() < usize::from(u16::MAX),
            "Source channel count differs"
        );
        let names = rows
            .iter()
            .map(|at| source.u32(*at))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            names.iter().copied().collect::<BTreeSet<_>>().len() == names.len(),
            "Duplicate source channel name"
        );
        let mut inherited = Vec::new();
        for (slot, at) in source
            .array(schema + 0x108, 12, None)?
            .into_iter()
            .enumerate()
        {
            let ordinal = usize::try_from(source.u32(at + 8)?)?;
            let row = *rows.get(ordinal).context("Source inherited declaration")?;
            // An empty namespace takes the parent channel of the local name. A named namespace
            // takes a parent channel under its own name, as Shadowkeep's rocket launcher bank
            // 8152A4F7 does for the same namespace 5F6317D5.
            let namespace = source.u32(at)?;
            let parent = source.u32(at + 4)?;
            ensure!(
                (if namespace == 0x811C9DC5 {
                    parent == names[ordinal]
                } else {
                    ![0, u32::MAX, 0x811C9DC5].contains(&parent)
                }) && source.u16(row + 72)? as usize == slot,
                "Source inherited channel lookup differs"
            );
            inherited.push((namespace, parent, ordinal));
        }
        // A bank may inherit nothing, as many source body and projectile banks do. Its span is
        // then FFFF, which native inherited lookup 00579880 answers at once with no match.
        ensure!(
            inherited
                .windows(2)
                .all(|pair| (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1)),
            "Source inherited names are unsorted"
        );
        let span = crate::tiger::channel::inherited::default_span(
            inherited.iter().map(|(namespace, _, _)| *namespace),
        )?;
        ensure!(
            [source.u16(schema + 0x128)?, source.u16(schema + 0x12A)?] == span,
            "Source default channel namespace span differs"
        );
        let inherited_ordinals = inherited
            .iter()
            .map(|(_, _, ordinal)| *ordinal)
            .collect::<BTreeSet<_>>();
        let inputs = incoming::read(source, &names)?;
        let input_ordinals = inputs
            .iter()
            .map(|input| input.ordinal)
            .collect::<BTreeSet<_>>();
        let mut constants = BTreeMap::new();
        let mut callbacks = BTreeMap::new();
        let mut programs = BTreeMap::new();
        let mut public = BTreeSet::new();
        let mut types = BTreeMap::new();
        let mut dependencies = Vec::new();
        for (ordinal, &row) in rows.iter().enumerate() {
            if let Some(found) = callback::read(source, row, rows.len())? {
                ensure!(
                    !inherited_ordinals.contains(&ordinal) && !input_ordinals.contains(&ordinal),
                    "Source callback overlaps an inherited or incoming channel"
                );
                callbacks.insert(ordinal, found);
                dependencies.push(Vec::new());
                continue;
            }
            ensure!(
                source.bytes::<2>(row + 104)? == [255, 254] && source.u64(row + 96)? == 0,
                "Source channel has unsupported callbacks or interpolation"
            );
            let code = source
                .array(row + 8, 1, Some(0x80800009))?
                .iter()
                .map(|at| source.u8(*at))
                .collect::<Result<Vec<_>>>()?;
            if code.is_empty() {
                // Only incoming channels have a public index here. Projectile bank `80AA2D90`
                // keeps its incoming `49FCE899` private, with no index and no lookup row.
                let indexed = source.u16(row + 106)? != u16::MAX;
                // The declared value type stays with the row. Shadowkeep's stock plain rows store
                // 0 to 5, and no native refresh, dispatch or interpreter path reads it.
                let kind = source.u16(row + 74)?;
                ensure!(
                    source.0[row + 16..row + 72].iter().all(|v| *v == 0)
                        && kind <= 5
                        && (!indexed || input_ordinals.contains(&ordinal)),
                    "Source channel has unsupported defaults or forwarding"
                );
                if kind != 0 {
                    types.insert(ordinal, kind);
                }
                if indexed {
                    public.insert(ordinal);
                }
                ensure!(
                    inherited_ordinals.contains(&ordinal) == (source.u16(row + 72)? != u16::MAX),
                    "Source channel inherited slot lacks a name"
                );
            } else {
                if code != [0x34, 0, 0x3E, 0]
                    || source.u64(row + 40)? != 0
                    || source.u64(row + 24)? != 1
                    || source.u16(row + 74)? != 1
                {
                    ensure!(
                        !inherited_ordinals.contains(&ordinal)
                            && !input_ordinals.contains(&ordinal),
                        "Source procedure overlaps an inherited or incoming channel"
                    );
                    programs.insert(ordinal, program::read(source, row, rows.len())?);
                    dependencies.push(Vec::new());
                    continue;
                }
                let values = source.array(row + 24, 16, Some(0x80800090))?;
                // D1 and native 8152B5B7 both use this zero-input vec4 return.
                // Other sequencers require their own instruction and state contract.
                ensure!(
                    code == [0x34, 0, 0x3E, 0]
                        && values.len() == 1
                        && source.u64(row + 40)? == 0
                        && source.u64(row + 48)? == 1
                        && source.u64(row + 56)? == 0
                        && source.u64(row + 64)? == 0
                        && source.u16(row + 72)? == u16::MAX
                        && source.u16(row + 74)? == 1,
                    "Source channel requires procedure translation"
                );
                let value = source.bytes::<16>(values[0])?;
                ensure!(
                    value
                        .chunks_exact(4)
                        .all(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()).is_finite()),
                    "Nonfinite source channel constant"
                );
                constants.insert(ordinal, value);
            }
            let words = source.array(row + 80, 4, Some(0x8080000B))?;
            ensure!(
                words.is_empty() || words.len() == rows.len().div_ceil(32),
                "Source channel dependency width differs"
            );
            if !inherited_ordinals.contains(&ordinal) && !input_ordinals.contains(&ordinal) {
                ensure!(
                    words.is_empty(),
                    "Local source channel has unsupported dependencies"
                );
            }
            let bytes = words
                .iter()
                .flat_map(|at| source.0[*at..*at + 4].iter().copied())
                .collect::<Vec<_>>();
            if let Some(&last) = words.last() {
                let tail = rows.len() % 32;
                ensure!(
                    tail == 0 || source.u32(last)? >> tail == 0,
                    "Source dependency exceeds channel declarations"
                );
            }
            dependencies.push(bytes);
        }
        program::validate_graph(&programs)?;
        let local = source.array(schema + 0xF8, 12, None)?;
        ensure!(
            local.len() == constants.len() + callbacks.len() + programs.len() + public.len(),
            "Source channel local names differ"
        );
        let mut local_ordinals = BTreeSet::new();
        let mut local_indices = BTreeSet::new();
        let count = local.len();
        for at in local {
            let ordinal = usize::try_from(source.u32(at + 8)?)?;
            let row = *rows.get(ordinal).context("Source local channel ordinal")?;
            // Each local row names a distinct place among the locals. Native rows name their
            // place in the sorted lookup, which the conversion writes afresh.
            let index = usize::from(source.u16(row + 106)?);
            ensure!(
                (constants.contains_key(&ordinal)
                    || callbacks.contains_key(&ordinal)
                    || programs.contains_key(&ordinal)
                    || public.contains(&ordinal))
                    && index < count
                    && local_indices.insert(index)
                    && local_ordinals.insert(ordinal)
                    && source.u32(at)? == 0x811C9DC5
                    && source.u32(at + 4)? == names[ordinal],
                "Source local channel lookup differs"
            );
        }
        let instance = source.pointer(16)?;
        ensure!(
            source.u64(instance + 0x80)? == 0 && source.u64(schema + 0xE8)? == 0,
            "Source channel has unsupported receivers"
        );
        let values = source.array(instance + 0x50, 16, Some(0x80800090))?;
        ensure!(
            values.len() == rows.len()
                && values
                    .iter()
                    .all(|at| source.0[*at..*at + 16].iter().all(|v| *v == 0)),
            "Source cached channel initialization differs"
        );
        let history = source.array_range(instance + 0x70, 16, Some(0x80800090))?;
        let history = source.0[history].to_vec();
        ensure!(
            history.len() <= 64 * 16 && history.iter().all(|v| *v == 0),
            "Source callback history is initialized or too large"
        );
        Ok(Self {
            names,
            inherited,
            span,
            constants,
            callbacks,
            programs,
            inputs,
            public,
            dependencies,
            history,
            retained: Vec::new(),
            types,
        })
    }
}

fn lookup(rows: &[(u32, u32, usize)]) -> Result<Vec<u8>> {
    rows.iter()
        .map(|&(namespace, name, ordinal)| {
            Ok([
                namespace.to_le_bytes(),
                name.to_le_bytes(),
                u32::try_from(ordinal)?.to_le_bytes(),
            ]
            .concat())
        })
        .collect::<Result<Vec<_>>>()
        .map(|rows| rows.concat())
}

impl Assembly<'_> {
    pub(in crate::d1_roi::native) fn channel_bank(
        &mut self,
        source: &Payload,
        symbol: &str,
        index: usize,
    ) -> Result<(Component, Vec<u32>)> {
        self.write_bank(Bank::read(source)?, source, symbol, index)
    }

    /// A source bank followed by the declarations only `carrier` has, for a native carrier
    /// whose own consumers stay connected to the bank by name. Declarations in `omit` are left
    /// out, as those only a left-out component supplies. Names in `crossed` keep source producers
    /// of another kind. Each `extra` name, owner and getter becomes an incoming channel, and the
    /// owner and getter key its emitted getter in `Component::incoming`.
    pub(in crate::d1_roi::native) fn retained_bank(
        &mut self,
        source: &Payload,
        (carrier, omit, crossed): (&Payload, &BTreeSet<u32>, &BTreeSet<u32>),
        extra: &[(u32, u32, usize)],
        symbol: &str,
        index: usize,
    ) -> Result<(Component, Vec<u32>)> {
        let mut bank = Bank::read(source)?;
        bank.retain(carrier, omit, crossed, extra)?;
        self.write_bank(bank, source, symbol, index)
    }

    #[allow(clippy::cognitive_complexity)]
    fn write_bank(
        &mut self,
        bank: Bank,
        source: &Payload,
        symbol: &str,
        index: usize,
    ) -> Result<(Component, Vec<u32>)> {
        let locals = bank.locals();
        let (template, entity) = if locals.is_empty() {
            (0x80F56FA1, 0x80BC358D)
        } else {
            (0x8152B5B7, 0x81523111)
        };
        let mut component = self.component(template, entity, symbol, index)?;
        let original = component.template.clone();
        let instance = original.pointer(16)?;
        let schema = original.pointer(24)?;
        let original_rows = original.array(schema + 0xD8, 112, Some(0x808097A1))?;
        let pending = original.array(instance + 0x40, 4, Some(0x8080000B))?;
        ensure!(
            pending.len() == original_rows.len().div_ceil(32)
                && pending.iter().all(|at| original.u32(*at).ok() == Some(0)),
            "Native pending channel initialization differs"
        );
        let mut constant_template = None;
        if !bank.constants.is_empty() {
            let candidates = original_rows
                .iter()
                .copied()
                .filter(|at| original.u64(*at + 8).ok() == Some(4))
                .collect::<Vec<_>>();
            ensure!(
                candidates.len() == 1,
                "Native constant channel template is ambiguous"
            );
            let row = candidates[0];
            let code = original
                .array(row + 8, 1, Some(0x80800009))?
                .iter()
                .map(|at| original.u8(*at))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                code == [0x34, 0, 0x3E, 0]
                    && original.u64(row + 40)? == 0
                    && original.u64(row + 48)? == 1
                    && original.u64(row + 56)? == 0
                    && original.u16(row + 72)? == u16::MAX
                    && original.u16(row + 74)? == 0
                    && original.u64(row + 80)? == 0
                    && original.u64(row + 96)? == 0
                    && original.bytes::<4>(row + 104)? == [255, 254, 0, 0],
                "Native constant channel contract differs"
            );
            constant_template = Some(original.bytes::<112>(row)?);
        }
        let p = &mut component.payload;
        let mut declarations = Vec::new();
        // A local row names its place in the name-sorted local lookup, as `8152A4F7` shows.
        let position = |ordinal: usize| -> Result<u16> {
            let index = locals
                .iter()
                .position(|(_, _, i)| *i == ordinal)
                .context("Local channel lookup position")?;
            Ok(u16::try_from(index)?)
        };
        for (ordinal, name) in bank.names.iter().enumerate() {
            let mut row = if bank.constants.contains_key(&ordinal) {
                let mut row = constant_template.context("Native local channel template")?;
                row[8..40].fill(0);
                // A source constant row always declares value type 1.
                put(&mut row, 74, &1u16.to_le_bytes())?;
                put(&mut row, 106, &position(ordinal)?.to_le_bytes())?;
                row
            } else if let Some(program) = bank.programs.get(&ordinal).or_else(|| {
                bank.callbacks
                    .get(&ordinal)
                    .map(|callback| &callback.program)
            }) {
                let mut row = [0; 112];
                put(&mut row, 40, &(program.inputs.len() as u64).to_le_bytes())?;
                put(&mut row, 48, &1u64.to_le_bytes())?;
                put(&mut row, 74, &program.flags.to_le_bytes())?;
                row[104..106].copy_from_slice(&[255, 254]);
                put(&mut row, 106, &position(ordinal)?.to_le_bytes())?;
                row
            } else {
                let mut row = [0; 112];
                let kind = bank.types.get(&ordinal).copied().unwrap_or(0);
                put(&mut row, 74, &kind.to_le_bytes())?;
                row[104..108].copy_from_slice(&[255, 254, 255, 255]);
                if bank.public.contains(&ordinal) {
                    put(&mut row, 106, &position(ordinal)?.to_le_bytes())?;
                }
                row
            };
            put(&mut row, 0, &name.to_le_bytes())?;
            let slot = bank
                .inherited
                .iter()
                .position(|(_, _, i)| *i == ordinal)
                .map(u16::try_from)
                .transpose()?
                .unwrap_or(u16::MAX);
            put(&mut row, 72, &slot.to_le_bytes())?;
            declarations.extend(row);
        }
        array(&mut p.0, schema + 0xD8, 0x808097A1, &declarations, 112)?;
        for (ordinal, row) in p
            .array(schema + 0xD8, 112, Some(0x808097A1))?
            .into_iter()
            .enumerate()
        {
            array(
                &mut p.0,
                row + 80,
                0x8080000B,
                &bank.dependencies[ordinal],
                4,
            )?;
            if let Some(value) = bank.constants.get(&ordinal) {
                array(&mut p.0, row + 8, 0x80800009, &[0x34, 0, 0x3E, 0], 1)?;
                array(&mut p.0, row + 24, 0x80800090, value, 16)?;
            }
            if let Some(callback) = bank.callbacks.get(&ordinal) {
                callback.write(p, row)?;
            }
            if let Some(program) = bank.programs.get(&ordinal) {
                program.write(p, row)?;
            }
        }
        array(
            &mut p.0,
            instance + 0x40,
            0x8080000B,
            &vec![0; bank.names.len().div_ceil(32) * 4],
            4,
        )?;
        array(
            &mut p.0,
            instance + 0x50,
            0x80800090,
            &vec![0; bank.names.len() * 16],
            16,
        )?;
        array(
            &mut p.0,
            instance + 0xA0,
            0x8080000B,
            &vec![0; bank.inherited.len().div_ceil(32) * 4],
            4,
        )?;
        array(
            &mut p.0,
            instance + 0x90,
            0x8080979C,
            &vec![255; bank.inherited.len() * 16],
            16,
        )?;
        array(
            &mut p.0,
            instance + 0x80,
            0x8080979D,
            &vec![255; locals.len() * 8],
            8,
        )?;
        array(
            &mut p.0,
            schema + 0x108,
            0x808097A0,
            &lookup(&bank.inherited)?,
            12,
        )?;
        array(&mut p.0, schema + 0xF8, 0x808097A0, &lookup(&locals)?, 12)?;
        let input_template = if bank.inputs.is_empty() {
            None
        } else {
            Some(self.native.tag(0x80FF0240, Some(0x80809C36))?)
        };
        if let Some(template) = &input_template {
            incoming::write(&bank.inputs, &mut component, template)?;
        }
        let p = &mut component.payload;
        array(&mut p.0, instance + 0x60, 0x80800090, &bank.history, 16)?;
        for (at, count) in [
            (0x134, bank.names.len()),
            (0x13C, locals.len()),
            (0x140, bank.inherited.len()),
        ] {
            put(&mut p.0, schema + at, &u32::try_from(count)?.to_le_bytes())?;
        }
        // Sight banks sort visibility's named namespace ahead of the default rows. A span
        // over every row hides default names such as ammunition `32CC5402` from the lookup.
        for (at, value) in [0x128, 0x12A].into_iter().zip(bank.span) {
            put(&mut p.0, schema + at, &value.to_le_bytes())?;
        }
        let tag = p.u32(0x44)?;
        let mut allocation = (*self.native.tag(tag, None)?).clone();
        let mut emptied = BTreeSet::new();
        for (name, class, before, after) in [
            (
                0x083C8E12,
                0x8080000B,
                original.u64(instance + 0xA0)? as usize,
                bank.inherited.len().div_ceil(32),
            ),
            (
                0x4A423762,
                0x8080000B,
                pending.len(),
                bank.names.len().div_ceil(32),
            ),
            (
                0xFC2F3D6F,
                0x80800090,
                original_rows.len(),
                bank.names.len(),
            ),
            (
                0x3A55A801,
                0x8080979C,
                original.u64(instance + 0x90)? as usize,
                bank.inherited.len(),
            ),
            (
                0xBAFA25AF,
                0x8080979D,
                original.u64(instance + 0x80)? as usize,
                locals.len(),
            ),
        ] {
            let matches = allocation
                .array(0x20, 40, Some(0x80808852))?
                .into_iter()
                .filter(|r| allocation.u32(*r).ok() == Some(name))
                .collect::<Vec<_>>();
            if before == 0 && after == 0 && matches.is_empty() {
                continue;
            }
            ensure!(matches.len() == 1, "Native channel allocation is ambiguous");
            let row = matches[0];
            ensure!(
                allocation.u32(row + 16)? == class && allocation.u32(row + 20)? as usize == before,
                "Native channel capacity differs"
            );
            put(
                &mut allocation.0,
                row + 20,
                &u32::try_from(after)?.to_le_bytes(),
            )?;
            if after == 0 {
                emptied.insert(name);
            }
        }
        // Stock banks without inherited names or locals have no allocation row for that storage,
        // such as 80B38443 and 80B4C2B4, so a member the merged bank no longer uses is removed
        // rather than kept with a zero count.
        if !emptied.is_empty() {
            let mut rows = Vec::new();
            for at in allocation.array(0x20, 40, Some(0x80808852))? {
                if !emptied.contains(&allocation.u32(at)?) {
                    rows.extend(allocation.bytes::<40>(at)?);
                }
            }
            array(&mut allocation.0, 0x20, 0x80808852, &rows, 40)?;
        }
        if let Some(template) = &input_template {
            let input_allocation = self.native.tag(template.u32(0x44)?, None)?;
            incoming::allocate(&mut allocation, &input_allocation, bank.inputs.len())?;
        }
        incoming::allocate_history(&mut allocation, bank.history.len() / 16)?;
        let allocation_symbol = format!("{symbol}-allocation");
        self.node(&allocation_symbol, tag, &allocation.0, None, &[])?;
        component.patch(0x44, &allocation_symbol)?;
        component.seal()?;
        self.evidence.push(json!({"source_channel_bank":source.u32(source.pointer(16)?)?,
            "declarations":bank.names.len(),"inherited":bank.inherited.len(),"local_constants":bank.constants.len(),
            "local_callbacks":bank.callbacks.len(),
            "callback_history_vectors":bank.history.len()/16,
            "adapted_springs":bank.callbacks.values().filter(|c| c.spring).count(),
            "spring_policy":"Source stiffness and damping become native frequency and damping ratio. Shadowkeep uses its analytic solver, not the source bounded integration steps.",
            "local_programs":bank.programs.len(),
            "incoming_values":bank.inputs.len(),
            "public_incoming":bank.public.len(),
            "retained_native":bank.retained,
            "unbound_zero":bank.names.len()-bank.inherited.len()-locals.len()-(bank.inputs.len()-bank.public.len())}));
        Ok((component, bank.names))
    }
}
