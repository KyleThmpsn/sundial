//! Bind clip slots through both rigs' explicit control tables, never through bone ordinals alone.
use super::*;

pub(super) fn owner(rig: &Value, first_person: bool, class: u32, source: bool) -> Result<u32> {
    optional_owner(rig, first_person, class, source)?
        .with_context(|| format!("Animation role has no unique {class:08X} owner"))
}

pub(super) fn optional_owner(
    rig: &Value,
    first_person: bool,
    class: u32,
    source: bool,
) -> Result<Option<u32>> {
    let root = u32::from_str_radix(
        rig["runtime_entity"].as_str().context("Runtime entity")?,
        16,
    )?;
    let tag = |v: &Value| -> Result<u32> {
        if source {
            Ok(u32::try_from(v.as_u64().context("Source component tag")?)?)
        } else {
            Ok(u32::from_str_radix(
                v.as_str().context("Native component tag")?,
                16,
            )?)
        }
    };
    let mut matches = Vec::new();
    for c in rig["components"].as_array().context("Runtime components")? {
        if (tag(&c["entity"])? != root) == first_person && tag(&c["class"])? == class {
            matches.push(tag(&c["owner"])?);
        }
    }
    ensure!(
        matches.len() <= 1,
        "Animation role has multiple {class:08X} owners"
    );
    Ok(matches.first().copied())
}

fn groups(p: &Payload, field: usize, source: bool) -> Result<Vec<(u32, u32)>> {
    p.array(field, 8, Some(if source { 0x80800380 } else { 0x8080907F }))?
        .into_iter()
        .map(|at| Ok((p.u32(at)?, p.u32(at + 4)?)))
        .collect()
}

pub(super) struct Mapping {
    map: Vec<u16>,
    pub bone_map: Vec<u16>,
    bones: usize,
    groups: Vec<(u32, u32)>,
    target: Payload,
    defaults: Vec<[f32; 8]>,
    source_defaults: Vec<[f32; 8]>,
    first_person: bool,
    source_slots: Vec<u16>,
}

