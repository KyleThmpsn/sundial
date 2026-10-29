//! One applied Jump key owns all its ordered fixed rows and their shared reference lifetime.
use super::write::NO_NAME;
use super::*;

/// A stock row is selected by its complete selection and literal body, not a mutable row index.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedWitness {
    pub key: u32,
    pub selection: FixedSelection,
    pub words: [u32; 8],
}

/// The template establishes the native storage and priority slot. The emitted selection is
/// independent, so a private membership gate does not become a stock template lookup key.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedRow {
    pub class: u32,
    pub words: [u32; 8],
    pub selection: FixedSelection,
    pub witness: FixedWitness,
}

fn word_count(class: u32) -> Result<usize, String> {
    match class {
        0x808044D2 | 0x808042A5 => Ok(1),
        0x808044D4 => Ok(2),
        0x808044D6 => Ok(4),
        _ => Err("Ordered ability rows require a qualified Jump modifier".into()),
    }
}

fn valid_words(class: u32, words: &[u32; 8]) -> Result<(), String> {
    let count = word_count(class)?;
    let valid = match class {
        0x808044D2 => words[0] <= 1,
        0x808042A5 => (1..=16).contains(&words[0]),
        _ => words[..count]
            .iter()
            .all(|&word| f32::from_bits(word).is_finite() && f32::from_bits(word) >= 0.0),
    };
    if !valid || words[count..].iter().any(|&word| word != 0) {
        return Err("Ordered Jump row has an invalid literal body".into());
    }
    Ok(())
}

fn valid_selection(class: u32, selection: FixedSelection) -> bool {
    matches!(selection.priority, 0 | 1 | 2 | 255)
        && ![u32::MAX, NO_NAME].contains(&selection.required_key)
        && (selection.required_key == 0 || class == 0x808042A5)
}

impl FixedRow {
    pub fn validate(&self) -> Result<(), String> {
        valid_words(self.class, &self.words)?;
        valid_words(self.class, &self.witness.words)?;
        if [0, u32::MAX, NO_NAME].contains(&self.witness.key)
            || !valid_selection(self.class, self.selection)
            || !valid_selection(self.class, self.witness.selection)
            || self.selection.name != self.witness.selection.name
            || self.selection.priority != self.witness.selection.priority
        {
            return Err("Ordered Jump row has an unqualified selection or witness".into());
        }
        Ok(())
    }

    fn body(self) -> FixedModifier {
        FixedModifier {
            template_key: self.witness.key,
            class: self.class,
            words: self.words,
            selection: Some(self.selection),
        }
    }

    fn handler(&self, payload: &[u8], layout: &Layout) -> Result<HandlerIndex, String> {
        self.validate()?;
        let mut found = None;
        for index in 0..layout.definition_rows.count {
            let at = layout.definition_rows.first + index * DEFINITION_ROW_SIZE;
            if u32_at(payload, at + ROW_KEY)? != self.witness.key
                || u32_at(payload, at + ROW_MODE)? != 255
                || u32_at(payload, at + ROW_MODE + 4)? != 0
                || selection(payload, at)? != self.witness.selection
            {
                continue;
            }
            let modifier = pointer(payload, at + ROW_MODIFIER)?;
            if u32_at(payload, modifier + BLOCK_CLASS)? != self.class {
                continue;
            }
            check_body(payload, layout, modifier, self.class)?;
            if words(payload, modifier, self.class)? != self.witness.words {
                continue;
            }
            let handler = HandlerIndex::new(u32_at(payload, at + ROW_HANDLER)?);
            if found.replace(handler).is_some() {
                return Err("Ordered Jump row has more than one matching native witness".into());
            }
        }
        found.ok_or_else(|| "Ordered Jump row has no complete native witness".into())
    }
}

