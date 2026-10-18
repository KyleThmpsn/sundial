//! Retain inherited inputs, unbound zero values, bounded local constants and kind-2 callback
//! channels.
use super::*;

/// Rise of Iron's callback class for kind 2 and Shadowkeep's. Native callback classes are
/// `808097B0` less the kind.
const SOURCE_KIND_2_CALLBACK: u32 = 0x8080_072D;
const NATIVE_KIND_2_CALLBACK: u32 = 0x8080_97AE;
/// The procedure a callback channel runs: read the callback's output, then return it.
const CALLBACK_CODE: [u8; 4] = [0x3C, 0, 0x3E, 0];

/// A local channel whose value a kind-2 callback produces from one input channel. The callback
/// object is kind 2, an unused `FFFFFFFF` word and two floats, the same 16 bytes in both
/// versions. Shadowkeep's rocket launcher bank `8152A4F7` holds rows of this exact shape.
struct Callback {
    input: u32,
    object: [u8; 16],
}

struct Bank {
    names: Vec<u32>,
    /// Parent lookup namespace, parent channel name and local ordinal, sorted by namespace
    /// then name as the native lookup is.
    inherited: Vec<(u32, u32, usize)>,
    constants: BTreeMap<usize, [u8; 16]>,
    callbacks: BTreeMap<usize, Callback>,
    dependencies: Vec<Vec<u8>>,
}

impl Bank {
    /// Local channels, constants and callbacks, by name as the native local lookup sorts them.
    fn locals(&self) -> Vec<(u32, u32, usize)> {
        let mut locals = self
            .constants
            .keys()
            .chain(self.callbacks.keys())
            .map(|&i| (0x811C9DC5, self.names[i], i))
            .collect::<Vec<_>>();
        locals.sort_unstable();
        locals
    }
}

/// A kind-2 callback channel row, or `None` when the row has no callback.
fn callback(source: &Payload, row: usize, channels: usize) -> Result<Option<Callback>> {
    if source.u64(row + 96)? == 0 {
        return Ok(None);
    }
    let code = source
        .array(row + 8, 1, Some(0x80800009))?
        .iter()
        .map(|at| source.u8(*at))
        .collect::<Result<Vec<_>>>()?;
    let inputs = source.array(row + 56, 4, Some(0x80800007))?;
    ensure!(
        code == CALLBACK_CODE
            && source.u64(row + 24)? == 0
            && source.u64(row + 40)? == 1
            && source.u64(row + 48)? == 1
            && inputs.len() == 1
            && source.u16(row + 72)? == u16::MAX
            && source.u16(row + 74)? == 1
            && source.u64(row + 80)? == 0
            && source.bytes::<2>(row + 104)? == [255, 254],
        "Source callback channel requires procedure translation"
    );
    let input = source.u32(inputs[0])?;
    ensure!(
        usize::try_from(input)? < channels,
        "Source callback input is not a channel of its bank"
    );
    let object = source.pointer(row + 96)?;
    ensure!(
        object >= 4 && source.u32(object - 4)? == SOURCE_KIND_2_CALLBACK,
        "Source channel callback kind requires translation"
    );
    ensure!(
        source.u32(object)? == 2
            && source.u32(object + 4)? == u32::MAX
            && [8, 12]
                .iter()
                .all(|at| source.f32(object + at).is_ok_and(f32::is_finite)),
        "Source kind-2 callback layout differs"
    );
    Ok(Some(Callback {
        input,
        object: source.bytes::<16>(object)?,
    }))
}

