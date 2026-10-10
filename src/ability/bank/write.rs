//! The steps that append a property row, from the byte writers to the read back check.
use super::*;

/// The header's relative pointers: the two block pointers, the reference table at +0x20, the
/// runtime block table at +0x30 and the trailing pointer, any of which may reach the instance
/// region.
const HEADER_POINTERS: [usize; 6] = [0x08, DEFINITION_POINTER, INSTANCE_POINTER, 0x28, 0x38, 0x48];
/// The header tables, count at the offset and pointer eight bytes on, with their entry stride;
/// each entry starts with a relative pointer that may reach the other region.
const HEADER_TABLES: [(usize, usize); 2] = [(0x20, 8), (0x30, 16)];
/// A definition modifier: 16 bytes of header, then its body.
const CHARGE_DEFINITION_SIZE: usize = 24;
const SCRIPT_DEFINITION_SIZE: usize = 40;
/// Every stock attack modifier keeps a relative pointer at +0x20, another at +0x28 and a word at
/// +0x30 before its attack record. Attack selection follows +0x28 when it is not zero, so a
/// shorter block puts the appended record's marker there and the client faults on it.
const ATTACK_DEFINITION_SIZE: usize = 0x38;
/// An instance modifier: the header, a pointer back to its instance row, and zeros.
const INSTANCE_MODIFIER_SIZE: usize = 40;
/// The name of a row with no name, FNV-1's offset basis, as the Dodge charge row is named.
const NO_NAME: u32 = 0x811C_9DC5;

/// Opens a block at the next 16 byte boundary, with its marker in the four bytes before it.
pub(super) fn open_block(out: &mut Vec<u8>, marker: u32) -> usize {
    let at = (out.len() + 4).next_multiple_of(16);
    out.resize(at, 0);
    out[at - 4..at].copy_from_slice(&marker.to_le_bytes());
    at
}

