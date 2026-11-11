//! Attack pairs own separate profiles and selectors in the definition region. A movement attack
//! (`80804119`) names its two records at `+10/+18`. A melee attack container (`80804427`, the
//! common handler's `0105C7F7`) names them at `+18/+20` behind two Float32 it keeps, and appends
//! both records to its attack lists, as the source container (`010E1354`) does.
use super::*;
use crate::sandbox_perk::action::native::Graph;

pub(super) const CLASS: u32 = 0x8080_4119;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AttackModifier {
    pub template_key: u32,
    pub selection: FixedSelection,
    /// Resolved private profiles. None retains the template during structural preflight.
    pub damage: Option<[u32; 2]>,
    /// A melee attack container rather than a movement attack.
    pub melee: bool,
    /// Each copied record's distance controls when they are not the template's, Float32 bits in
    /// `CONTROLS` order. Source melee containers reach farther than Shadowkeep's.
    pub controls: Option<[[u32; 5]; 2]>,
}

/// A record's distance controls, by the marker of the block holding each and its offset there:
/// the profile's reach (`+2C`) and range (`+204`), its lunge's range (`+20`) and next lane
/// (`+30`), and its contact control (`+8`). Source and native records keep each lane at the same
/// place relative to the corresponding block.
pub const CONTROLS: [(u32, usize); 5] = [
    (0x8080_2D7B, 0x2C),
    (0x8080_2D7B, 0x204),
    (0x8080_4445, 0x20),
    (0x8080_4445, 0x30),
    (0x8080_4455, 0x8),
];

impl AttackModifier {
    /// The modifier class the row's container takes.
    pub(super) fn class(self) -> u32 {
        if self.melee {
            ATTACK_DEFINITION_CLASS
        } else {
            CLASS
        }
    }

    pub fn validate(self) -> Result<(), String> {
        let valid = |key| ![0, u32::MAX, 0x811C_9DC5].contains(&key);
        // A movement attack is a subclass-restricted wildcard row. A melee container keeps its
        // source priority, and its gate is optional.
        if !valid(self.template_key)
            || !(valid(self.selection.required_key)
                || (self.melee && self.selection.required_key == 0))
            || self.selection.name == 0
            || (!self.melee && self.selection.priority != 255)
            || self.controls.is_some_and(|records| {
                !self.melee
                    || records
                        .iter()
                        .flatten()
                        .any(|&value| !f32::from_bits(value).is_finite())
            })
            || self.damage.is_some_and(|tags| {
                tags.iter()
                    .any(|&t| !crate::package_runtime::is_valid_package_tag(tiger_pkg::TagHash(t)))
            })
        {
            return Err("Movement attack has an invalid selection or damage profile".into());
        }
        Ok(())
    }

    pub fn profiles(self, payload: &[u8]) -> Result<[u32; 2], String> {
        let roots = self.roots(payload)?;
        let mut result = [0; 2];
        for (i, root) in roots.into_iter().enumerate() {
            let profile = profile(payload, root)?;
            let contact = pointer(payload, profile + 0x1A8)?;
            if contact < 4 || u32_at(payload, contact - 4)? != 0x8080_4455 {
                return Err("Movement attack has another contact layout".into());
            }
            result[i] = u32_at(payload, contact + 40)?;
        }
        Ok(result)
    }