fn selection(payload: &[u8], at: usize) -> Result<FixedSelection, String> {
    let required_key = if u64_at(payload, at + ROW_GATE)? == 0 {
        0
    } else {
        let gate = pointer(payload, at + ROW_GATE)?;
        if gate < 4 || u32_at(payload, gate - 4)? != GATE_CLASS || u32_at(payload, gate + 4)? != 0 {
            return Err("Ordered Jump row has an invalid membership gate".into());
        }
        u32_at(payload, gate)?
    };
    Ok(FixedSelection {
        name: u32_at(payload, at + ROW_NAME)?,
        priority: u8::try_from(u32_at(payload, at + ROW_FLAGS)?)
            .map_err(|_| "Ordered Jump row priority exceeds its byte")?,
        required_key,
    })
}

fn words(payload: &[u8], modifier: usize, class: u32) -> Result<[u32; 8], String> {
    let mut words = [0; 8];
    for (index, word) in words[..word_count(class)?].iter_mut().enumerate() {
        *word = u32_at(payload, modifier + 16 + 4 * index)?;
    }
    Ok(words)
}

fn check_body(payload: &[u8], layout: &Layout, modifier: usize, class: u32) -> Result<(), String> {
    let twin = usize::try_from(u64_at(payload, modifier + BLOCK_TWIN)?)
        .map_err(|_| "Ordered Jump modifier twin does not fit this platform")?;
    if modifier < 4
        || twin < 4
        || u32_at(payload, modifier - 4)? != class + 1
        || u32_at(payload, twin - 4)? != class
        || u32_at(payload, modifier)? != layout.owner
        || u32_at(payload, twin)? != layout.owner
        || u32_at(payload, twin + BLOCK_CLASS)? != class + 1
        || u64_at(payload, twin + BLOCK_TWIN)? != modifier as u64
    {
        return Err("Ordered Jump modifier has another owner or reciprocal class".into());
    }
    words(payload, modifier, class).map(|_| ())
}

fn jump_layout(payload: &[u8]) -> Result<Layout, String> {
    let layout = layout(payload)?;
    if u32_at(payload, layout.definition - 4)? != 0x8080429E
        || u32_at(payload, layout.instance - 4)? != 0x8080429F
        || u32_at(payload, layout.definition + BLOCK_CLASS)? != 0x8080429F
        || u32_at(payload, layout.instance + BLOCK_CLASS)? != 0x8080429E
    {
        return Err("Ordered fixed rows require a qualified Jump bank".into());
    }
    let priorities = rows(
        payload,
        layout.definition + 0x190,
        0x80800009,
        "Jump priority",
    )?;
    if u64_at(payload, layout.instance + 0xB8)? != priorities.count as u64
        || priorities.first < layout.definition
        || priorities.first + priorities.count > layout.instance - 4
        || (0..layout.definition_rows.count).any(|index| {
            u32_at(
                payload,
                layout.definition_rows.first + index * DEFINITION_ROW_SIZE + ROW_HANDLER,
            )
            .map_or(true, |slot| slot as usize >= priorities.count)
        })
    {
        return Err("Ordered Jump rows leave their bank-local priority storage".into());
    }
    Ok(layout)
}

/// Compares the entire group through its live row and modifier references. A partial group or
/// different order is a conflict, including when a private bank already carries the same key.
pub fn ordered_rows_match(payload: &[u8], key: u32, members: &[FixedRow]) -> Result<bool, String> {
    let layout = jump_layout(payload)?;
    let rows = (0..layout.definition_rows.count)
        .map(|index| layout.definition_rows.first + index * DEFINITION_ROW_SIZE)
        .filter(|&at| u32_at(payload, at + ROW_KEY).ok() == Some(key))
        .collect::<Vec<_>>();
    if members.is_empty() || rows.len() != members.len() {
        return Ok(false);
    }
    for (&at, member) in rows.iter().zip(members) {
        let handler = member.handler(payload, &layout)?;
        let body = pointer(payload, at + ROW_MODIFIER)?;
        if u32_at(payload, body + BLOCK_CLASS)? != member.class
            || u32_at(payload, at + ROW_MODE)? != 255
            || u32_at(payload, at + ROW_MODE + 4)? != 0
            || u32_at(payload, at + ROW_HANDLER)? != handler.get()
            || selection(payload, at)? != member.selection
            || words(payload, body, member.class)? != member.words
        {
            return Ok(false);
        }
        check_body(payload, &layout, body, member.class)?;
    }
    Ok(true)
}