impl Mapping {
    pub fn read(
        source: &Source,
        runtime: &Value,
        native: &mut Reader,
        rig: &Value,
        first_person: bool,
    ) -> Result<Self> {
        let from = source.read(owner(runtime, first_person, 0x8080099B, true)?)?;
        let to = if let Some(file) = rig["owned_controls"][first_person.to_string()].as_str() {
            std::sync::Arc::new(Payload(fs::read(file)?))
        } else {
            native.tag(
                owner(rig, first_person, 0x80808F8F, false)?,
                Some(0x80809C36),
            )?
        };
        crate::tiger::animation::rig::controls::validate(&to.0)?;
        let skeleton = |r: &Value| -> Result<Vec<Value>> {
            let matches = r["skeletons"]
                .as_array()
                .context("Animation skeletons")?
                .iter()
                .filter(|s| (s["entity"] != r["runtime_entity"]) == first_person)
                .collect::<Vec<_>>();
            ensure!(matches.len() == 1, "Animation role skeleton is ambiguous");
            Ok(matches[0]["bones"]
                .as_array()
                .context("Animation bones")?
                .clone())
        };
        let sb = skeleton(runtime)?;
        let nb = skeleton(rig)?;
        let mut bone_map = Vec::new();
        for b in &sb {
            let matches = nb
                .iter()
                .enumerate()
                .filter(|(_, n)| n["name_hash"] == b["name_hash"])
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "Animation bone identity is missing or ambiguous"
            );
            bone_map.push(u16::try_from(matches[0])?);
        }
        let sr = from.pointer(24)?;
        let nr = to.pointer(24)?;
        ensure!(
            from.u32(sr - 4)? == 0x8080099B,
            "Source control resource differs"
        );
        let source_groups = groups(&from, sr + 0x90, true)?;
        let target_groups = groups(&to, nr + 0x90, false)?;
        let sf = from.array(sr + 0xD8, 2, Some(0x80800006))?;
        let si = from.array(sr + 0xE8, 2, Some(0x80800006))?;
        let nf = to.array(nr + 0xD8, 2, Some(0x80800006))?;
        let ni = to.array(nr + 0xE8, 2, Some(0x80800006))?;
        let so = from.array(sr + 0xA0, 48, Some(0x80800638))?;
        let no = to.array(nr + 0xA0, 52, Some(0x80809076))?;
        let sp = from.array(sr + 0xB0, 32, Some(0x808001B0))?;
        let np = to.array(nr + 0xB0, 32, Some(0x80809F75))?;
        ensure!(
            sf.len() == sb.len()
                && nf.len() == nb.len()
                && si.len() == so.len()
                && sp.len() == so.len()
                && source_groups.iter().map(|g| u64::from(g.1)).sum::<u64>() == so.len() as u64
                && source_groups.len() == target_groups.len(),
            "Animation control dimensions disagree"
        );
        let mapped_bone = |value: u16| -> Result<u16> {
            if value == u16::MAX {
                Ok(value)
            } else {
                bone_map
                    .get(usize::from(value))
                    .copied()
                    .context("Control bone exceeds source rig")
            }
        };
        let mut map = Vec::new();
        for (slot, &at) in si.iter().enumerate() {
            let bone = from.u16(at)?;
            ensure!(
                from.u16(*sf.get(usize::from(bone)).context("Source slot bone")?)? as usize == slot,
                "Source control lookup is not bidirectional"
            );
            let native_bone = mapped_bone(bone)?;
            let target = to.u16(nf[usize::from(native_bone)])?;
            ensure!(
                target != u16::MAX && to.u16(ni[usize::from(target)])? == native_bone,
                "Mapped bone has no native control slot"
            );
            let mut operation = from.bytes::<48>(so[slot])?;
            for offset in [0, 6, 20, 26, 40] {
                let value = mapped_bone(from.u16(so[slot] + offset)?)?;
                operation[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            }
            ensure!(
                operation == to.bytes::<48>(no[usize::from(target)] + 4)?,
                "Source and native control operations differ"
            );
            map.push(target);
        }
        ensure!(
            map.iter().collect::<BTreeSet<_>>().len() == map.len(),
            "Animation control slots alias"
        );
        let (mut sa, mut na) = (0usize, 0usize);
        for (s, n) in source_groups.iter().zip(&target_groups) {
            let se = sa + usize::try_from(s.1)?;
            let ne = na + usize::try_from(n.1)?;
            ensure!(
                map.get(sa..se)
                    .context("Source control group extent")?
                    .iter()
                    .all(|&i| (na..ne).contains(&usize::from(i))),
                "Animation slot mapping crosses control groups"
            );
            sa = se;
            na = ne;
        }
        let source_defaults = sp
            .into_iter()
            .map(|at| {
                let mut pose = [0.; 8];
                for (i, value) in pose.iter_mut().enumerate() {
                    *value = from.f32(at + i * 4)?;
                }
                crate::tiger::animation::pose::Pose::checked(pose)?;
                Ok(pose)
            })
            .collect::<Result<Vec<_>>>()?;
        let defaults = np
            .into_iter()
            .map(|at| {
                let mut result = [0.; 8];
                for (i, v) in result.iter_mut().enumerate() {
                    *v = to.f32(at + i * 4)?;
                }
                Ok(result)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            map,
            bone_map,
            bones: nb.len(),
            groups: source_groups,
            target: to.as_ref().clone(),
            defaults,
            source_defaults,
            first_person,
            source_slots: si.iter().map(|&r| from.u16(r)).collect::<Result<_>>()?,
        })
    }

