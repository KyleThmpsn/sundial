//! Resolve pattern-owned components and the selected first-person skeleton.
use super::*;

impl Source {
    pub(in super::super) fn runtime_assignments(
        &self,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<BTreeMap<u32, u32>> {
        let root_tag = self.named("investment_assets")?;
        let root = self.read(root_tag)?;
        let tag = root.u32(0x10)?;
        ensure!(
            self.reference(tag)? == 0x80800341,
            "Source runtime assignment root differs"
        );
        let map = self.read(tag)?;
        let mut result = BTreeMap::new();
        for row in map.array(8, 8, None)? {
            ensure!(
                result.insert(map.u32(row)?, map.u32(row + 4)?).is_none(),
                "Duplicate source runtime assignment"
            );
        }
        assets.insert(root_tag, root);
        assets.insert(tag, map);
        Ok(result)
    }

    pub(in super::super) fn weapon_runtime(
        &self,
        item: &Weapon,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Value> {
        let (tag, table) = self.table(0x8080339A)?;
        let rows = table.array(8, 32, Some(0x808033BC))?;
        let row = *rows
            .get(usize::from(item.pattern))
            .context("Source weapon pattern is missing")?;
        let key = table.u32(row)?;
        let content = table.u32(row + 4)?;
        let assignments = self.runtime_assignments(assets)?;
        let entity = *assignments
            .get(&key)
            .context("Source weapon runtime is unassigned")?;
        assets.insert(tag, table);
        let mut pending = vec![entity];
        let mut visited = BTreeSet::new();
        let mut components = Vec::new();
        let mut skeletons = Vec::new();
        while let Some(entity) = pending.pop() {
            if !visited.insert(entity) {
                continue;
            }
            ensure!(
                visited.len() <= 16,
                "Source weapon attachment recursion exceeds its bound"
            );
            ensure!(
                self.reference(entity)? == 0x80800734,
                "Source runtime is not an entity"
            );
            let data = self.read(entity)?;
            for row in data.array(0x20, 12, Some(0x80800715))? {
                let tag = data.u32(row)?;
                ensure!(
                    self.reference(tag)? == 0x80800861,
                    "Source runtime component type differs"
                );
                let owner = self.read(tag)?;
                let resource = owner.pointer(24)?;
                let class = owner.u32(resource - 4)?;
                components.push(json!({"entity":entity,"owner":tag,"class":class}));
                if class == 0x8080049A {
                    let bones = owner.array(resource + 0x98, 16, Some(0x808004F4))?;
                    let inverse = owner.array(resource + 0xB8, 32, Some(0x808001B0))?;
                    ensure!(
                        !bones.is_empty() && bones.len() == inverse.len(),
                        "Source skeleton arrays differ"
                    );
                    let entries = bones.into_iter().zip(inverse).enumerate().map(|(i,(bone,transform))| {
                        let parent = i32::from_le_bytes(owner.bytes::<4>(bone+4)?);
                        ensure!(parent == -1 || (parent >= 0 && (parent as usize) < i), "Source bone parent is not earlier in its palette");
                        Ok(json!({"index":i,"name_hash":format!("{:08X}",owner.u32(bone)?),"parent":parent,
                            "inverse_object_transform":(0..8).map(|j|owner.f32(transform+j*4)).collect::<Result<Vec<_>>>()?}))
                    }).collect::<Result<Vec<_>>>()?;
                    skeletons.push(json!({"entity":format!("{entity:08X}"),"owner":format!("{tag:08X}"),"bones":entries}));
                }
                if class == 0x80802580 {
                    let mut selected = owner
                        .array(resource + 0x5D8, 0xD8, Some(0x808031DE))?
                        .into_iter()
                        .filter(|r| owner.u32(*r).ok() == Some(content))
                        .collect::<Vec<_>>();
                    if selected.is_empty() {
                        selected.push(resource + 0x500);
                    }
                    ensure!(
                        selected.len() == 1,
                        "Ambiguous source first-person attachment"
                    );
                    let target = owner.u32(selected[0] + 0x68)?;
                    if !matches!(target, 0 | u32::MAX | 0x811C9DC5) {
                        pending.push(target);
                    }
                }
                assets.insert(tag, owner);
            }
            assets.insert(entity, data);
        }
        ensure!(
            !skeletons.is_empty(),
            "Source weapon has no readable skeleton"
        );
        Ok(
            json!({"pattern_index":item.pattern,"pattern_key":key,"content_key":content,
            "runtime_entity":format!("{entity:08X}"),"components":components,"skeletons":skeletons,
            "gameplay_verified":false}),
        )
    }
}