pub(super) fn put_u32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn put_u64(out: &mut [u8], at: usize, value: u64) {
    out[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// Writes a relative pointer at `at` that lands on `target`.
pub(super) fn put_pointer(out: &mut [u8], at: usize, target: usize) -> Result<(), String> {
    let relative = i64::try_from(target)
        .ok()
        .zip(i64::try_from(at).ok())
        .and_then(|(target, at)| target.checked_sub(at))
        .ok_or_else(|| "Bank pointer does not fit i64".to_owned())?;
    out[at..at + 8].copy_from_slice(&relative.to_le_bytes());
    Ok(())
}

/// What a modifier is called when the bank cannot take it.
pub(super) fn modifier_kind(modifier: Modifier) -> &'static str {
    match modifier {
        Modifier::Charges(_) => "charge",
        Modifier::Parameter { .. } => "script parameter",
        Modifier::Melee { .. } => "melee attack",
        Modifier::Scalar { .. } => "base ability input",
    }
}

pub(super) fn modifier_classes(modifier: Modifier) -> ModifierClasses {
    let (definition, instance, definition_size) = match modifier {
        Modifier::Charges(_) => (
            CHARGE_DEFINITION_CLASS,
            CHARGE_INSTANCE_CLASS,
            CHARGE_DEFINITION_SIZE,
        ),
        Modifier::Parameter { .. } => (
            SCRIPT_DEFINITION_CLASS,
            SCRIPT_INSTANCE_CLASS,
            SCRIPT_DEFINITION_SIZE,
        ),
        Modifier::Melee { .. } => (
            ATTACK_DEFINITION_CLASS,
            ATTACK_INSTANCE_CLASS,
            ATTACK_DEFINITION_SIZE,
        ),
        Modifier::Scalar { .. } => (scalar::CLASS, scalar::TWIN, 40),
    };
    ModifierClasses {
        definition,
        instance,
        definition_size,
    }
}

/// The parameter row a parameter modifier appends: the bank's own reset value with the
/// modifier's applied value. Refuses a parameter the bank does not list.
pub(super) fn listed_parameter(
    table: &[Parameter],
    modifier: Modifier,
) -> Result<Option<Parameter>, String> {
    match modifier {
        Modifier::Charges(_) | Modifier::Melee { .. } | Modifier::Scalar { .. } => Ok(None),
        Modifier::Parameter { name, applied, add } => {
            let listed = table
                .iter()
                .find(|parameter| parameter.name == name)
                .ok_or_else(|| format!("The bank does not list script parameter {name:08X}"))?;
            Ok(Some(Parameter {
                name,
                reset: listed.reset,
                applied,
                add,
            }))
        }
    }
}

/// The definition region, then the instance rows array of `rows` rows and an instance modifier
/// for each new row, behind its definition's class in `markers`, then the instance region moved
/// down by a multiple of 16 so every offset keeps its alignment. The instance block's marker
/// sits in the four bytes before it.
pub(super) fn open_definition_region(
    payload: &[u8],
    layout: &Layout,
    rows: usize,
    markers: &[u32],
) -> Region {
    let instance = layout.instance;
    let marker = instance - 4;
    let mut out = payload[..marker].to_vec();
    let instance_header = open_block(&mut out, ARRAY_MARKER);
    out.resize(instance_header + 16 + rows * INSTANCE_ROW_SIZE, 0);
    put_u64(&mut out, instance_header, rows as u64);
    put_u32(&mut out, instance_header + 8, DEFINITION_ROW_CLASS);
    let modifier_instances = markers
        .iter()
        .map(|&definition_class| {
            let at = open_block(&mut out, definition_class);
            out.resize(at + INSTANCE_MODIFIER_SIZE, 0);
            at
        })
        .collect();
    while (out.len() + 4) % 16 != instance % 16 {
        out.push(0);
    }
    let delta = out.len() + 4 - instance;
    out.extend_from_slice(&payload[marker..]);
    Region {
        out,
        instance_header,
        instance_first: instance_header + 16,
        modifier_instances,
        delta,
    }
}

/// Every twin offset follows its block, every header pointer and header table entry that
/// reaches the instance region follows it, and a table entry that moved with the instance
/// region but points back keeps its target.
pub(super) fn follow_shift(
    payload: &[u8],
    out: &mut [u8],
    links: &BTreeMap<usize, usize>,
    shift: Shift,
) -> Result<(), String> {
    for (&block, &twin) in links {
        put_u64(out, shift.at(block) + BLOCK_TWIN, shift.at(twin) as u64);
    }
    for at in HEADER_POINTERS {
        if i64_at(payload, at)? != 0 {
            let target = pointer(payload, at)?;
            put_pointer(out, at, shift.at(target))?;
        }
    }
    for (table, stride) in HEADER_TABLES {
        let entries = usize::try_from(u64_at(payload, table)?)
            .map_err(|_| "Bank header table count does not fit this platform".to_owned())?;
        if entries == 0 {
            continue;
        }
        let first = pointer(payload, table + 8)? + 16;
        for entry in (0..entries).map(|index| first + index * stride) {
            if i64_at(payload, entry)? != 0 {
                let target = pointer(payload, entry)?;
                put_pointer(out, shift.at(entry), shift.at(target))?;
            }
        }
    }
    Ok(())
}

/// The parameter table again at the end, one row longer, with the new parameter last.
/// Returns the new table's header.
pub(super) fn append_parameter_table(
    out: &mut Vec<u8>,
    layout: &Layout,
    table: &[Parameter],
    parameter: Parameter,
    shift: Shift,
) -> usize {
    let header = open_block(out, ARRAY_MARKER);
    out.resize(header + 16 + (table.len() + 1) * PARAMETER_ROW_SIZE, 0);
    put_u64(out, header, table.len() as u64 + 1);
    put_u32(out, header + 8, PARAMETER_ROW_CLASS);
    if let Some(old) = &layout.parameters {
        let first = shift.at(old.first);
        out.copy_within(first..first + table.len() * PARAMETER_ROW_SIZE, header + 16);
    }
    let row = header + 16 + table.len() * PARAMETER_ROW_SIZE;
    put_u32(out, row, parameter.name);
    put_u32(out, row + 4, parameter.reset.to_bits());
    put_u32(out, row + 8, parameter.applied.to_bits());
    put_u32(out, row + 12, u32::from(parameter.add));
    header
}

/// What one place in the rebuilt row arrays holds: a stock row by its old index, or an added
/// row by its index among the added ones.
#[derive(Clone, Copy)]
pub(super) enum Slot {
    Stock(usize),
    Added(usize),
}

/// The rebuilt row order: the stock rows in order, each added row just before the stock row it
/// names, in the order given, and those naming the stock row count last.
pub(super) fn arrangement(count: usize, added: &[Added]) -> Vec<Slot> {
    let mut slots = Vec::with_capacity(count + added.len());
    for old in 0..=count {
        for (new, row) in added.iter().enumerate() {
            if row.before.min(count) == old {
                slots.push(Slot::Added(new));
            }
        }
        if old < count {
            slots.push(Slot::Stock(old));
        }
    }
    slots
}

/// Every stock row copied into the new arrays with the added rows among them, each pair
/// twinned, pointed at the bank and at its modifiers, each gated row pointed at its gate block
/// and each instance modifier pointed back at its moved row.
pub(super) fn write_rows(
    payload: &[u8],
    out: &mut [u8],
    layout: &Layout,
    count: usize,
    placement: &Placement,
    shift: Shift,
    added: &[Added],
) -> Result<(), String> {
    for (index, slot) in arrangement(count, added).into_iter().enumerate() {
        let instance_row = placement.instance_first + index * INSTANCE_ROW_SIZE;
        let definition_row = placement.definition_first + index * DEFINITION_ROW_SIZE;
        let (instance_target, definition_target, gate) = match slot {
            Slot::Stock(old) => {
                let old_instance = layout.instance_rows.first + old * INSTANCE_ROW_SIZE;
                let old_definition = layout.definition_rows.first + old * DEFINITION_ROW_SIZE;
                out[instance_row..instance_row + INSTANCE_ROW_SIZE]
                    .copy_from_slice(&payload[old_instance..old_instance + INSTANCE_ROW_SIZE]);
                out[definition_row..definition_row + DEFINITION_ROW_SIZE].copy_from_slice(
                    &payload[old_definition..old_definition + DEFINITION_ROW_SIZE],
                );
                // The copied pointer is relative to the old row, and its block moved with the
                // instance region.
                let gate = if i64_at(payload, old_definition + ROW_GATE)? != 0 {
                    Some(shift.at(pointer(payload, old_definition + ROW_GATE)?))
                } else {
                    None
                };
                (
                    pointer(payload, old_instance + INSTANCE_ROW_MODIFIER)?,
                    shift.at(pointer(payload, old_definition + ROW_MODIFIER)?),
                    gate,
                )
            }
            Slot::Added(new) => {
                let row = &added[new];
                put_u32(out, instance_row, layout.owner);
                put_u32(out, instance_row + BLOCK_CLASS, INSTANCE_ROW_CLASS);
                put_u32(out, definition_row, layout.owner);
                put_u32(out, definition_row + BLOCK_CLASS, DEFINITION_ROW_CLASS);
                put_u32(out, definition_row + ROW_KEY, row.key);
                put_u32(out, definition_row + ROW_NAME, NO_NAME);
                put_u32(out, definition_row + ROW_FLAGS, 0xFF);
                put_u32(out, definition_row + ROW_HANDLER, row.handler.get());
                put_u32(out, definition_row + ROW_MODE, 0xFF);
                (row.modifier_instance, row.modifier_definition, row.gate)
            }
        };
        put_u64(out, instance_row + BLOCK_TWIN, definition_row as u64);
        put_u64(out, definition_row + BLOCK_TWIN, instance_row as u64);
        put_pointer(out, instance_row + INSTANCE_ROW_BANK, layout.definition)?;
        put_pointer(out, instance_row + INSTANCE_ROW_MODIFIER, instance_target)?;
        put_pointer(out, definition_row + ROW_MODIFIER, definition_target)?;
        if let Some(gate) = gate {
            put_pointer(out, definition_row + ROW_GATE, gate)?;
        }
        // The modifier's own pointer back at its row follows the row to the new array.
        put_pointer(out, instance_target + INSTANCE_MODIFIER_ROW, instance_row)?;
    }
    Ok(())
}

/// The ranged melee window modifier's classes. Its definition holds the window's two floats
/// after the header, the instance only the header and the pointer back at its row.
pub(super) const WINDOW_CLASSES: ModifierClasses = ModifierClasses {
    definition: WINDOW_DEFINITION_CLASS,
    instance: WINDOW_INSTANCE_CLASS,
    definition_size: 0x18,
};

/// A window no reading lies in, so a melee press always strikes.
pub(super) const CLOSED_WINDOW: (f32, f32) = (f32::INFINITY, f32::NEG_INFINITY);

/// The handler slot the bank's own window rows use, or `None` for a bank without them.
pub(super) fn window_handler(rows: &[PropertyRow]) -> Result<Option<HandlerIndex>, String> {
    let slots: BTreeSet<HandlerIndex> = rows
        .iter()
        .filter(|row| row.modifier_class == WINDOW_DEFINITION_CLASS)
        .map(|row| row.handler)
        .collect();
    match slots.len() {
        0 | 1 => Ok(slots.into_iter().next()),
        _ => Err(format!(
            "The bank hands window modifiers to several handler slots {slots:?}"
        )),
    }
}

/// The keys whose rows open the ranged melee window, each with the index of the key's last row,
/// in row order. A weapon melee's closed window gated on its key after those rows is written
/// over the opened one whenever both keys apply, whichever was applied first.
pub(super) fn window_keys(payload: &[u8], layout: &Layout) -> Result<Vec<(u32, usize)>, String> {
    let mut last = BTreeMap::new();
    let mut openers = BTreeSet::new();
    for index in 0..layout.definition_rows.count {
        let row = layout.definition_rows.first + index * DEFINITION_ROW_SIZE;
        let key = u32_at(payload, row + ROW_KEY)?;
        last.insert(key, index);
        let modifier = pointer(payload, row + ROW_MODIFIER)?;
        if u32_at(payload, modifier + BLOCK_CLASS)? == WINDOW_DEFINITION_CLASS {
            let low = f32::from_bits(u32_at(payload, modifier + WINDOW_LOW)?);
            let high = f32::from_bits(u32_at(payload, modifier + WINDOW_HIGH)?);
            if low <= high {
                openers.insert(key);
            }
        }
    }
    let mut keys: Vec<(u32, usize)> = last
        .into_iter()
        .filter(|(key, _)| openers.contains(key))
        .collect();
    keys.sort_by_key(|&(_, index)| index);
    Ok(keys)
}

/// A gate block naming `key`, which a row pointing at it needs applied with its own.
pub(super) fn append_gate(out: &mut Vec<u8>, key: u32) -> usize {
    let at = open_block(out, GATE_CLASS);
    out.extend_from_slice(&key.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    at
}

/// An added row's modifier pair: headers, twins, and the definition's body for its class.
pub(super) fn write_modifier_pair(out: &mut [u8], owner: u32, row: &Added) -> Result<(), String> {
    let (modifier_instance, modifier_definition) = (row.modifier_instance, row.modifier_definition);
    put_u32(out, modifier_instance, owner);
    put_u32(out, modifier_instance + BLOCK_CLASS, row.classes.instance);
    put_u64(
        out,
        modifier_instance + BLOCK_TWIN,
        modifier_definition as u64,
    );
    put_u32(out, modifier_definition, owner);
    put_u32(
        out,
        modifier_definition + BLOCK_CLASS,
        row.classes.definition,
    );
    put_u64(
        out,
        modifier_definition + BLOCK_TWIN,
        modifier_instance as u64,
    );
    match row.body {
        Body::Charges(charges) => {
            put_u64(out, modifier_definition + CHARGE_VALUE, charges as u64);
        }
        Body::Parameter(index) => {
            put_u32(out, modifier_definition + SCRIPT_FIRST, index as u32);
            put_u32(out, modifier_definition + SCRIPT_COUNT, 1);
        }
        Body::Attack(first, second) => {
            // The unpowered native override's selector values, retained verbatim.
            put_u32(out, modifier_definition + 0x10, 0);
            put_u32(out, modifier_definition + 0x14, 2.0f32.to_bits());
            put_pointer(out, modifier_definition + 0x18, first)?;
            put_pointer(out, modifier_definition + 0x20, second)?;
        }
        Body::Window((low, high)) => {
            put_u32(out, modifier_definition + WINDOW_LOW, low.to_bits());
            put_u32(out, modifier_definition + WINDOW_HIGH, high.to_bits());
        }
        Body::Own => {
            return Err("Numeric modifiers require their paired input arrays".into());
        }
    }
    Ok(())
}

/// Both blocks' descriptors name the new arrays of `rows` rows, and the instance block's
/// parameter table when one was appended.
pub(super) fn write_descriptors(
    out: &mut [u8],
    layout: &Layout,
    shift: Shift,
    rows: usize,
    instance_header: usize,
    definition_header: usize,
    parameters: Option<(usize, usize)>,
) -> Result<(), String> {
    let moved_instance = shift.at(layout.instance);
    put_u64(
        out,
        layout.definition + INSTANCE_ROWS_DESCRIPTOR,
        rows as u64,
    );
    put_pointer(
        out,
        layout.definition + INSTANCE_ROWS_DESCRIPTOR + 8,
        instance_header,
    )?;
    put_u64(
        out,
        moved_instance + DEFINITION_ROWS_DESCRIPTOR,
        rows as u64,
    );
    put_pointer(
        out,
        moved_instance + DEFINITION_ROWS_DESCRIPTOR + 8,
        definition_header,
    )?;
    if let Some((header, rows)) = parameters {
        put_u64(out, moved_instance + PARAMETERS_DESCRIPTOR, rows as u64);
        put_pointer(out, moved_instance + PARAMETERS_DESCRIPTOR + 8, header)?;
    }
    Ok(())
}

/// The result reads back, through the client's path, as the same rows with the new ones where
/// they were placed.
pub(super) fn check_read_back(
    out: &[u8],
    before: &[PropertyRow],
    added: &[Added],
    parameter: Option<Parameter>,
) -> Result<(), String> {
    let after = property_rows(out)?;
    let expected: Vec<PropertyRow> = arrangement(before.len(), added)
        .into_iter()
        .map(|slot| match slot {
            Slot::Stock(old) => before[old].clone(),
            Slot::Added(new) => {
                let row = &added[new];
                PropertyRow {
                    key: row.key,
                    name: NO_NAME,
                    handler: row.handler,
                    modifier_class: row.classes.definition,
                    charge: match row.body {
                        Body::Charges(charges) => Some(charges),
                        _ => None,
                    },
                    parameters: match row.body {
                        Body::Parameter(_) => parameter.into_iter().collect(),
                        _ => Vec::new(),
                    },
                }
            }
        })
        .collect();
    if after != expected {
        return Err("The edited bank does not read back as its rows with the new ones".into());
    }
    Ok(())
}

/// The stock per-surface impact responses the native melee template names.
const MELEE_RESPONSES: u32 = 0x80FE_E146;
/// The marker before every attack record an attack modifier names.
const ATTACK_RECORD_MARKER: u32 = 0x8080_4464;
/// The attack records an attack modifier names, at +0x18 and, on most Warlock rows, +0x20.
const ATTACK_RECORDS: [usize; 2] = [0x18, 0x20];
/// The attack record's selection priority. Attack selection (`D17850`) keeps the candidate
/// whose record holds the larger float here and, on a tie, the one walked first. Every stock
/// attack record leaves it at zero, so a subclass melee key applied before the weapon's own won
/// every tie and the class attack played.
const ATTACK_PRIORITY: usize = 4;

/// The priorities of the attack records a bank row's modifier names, or `None` for a row
/// whose modifier is not an attack.
fn attack_priorities(payload: &[u8], index: usize, row: usize) -> Result<Option<Vec<f32>>, String> {
    let modifier = pointer(payload, row + ROW_MODIFIER)?;
    if u32_at(payload, modifier + BLOCK_CLASS)? != ATTACK_DEFINITION_CLASS {
        return Ok(None);
    }
    let mut priorities = Vec::new();
    for field in ATTACK_RECORDS {
        if i64_at(payload, modifier + field)? == 0 {
            continue;
        }
        let record = pointer(payload, modifier + field)?;
        if record < 4 || u32_at(payload, record - 4)? != ATTACK_RECORD_MARKER {
            return Err(format!(
                "Bank row {index}'s attack record is not behind its marker"
            ));
        }
        let priority = f32::from_bits(u32_at(payload, record + ATTACK_PRIORITY)?);
        if !priority.is_finite() {
            return Err(format!(
                "Bank row {index}'s attack priority is not a number"
            ));
        }
        priorities.push(priority);
    }
    Ok(Some(priorities))
}

/// One above the highest priority of any attack the bank already holds, so a weapon's own
/// melee outranks the class and subclass attacks applied with it.
pub(super) fn weapon_attack_priority(payload: &[u8], layout: &Layout) -> Result<f32, String> {
    let mut highest = 0.0f32;
    for index in 0..layout.definition_rows.count {
        let row = layout.definition_rows.first + index * DEFINITION_ROW_SIZE;
        for priority in attack_priorities(payload, index, row)?.unwrap_or_default() {
            highest = highest.max(priority);
        }
    }
    Ok(highest + 1.0)
}

/// The keys of the bank's own attacks, each with the index of its first row, in row order:
/// the keys of attack rows whose records all rank zero, as every stock attack does, unlike the
/// weapon attacks ranked above them.
///
/// Attack selection reads three lists of attack rows on the melee (ability +0xCB0, +0xCBC and
/// +0xCC8), each holding at most two rows. `105C580` adds a row to every list whose record its
/// modifier names while the list has room, walking the applied keys in the order they were
/// applied and each key's rows in bank order. A subclass key applied before a weapon's, with
/// two attack rows of its own, so filled the always walked list that the weapon's attack never
/// reached it on the ground. A weapon attack's copy under each of these keys, gated on the
/// weapon's key and ahead of the key's rows, reaches the lists first.
pub(super) fn own_attack_keys(
    payload: &[u8],
    layout: &Layout,
) -> Result<Vec<(u32, usize)>, String> {
    let mut first = BTreeMap::new();
    let mut own = BTreeSet::new();
    for index in 0..layout.definition_rows.count {
        let row = layout.definition_rows.first + index * DEFINITION_ROW_SIZE;
        let key = u32_at(payload, row + ROW_KEY)?;
        first.entry(key).or_insert(index);
        if let Some(priorities) = attack_priorities(payload, index, row)?
            && priorities.iter().all(|priority| *priority == 0.0)
        {
            own.insert(key);
        }
    }
    let mut keys: Vec<(u32, usize)> = first
        .into_iter()
        .filter(|(key, _)| own.contains(key))
        .collect();
    keys.sort_by_key(|&(_, index)| index);
    Ok(keys)
}

pub(super) fn append_melee(
    out: &mut Vec<u8>,
    damage: u32,
    responses: Option<u32>,
    priority: f32,
) -> Result<usize, String> {
    use crate::sandbox_perk::action::native::Graph;
    let mut graph: Graph = serde_json::from_str(include_str!("melee.json"))
        .map_err(|error| format!("Native melee template: {error}"))?;
    let mut replaced = 0;
    for block in &mut graph.blocks {
        if block.class == 0x8080_444C {
            if u32_at(&block.bytes, 0x38)? != MELEE_DAMAGE_PROFILE {
                return Err("Native melee template has a different contact profile".into());
            }
            put_u32(&mut block.bytes, 0x38, damage);
            if let Some(responses) = responses {
                if u32_at(&block.bytes, 0x48)? != MELEE_RESPONSES {
                    return Err("Native melee template has different impact responses".into());
                }
                put_u32(&mut block.bytes, 0x48, responses);
            }
            replaced += 1;
        }
    }
    if replaced != 1 {
        return Err("Native melee template does not select one contact profile".into());
    }
    let mut bytes = graph.emit()?;
    if Graph::read(&bytes, 0, ATTACK_RECORD_MARKER)? != graph {
        return Err("Native melee attack changed during relocation".into());
    }
    if u32_at(&bytes, ATTACK_PRIORITY)? != 0 {
        return Err("Native melee template already ranks its attack".into());
    }
    put_u32(&mut bytes, ATTACK_PRIORITY, priority.to_bits());
    let start = open_block(out, ATTACK_RECORD_MARKER);
    out.extend_from_slice(&bytes);
    Ok(start)
}
