//! Movement attack pairs own separate profiles and selectors in the definition region.
use super::*;
use crate::sandbox_perk::action::native::Graph;

pub(super) const CLASS: u32 = 0x8080_4119;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttackModifier {
    pub template_key: u32,
    pub selection: FixedSelection,
    /// Resolved private profiles. None retains the template during structural preflight.
    pub damage: Option<[u32; 2]>,
}

impl AttackModifier {
    pub fn validate(self) -> Result<(), String> {
        let valid = |key| ![0, u32::MAX, 0x811C_9DC5].contains(&key);
        if !valid(self.template_key)
            || !valid(self.selection.required_key)
            || self.selection.name == 0
            || self.selection.priority != 255
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

    pub(super) fn append(self, payload: &[u8], out: &mut Vec<u8>) -> Result<[usize; 2], String> {
        let roots = self.roots(payload)?;
        let originals = self.profiles(payload)?;
        let damage = self.damage.unwrap_or(originals);
        let mut appended = [0; 2];
        for lane in 0..2 {
            let root = roots[lane];
            let mut graph = Graph::read(payload, profile(payload, root)?, 0x8080_2D7B)?;
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
    if root < 4
        || u32_at(payload, root - 4)? != 0x8080_4464
        || u64_at(payload, root + 8)? != 0
        || u64_at(payload, root + 16)? != 0
        || u64_at(payload, root + 24)? != 0
        || u64_at(payload, root + 48)? != 0
        || u64_at(payload, root + 104)? != 0
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
