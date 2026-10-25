//! Empty optic inputs inherit the source weapon's default, not the native base's.
use super::*;

impl Assembly<'_> {
    pub(super) fn inherited_aim(&self, marker: u32) -> Result<(u32, [u8; 64])> {
        let root = self.source.read(
            self.weapon_runtime
                .context("Source optic needs a weapon runtime")?,
        )?;
        let components = root.array(0x20, 12, Some(0x80800715))?;
        let mut found = Vec::new();
        for at in root.array(0x30, 88, Some(0x8080054E))? {
            if root.u64(at + 24)? != 0x8080073D {
                continue;
            }
            let tag = u32::try_from(root.u64(at + 8)?)?;
            let input = usize::try_from(root.u64(at + 16)?)?;
            let owner = self.source.read(tag)?;
            if owner.u32(input + 0x3C)? != marker {
                continue;
            }
            let component = usize::try_from(root.u64(at + 32)?)?;
            ensure!(
                components
                    .get(component)
                    .is_some_and(|&row| root.u32(row).ok() == Some(tag)),
                "Inherited aim owner is not a root component"
            );
            ensure!(
                root.u64(at)? == 0x811C9DC5
                    && root.u64(at + 40)? == 0x811C9DC5
                    && root.u64(at + 64)? == 0x808003A6
                    && root.u64(at + 80)? == 0,
                "Inherited aim requires a different movement provider"
            );
            let selector = usize::try_from(root.u64(at + 72)?)?;
            let provider = u32::try_from(root.u64(at + 48)?)?;
            ensure!(
                components
                    .get(selector)
                    .is_some_and(|&row| root.u32(row).ok() == Some(provider)),
                "Inherited aim provider is not a root component"
            );
            let skeleton = self.source.read(provider)?;
            let definition = skeleton.pointer(24)?;
            ensure!(
                skeleton.u32(definition - 4)? == 0x8080049A
                    && root.u64(at + 56)? == (definition + 0x60) as u64,
                "Inherited aim provider is not the root skeleton"
            );
            let bones = skeleton.array(definition + 0x98, 16, Some(0x808004F4))?;
            let inverse = skeleton.array(definition + 0xB8, 32, Some(0x808001B0))?;
            let bone = *bones.first().context("Inherited aim root bone")?;
            let transform = *inverse.first().context("Inherited aim root transform")?;
            ensure!(
                skeleton.u32(bone + 4)? == u32::MAX,
                "Inherited aim root bone has a parent"
            );
            for (i, expected) in [0., 0., 0., 1., 0., 0., 0., 1.].into_iter().enumerate() {
                ensure!(
                    (skeleton.f32(transform + i * 4)? - expected).abs() < 1e-6,
                    "Inherited aim root transform is not identity"
                );
            }
            ensure!(
                owner.u64(input)? == u64::from(tag)
                    && owner.u64(input + 0x10)? == 0x80800862
                    && owner.u64(input + 0x18)? == 0
                    && owner.u64(input + 0x20)? == 0x8080052A
                    && owner.u32(input + 0x28)? == 1
                    && owner.u32(input + 0x2C)? == u32::MAX
                    && owner.u64(input + 0x30)? == 0
                    && owner.u32(input + 0x38)? == 0x80800608
                    && owner.u64(input + 0x40)? == 1,
                "Inherited aim input layout differs"
            );
            let rows = owner.array(input + 0x48, 64, Some(0x80800608))?;
            ensure!(rows.len() == 1, "Inherited aim needs one default row");
            let row = rows[0];
            ensure!(
                owner.bytes::<16>(row)? == [0; 16] && owner.u32(row + 48)? == marker,
                "Inherited aim needs an object-space default"
            );
            for offset in (16..48).step_by(4) {
                ensure!(
                    owner.f32(row + offset)?.is_finite(),
                    "Inherited aim contains a nonfinite transform"
                );
            }
            found.push((tag, owner.bytes::<64>(row)?));
        }
        ensure!(
            found.len() == 1,
            "Inherited source aim point is absent or ambiguous"
        );
        Ok(found[0])
    }
}