    pub fn apply(&self, source: &Payload, converted: &Payload) -> Result<(Payload, Value)> {
        let source_groups = groups(source, 0x150, true)?;
        let default_policy = if converted.u8(0x181)? == 0 {
            "source_absolute_channels"
        } else {
            "authored_additive_or_sparse_masks"
        };
        if self.first_person
            && (usize::from(source.u8(0x12A)?) != self.bone_map.len()
                || source_groups != self.groups)
        {
            let target_groups = groups(&self.target, self.target.pointer(24)? + 0x90, false)?;
            let matched = source_groups
                .iter()
                .zip(&self.groups)
                .zip(&target_groups)
                .take_while(|((clip, own), native)| clip == own && clip == native)
                .count();
            let common = source_groups[..matched]
                .iter()
                .map(|g| g.1 as usize)
                .sum::<usize>();
            ensure!(
                common > 0
                    && converted.u8(0x181)? <= 2
                    && source_groups
                        .get(matched)
                        .zip(self.groups.get(matched))
                        .is_some_and(|(clip, own)| clip.0 != own.0)
                    && source_groups
                        .get(matched)
                        .zip(target_groups.get(matched))
                        .is_some_and(|(clip, native)| clip.0 != native.0)
                    && common < source.u8(0x12B)? as usize
                    && source_groups.iter().map(|g| g.1 as usize).sum::<usize>()
                        == source.u8(0x12B)? as usize
                    && self.map.get(..common).is_some_and(|map| map
                        .iter()
                        .enumerate()
                        .all(|(slot, &target)| slot == usize::from(target))),
                "Borrowed source clip has no identical leading control groups"
            );
            let bones = usize::from(
                *self
                    .source_slots
                    .get(..common)
                    .context("Borrowed arm slots")?
                    .iter()
                    .max()
                    .context("Borrowed arm bones")?,
            ) + 1;
            ensure!(
                self.bone_map.get(..bones).is_some_and(|map| map
                    .iter()
                    .enumerate()
                    .all(|(bone, &target)| bone == usize::from(target))),
                "Borrowed source clip remaps shared arm bones"
            );
            if source.u64(0x30)? != 0 {
                for row in source.array(source.pointer(0x30)?, 64, Some(0x8080081F))? {
                    for offset in [0, 2, 16] {
                        let bone = source.u16(row + offset)?;
                        ensure!(
                            bone == u16::MAX || usize::from(bone) < bones,
                            "Borrowed source constraint addresses its foreign weapon"
                        );
                    }
                }
            }
            // The original mixer stops at the first differing group. Keep the
            // foreign suffix and its encoded samples intact, outside that span.
            let payload = if converted.u8(0x181)? == 0 {
                crate::tiger::animation::clip::bind_defaults_prefix(
                    converted,
                    &self.source_defaults[..common],
                )?
            } else {
                converted.clone()
            };
            return Ok((
                payload,
                json!({"shared_controls":common,"shared_bones":bones,
                "control_mapping":"identical_leading_groups","foreign_weapon_controls":"excluded_by_native_group_matching",
                "default_poses":default_policy}),
            ));
        }
        ensure!(
            usize::from(source.u8(0x12A)?) == self.bone_map.len() && source_groups == self.groups,
            "Source clip uses another control rig"
        );
        // Absent absolute channels use the source controller's pose. Additive
        // deltas and sparse replacements must retain their authored masks.
        let bound = if converted.u8(0x181)? == 0 {
            crate::tiger::animation::clip::bind_defaults(converted, &self.source_defaults)?
        } else {
            converted.clone()
        };
        let payload = crate::tiger::animation::clip::remap_controls(
            &bound,
            &self.map,
            self.bones,
            &self.target,
            &self.defaults,
        )?;
        Ok((
            payload,
            json!({"slot_map":self.map,"source_bones":self.bone_map.len(),"native_bones":self.bones,
            "native_slots":self.defaults.len(),"control_operations":"mapped_equal","default_poses":default_policy}),
        ))
    }
}