    pub(super) fn roots(self, payload: &[u8]) -> Result<[usize; 2], String> {
        if self.melee {
            let modifier = self.melee_template(payload)?;
            let roots = [
                pointer(payload, modifier + 0x18)?,
                pointer(payload, modifier + 0x20)?,
            ];
            for &root in &roots {
                profile(payload, root)?;
            }
            return Ok(roots);
        }
        self.validate()?;
        let layout = layout(payload)?;
        if u32_at(payload, layout.definition - 4)? != 0x8080_4112
            || u32_at(payload, layout.instance - 4)? != 0x8080_4113
        {
            return Err("Movement attack requires a movement bank".into());
        }
        let candidates = (0..layout.definition_rows.count)
            .map(|i| layout.definition_rows.first + i * DEFINITION_ROW_SIZE)
            .filter(|&r| u32_at(payload, r + ROW_KEY).ok() == Some(self.template_key))
            .collect::<Vec<_>>();
        let [row] = candidates.as_slice() else {
            return Err("Movement attack requires one template property".into());
        };
        let gate = pointer(payload, row + ROW_GATE)?;
        let modifier = pointer(payload, row + ROW_MODIFIER)?;
        let twin = usize::try_from(u64_at(payload, modifier + 8)?)
            .map_err(|_| "Movement attack twin exceeds this platform")?;
        if gate < 4
            || modifier < 4
            || twin < 4
            || u32_at(payload, gate - 4)? != GATE_CLASS
            || u32_at(payload, gate)? != self.selection.required_key
            || u32_at(payload, gate + 4)? != 0
            || u32_at(payload, row + ROW_FLAGS)? != 255
            || u32_at(payload, row + ROW_MODE)? != 255
            || u32_at(payload, row + 36)? != 0
            || u32_at(payload, modifier)? != layout.owner
            || u32_at(payload, modifier + 4)? != CLASS
            || u32_at(payload, modifier - 4)? != CLASS + 1
            || u32_at(payload, twin)? != layout.owner
            || u32_at(payload, twin + 4)? != CLASS + 1
            || u32_at(payload, twin - 4)? != CLASS
            || u64_at(payload, twin + 8)? != modifier as u64
        {
            return Err("Movement attack template has a different gate, mode or pair".into());
        }
        let roots = [
            pointer(payload, modifier + 16)?,
            pointer(payload, modifier + 24)?,
        ];
        for &root in &roots {
            profile(payload, root)?;
        }
        Ok(roots)
    }

    /// The template's melee container: the one `80804427` row of the template key in a melee
    /// bank, with its reciprocal pair and no attack at `+28`. Its selection is the stock one,
    /// which the emitted row's own gate may replace.
    fn melee_template(self, payload: &[u8]) -> Result<usize, String> {
        self.validate()?;
        let layout = layout(payload)?;
        if u32_at(payload, layout.definition - 4)? != 0x8080_41B6
            || u32_at(payload, layout.instance - 4)? != 0x8080_41B7
        {
            return Err("Melee attack requires a melee bank".into());
        }
        let mut found = Vec::new();
        for i in 0..layout.definition_rows.count {
            let row = layout.definition_rows.first + i * DEFINITION_ROW_SIZE;
            if u32_at(payload, row + ROW_KEY)? != self.template_key {
                continue;
            }
            let modifier = pointer(payload, row + ROW_MODIFIER)?;
            if u32_at(payload, modifier + 4)? == ATTACK_DEFINITION_CLASS {
                found.push((row, modifier));
            }
        }
        let [(row, modifier)] = found.as_slice() else {
            return Err("Melee attack requires one template container".into());
        };
        let (row, modifier) = (*row, *modifier);
        let twin = usize::try_from(u64_at(payload, modifier + 8)?)
            .map_err(|_| "Melee attack twin exceeds this platform")?;
        if modifier < 4
            || twin < 4
            || u32_at(payload, row + ROW_MODE)? != 255
            || u32_at(payload, row + 36)? != 0
            || u32_at(payload, modifier)? != layout.owner
            || u32_at(payload, modifier - 4)? != ATTACK_INSTANCE_CLASS
            || u32_at(payload, twin)? != layout.owner
            || u32_at(payload, twin + 4)? != ATTACK_INSTANCE_CLASS
            || u32_at(payload, twin - 4)? != ATTACK_DEFINITION_CLASS
            || u64_at(payload, twin + 8)? != modifier as u64
            || i64_at(payload, modifier + 0x28)? != 0
            || u32_at(payload, modifier + 0x30)? != 0
        {
            return Err("Melee attack template has a different mode or pair".into());
        }
        Ok(modifier)
    }

    /// The two Float32 a melee container keeps ahead of its records.
    pub(super) fn prefix(self, payload: &[u8]) -> Result<[u32; 2], String> {
        let modifier = self.melee_template(payload)?;
        Ok([
            u32_at(payload, modifier + 0x10)?,
            u32_at(payload, modifier + 0x14)?,
        ])
    }