/// Appends all members atomically, preserving stock order and every member's private gate.
/// One successful Apply operation admits the entire group and one cleanup releases its reference.
pub fn with_ordered_rows(
    payload: &[u8],
    key: u32,
    members: &[FixedRow],
) -> Result<Vec<u8>, String> {
    if members.is_empty() || [0, u32::MAX, NO_NAME].contains(&key) {
        return Err("Ordered ability property has no members or valid identity".into());
    }
    let layout = jump_layout(payload)?;
    let before = property_rows(payload)?;
    if before.iter().any(|row| row.key == key) {
        return Err(format!(
            "The bank already defines ordered property {key:08X}"
        ));
    }
    let handlers = members
        .iter()
        .map(|member| member.handler(payload, &layout))
        .collect::<Result<Vec<_>, _>>()?;
    let count = layout.definition_rows.count;
    let rows = count
        .checked_add(members.len())
        .ok_or("Ordered row count overflow")?;
    if rows > 4096 {
        return Err("Ordered ability rows exceed the checked bank row limit".into());
    }
    let links = blocks(payload, layout.owner);
    let markers = members
        .iter()
        .map(|member| member.class)
        .collect::<Vec<_>>();
    let mut region = open_definition_region(payload, &layout, rows, &markers)?;
    let shift = Shift {
        instance: layout.instance,
        delta: region.delta,
    };
    follow_shift(payload, &mut region.out, &links, shift)?;
    let out = &mut region.out;
    let definition_header = open_block(out, ARRAY_MARKER);
    out.resize(definition_header + 16 + rows * DEFINITION_ROW_SIZE, 0);
    put_u64(out, definition_header, rows as u64);
    put_u32(out, definition_header + 8, INSTANCE_ROW_CLASS);
    let mut added = Vec::with_capacity(members.len());
    for ((member, &handler), &modifier_instance) in members
        .iter()
        .zip(&handlers)
        .zip(&region.modifier_instances)
    {
        let body = member.body();
        let classes = modifier_classes(Modifier::Fixed(body))?;
        let modifier_definition = open_block(out, classes.instance);
        out.resize(modifier_definition + classes.definition_size, 0);
        let gate = (member.selection.required_key != 0)
            .then(|| append_gate(out, member.selection.required_key));
        added.push(Added {
            key,
            before: count,
            gate,
            handler,
            classes,
            modifier_instance,
            modifier_definition,
            body: Body::Fixed(body),
        });
    }
    out.resize(out.len().next_multiple_of(16), 0);
    let placement = Placement {
        instance_first: region.instance_first,
        definition_first: definition_header + 16,
    };
    write_rows(payload, out, &layout, count, &placement, shift, &added)?;
    for row in &added {
        write_modifier_pair(out, layout.owner, row)?;
    }
    write_descriptors(
        out,
        &layout,
        shift,
        rows,
        region.instance_header,
        definition_header,
        None,
    )?;
    let size = out.len() as u64;
    put_u64(out, SIZE, size);
    if blocks(out, layout.owner).len() != links.len() + 2 * rows + 2 * added.len() {
        return Err("The ordered bank edit lost a reciprocal block".into());
    }
    check_read_back(out, &before, &added, None)?;
    if !ordered_rows_match(out, key, members)? {
        return Err("The ordered ability group did not read back completely".into());
    }
    Ok(region.out)
}
