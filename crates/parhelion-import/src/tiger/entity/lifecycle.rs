//! Ordered native constructor and cleanup records for roots and embedded component objects.
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Method {
    /// Absolute payload offset, stored as a relative pointer from the emitted row.
    pub target: usize,
    pub implementation: u32,
    pub constructor: u16,
    pub cleanup: u16,
}

impl Method {
    fn validate(&self, owner: &Payload, mutable_end: usize) -> Result<()> {
        let root = owner.pointer(16)?;
        ensure!(
            self.target >= root
                && self
                    .target
                    .checked_add(32)
                    .is_some_and(|end| end <= mutable_end)
                && mutable_end <= owner.0.len(),
            "Lifecycle target exceeds the copied component instance"
        );
        let definition = usize::try_from(owner.u64(self.target + 8)?)?;
        ensure!(
            owner.u32(self.target)? == owner.u32(root)?
                && owner.u32(definition)? == owner.u32(root)?
                && owner.u64(definition + 8)? == self.target as u64
                && sundial::package_authoring::runtime::native_type_inherits(
                    owner.u32(definition + 4)?,
                    self.implementation
                ),
            "Lifecycle implementation does not belong to its reciprocal target"
        );
        ensure!(
            [self.constructor, self.cleanup]
                .into_iter()
                .all(|selector| selector == u16::MAX || selector <= i16::MAX as u16)
                && (self.constructor != u16::MAX || self.cleanup != u16::MAX),
            "Lifecycle method selector is absent or sign-extends to a negative ordinal"
        );
        Ok(())
    }
}

pub fn read(owner: &Payload) -> Result<Vec<Method>> {
    ensure!(
        owner.u64(0)? == owner.0.len() as u64,
        "Lifecycle owner size differs"
    );
    let mut methods = Vec::new();
    for row in owner.array(0x30, 16, Some(0x808091A4))? {
        let method = Method {
            target: owner.pointer(row)?,
            implementation: owner.u32(row + 8)?,
            constructor: owner.u16(row + 12)?,
            cleanup: owner.u16(row + 14)?,
        };
        method.validate(owner, owner.pointer(24)?)?;
        methods.push(method);
    }
    Ok(methods)
}

/// Install a complete method table before sealing the component. The caller must supply
/// qualified implementation/ordinal mappings, including cleanup-only rows in original order.
/// Record layout and ancestry checks do not establish cross-version method equivalence.
pub fn write(owner: &mut Payload, methods: &[Method], mutable_end: usize) -> Result<()> {
    ensure!(
        owner.bytes::<16>(0x30)? == [0; 16],
        "Lifecycle table is already populated"
    );
    for method in methods {
        method.validate(owner, mutable_end)?;
    }
    if methods.is_empty() {
        return Ok(());
    }
    let mut output = owner.clone();
    let size = methods
        .len()
        .checked_mul(16)
        .context("Lifecycle table size overflow")?;
    append(&mut output.0, 0x30, 0x808091A4, &vec![0; size], 16)?;
    for (row, method) in output
        .array(0x30, 16, Some(0x808091A4))?
        .into_iter()
        .zip(methods)
    {
        let relative = i64::try_from(method.target)? - i64::try_from(row)?;
        put(&mut output.0, row, &relative.to_le_bytes())?;
        put(&mut output.0, row + 8, &method.implementation.to_le_bytes())?;
        put(&mut output.0, row + 12, &method.constructor.to_le_bytes())?;
        put(&mut output.0, row + 14, &method.cleanup.to_le_bytes())?;
    }
    let size = output.0.len() as u64;
    put(&mut output.0, 0, &size.to_le_bytes())?;
    *owner = output;
    Ok(())
}