    pub(super) fn append(self, payload: &[u8], out: &mut Vec<u8>) -> Result<[usize; 2], String> {
        let roots = self.roots(payload)?;
        let originals = self.profiles(payload)?;
        let damage = self.damage.unwrap_or(originals);
        let mut appended = [0; 2];
        for lane in 0..2 {
            let root = roots[lane];
            let mut graph = Graph::read(payload, profile(payload, root)?, 0x8080_2D7B)?;
            if let Some(records) = self.controls {
                for ((class, offset), value) in CONTROLS.into_iter().zip(records[lane]) {
                    // The profile itself is the graph's first block. Its lunge and contact
                    // blocks are each the only one of their class.
                    let mut found = graph
                        .blocks
                        .iter_mut()
                        .enumerate()
                        .filter(|(index, block)| {
                            block.class == class && (class != 0x8080_2D7B || *index == 0)
                        })
                        .map(|(_, block)| block)
                        .collect::<Vec<_>>();
                    let [block] = found.as_mut_slice() else {
                        return Err("Melee attack controls have another layout".into());
                    };
                    if block.links.contains_key(&offset) {
                        return Err("Melee attack control is a reference".into());
                    }
                    block
                        .bytes
                        .get_mut(offset..offset + 4)
                        .ok_or("Melee attack controls exceed their block")?
                        .copy_from_slice(&value.to_le_bytes());
                }
            }
            let mut contacts = 0;
            for block in &mut graph.blocks {
                if block.class == 0x8080_4455 {
                    if block.count.is_some() || u32_at(&block.bytes, 40)? != originals[lane] {
                        return Err("Movement attack has ambiguous contact profiles".into());
                    }
                    put_u32(&mut block.bytes, 40, damage[lane]);
                    contacts += 1;
                }
            }
            if contacts != 1 {
                return Err("Movement attack requires one contact profile".into());
            }
            let record = open_block(out, 0x8080_4464);
            out.extend_from_slice(
                payload
                    .get(root..root + 112)
                    .ok_or("Movement attack extent")?,
            );
            let header = open_block(out, ARRAY_MARKER);
            out.resize(header + 16, 0);
            put_u64(out, header, 1);
            put_u32(out, header + 8, 0x8080_2D7B);
            let profile_at = out.len();
            out.extend_from_slice(&graph.emit()?);
            if Graph::read(out, profile_at, 0x8080_2D7B)? != graph {
                return Err("Movement attack changed during relocation".into());
            }
            let selector = open_block(out, ARRAY_MARKER);
            out.resize(selector + 24, 0);
            put_u64(out, selector, 1);
            put_u32(out, selector + 8, 0x8080_2EAE);
            put_pointer(out, selector + 16, profile_at)?;
            put_pointer(out, record + 80, header)?;
            put_pointer(out, record + 96, selector)?;
            // The secondary selector is an interior pointer into the profile array.
            // It cannot be captured as a separately marked Graph allocation.
            if profile(out, record)? != profile_at {
                return Err("Movement attack selector changed".into());
            }
            appended[lane] = record;
        }
        Ok(appended)
    }
}

fn profile(payload: &[u8], root: usize) -> Result<usize, String> {
    // A record's word at +68 is 1 on melee records and zero on movement ones. It is not a
    // reference, so the copied record keeps it as it is.
    if root < 4
        || u32_at(payload, root - 4)? != 0x8080_4464
        || u64_at(payload, root + 8)? != 0
        || u64_at(payload, root + 16)? != 0
        || u64_at(payload, root + 24)? != 0
        || u64_at(payload, root + 48)? != 0
        || u32_at(payload, root + 104)? > 1
        || u32_at(payload, root + 108)? != 0
    {
        return Err("Movement attack root has unqualified references".into());
    }
    let profiles = rows(payload, root + 72, 0x8080_2D7B, "attack profile")?;
    let selectors = rows(payload, root + 88, 0x8080_2EAE, "attack selector")?;
    if profiles.count != 1
        || selectors.count != 1
        || pointer(payload, selectors.first)? != profiles.first
        || profiles
            .first
            .checked_add(544)
            .is_none_or(|end| end > payload.len())
    {
        return Err("Movement attack requires one selected profile".into());
    }
    Ok(profiles.first)
}