impl Bank {
    fn read(source: &Payload) -> Result<Self> {
        let schema = source.pointer(24)?;
        let rows = source.array(schema + 0xD8, 112, Some(0x80800883))?;
        ensure!(
            !rows.is_empty() && rows.len() <= 64,
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
        ensure!(
            !inherited.is_empty()
                && inherited
                    .windows(2)
                    .all(|pair| (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1)),
            "Source inherited names are absent or unsorted"
        );
        let inherited_ordinals = inherited
            .iter()
            .map(|(_, _, ordinal)| *ordinal)
            .collect::<BTreeSet<_>>();
        let mut constants = BTreeMap::new();
        let mut callbacks = BTreeMap::new();
        let mut dependencies = Vec::new();
        for (ordinal, &row) in rows.iter().enumerate() {
            if let Some(found) = callback(source, row, rows.len())? {
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
                ensure!(
                    source.0[row + 16..row + 72].iter().all(|v| *v == 0)
                        && source.u16(row + 74)? == 0
                        && source.u16(row + 106)? == u16::MAX,
                    "Source channel has unsupported defaults or forwarding"
                );
                ensure!(
                    inherited_ordinals.contains(&ordinal) == (source.u16(row + 72)? != u16::MAX),
                    "Source channel inherited slot lacks a name"
                );
            } else {
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
            if !inherited_ordinals.contains(&ordinal) {
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
        let local = source.array(schema + 0xF8, 12, None)?;
        ensure!(
            local.len() == constants.len() + callbacks.len() && source.u64(schema + 0x118)? == 0,
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
                (constants.contains_key(&ordinal) || callbacks.contains_key(&ordinal))
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
            source.u64(instance + 0x70)? == 0
                && source.u64(instance + 0x80)? == 0
                && source.u64(schema + 0xC8)? == 0
                && source.u64(schema + 0xE8)? == 0,
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
        Ok(Self {
            names,
            inherited,
            constants,
            callbacks,
            dependencies,
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

/// The declared channel names of a source bank in ordinal order, which a material's object
/// inputs index.
pub(super) fn channel_names(source: &Payload) -> Result<Vec<u32>> {
    Ok(Bank::read(source)?.names)
}

impl Assembly<'_> {
    pub(super) fn channel_bank(
        &mut self,
        source: &Payload,
        symbol: &str,
        index: usize,
    ) -> Result<(Component, Vec<u32>)> {
        let bank = Bank::read(source)?;
        let locals = bank.locals();
        let (template, entity) = if locals.is_empty() {
            (0x80F56FA1, 0x80BC358D)
        } else {
            (0x8152B5B7, 0x81523111)
        };
        let mut component = self.component(template, entity, symbol, index)?;
        let original = &component.template;
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
                put(&mut row, 106, &position(ordinal)?.to_le_bytes())?;
                row
            } else if bank.callbacks.contains_key(&ordinal) {
                let mut row = [0; 112];
                put(&mut row, 40, &1u64.to_le_bytes())?;
                put(&mut row, 48, &1u64.to_le_bytes())?;
                put(&mut row, 74, &1u16.to_le_bytes())?;
                row[104..106].copy_from_slice(&[255, 254]);
                put(&mut row, 106, &position(ordinal)?.to_le_bytes())?;
                row
            } else {
                let mut row = [0; 112];
                row[104..108].copy_from_slice(&[255, 254, 255, 255]);
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
                array(&mut p.0, row + 8, 0x80800009, &CALLBACK_CODE, 1)?;
                array(
                    &mut p.0,
                    row + 56,
                    0x80800007,
                    &callback.input.to_le_bytes(),
                    4,
                )?;
                // The callback object follows inline, eight-byte aligned behind its class word.
                let object = (p.0.len() + 4).next_multiple_of(8);
                p.0.resize(object - 4, 0);
                p.0.extend(NATIVE_KIND_2_CALLBACK.to_le_bytes());
                p.0.extend(callback.object);
                put(
                    &mut p.0,
                    row + 96,
                    &(object as i64 - (row + 96) as i64).to_le_bytes(),
                )?;
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
        for (at, count) in [
            (0x134, bank.names.len()),
            (0x13C, locals.len()),
            (0x140, bank.inherited.len()),
        ] {
            put(&mut p.0, schema + at, &u32::try_from(count)?.to_le_bytes())?;
        }
        put(&mut p.0, schema + 0x128, &0u16.to_le_bytes())?;
        put(
            &mut p.0,
            schema + 0x12A,
            &u16::try_from(bank.inherited.len() - 1)?.to_le_bytes(),
        )?;
        let tag = p.u32(0x44)?;
        let mut allocation = (*self.native.tag(tag, None)?).clone();
        for (name, class, before, after) in [
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
        }
        let allocation_symbol = format!("{symbol}-allocation");
        self.node(&allocation_symbol, tag, &allocation.0, None, &[])?;
        component.patch(0x44, &allocation_symbol)?;
        component.seal()?;
        self.evidence.push(json!({"source_channel_bank":source.u32(source.pointer(16)?)?,
            "declarations":bank.names.len(),"inherited":bank.inherited.len(),"local_constants":bank.constants.len(),
            "local_callbacks":bank.callbacks.len(),
            "unbound_zero":bank.names.len()-bank.inherited.len()-locals.len()}));
        Ok((component, bank.names))
    }
}
