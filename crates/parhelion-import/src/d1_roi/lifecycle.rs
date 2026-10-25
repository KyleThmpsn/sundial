//! Explicit lifecycle translation preserves embedded targets, order and cleanup-only rows.
use crate::tiger::{entity::lifecycle as native, payload::Payload};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Method {
    pub target: usize,
    pub class: u32,
    pub implementation: u32,
    pub constructor: u16,
    pub cleanup: u16,
}

#[derive(Debug, Serialize)]
pub struct Lifecycle {
    pub methods: Vec<Method>,
}

impl Lifecycle {
    pub fn read(owner: &Payload) -> Result<Self> {
        ensure!(
            owner.u64(0)? == owner.0.len() as u64,
            "Source lifecycle owner size differs"
        );
        let runtime = owner.pointer(16)?;
        let boundary = owner.pointer(24)?;
        let mut methods = Vec::new();
        for row in owner.array(0x30, 16, Some(0x80800930))? {
            let target = owner.pointer(row)?;
            ensure!(
                target >= runtime && target.checked_add(32).is_some_and(|end| end <= boundary),
                "Source lifecycle target exceeds the copied component instance"
            );
            let definition = usize::try_from(owner.u64(target + 8)?)?;
            ensure!(
                owner.u64(target)? == owner.u64(runtime)?
                    && owner.u64(definition)? == owner.u64(runtime)?
                    && owner.u64(definition + 8)? == target as u64
                    && owner.u64(target + 16)? <= u32::MAX as u64
                    && owner.u64(definition + 16)? <= u32::MAX as u64,
                "Source lifecycle target is not a reciprocal typed object"
            );
            let method = Method {
                target,
                class: owner.u32(definition + 16)?,
                implementation: owner.u32(row + 8)?,
                constructor: owner.u16(row + 12)?,
                cleanup: owner.u16(row + 14)?,
            };
            ensure!(
                method.constructor != u16::MAX || method.cleanup != u16::MAX,
                "Source lifecycle record has no methods"
            );
            methods.push(method);
        }
        Ok(Self { methods })
    }

    /// Mappings are consumer-qualified contracts, not method-number copies or class-name guesses.
    /// Every source record participates, even when construction itself is skipped.
    pub fn write(
        &self,
        owner: &mut Payload,
        mappings: &BTreeMap<Method, native::Method>,
        mutable_end: usize,
    ) -> Result<()> {
        self.write_expanded(
            owner,
            &mappings
                .iter()
                .map(|(&source, &target)| (source, vec![target]))
                .collect(),
            mutable_end,
        )
    }

    /// A native implementation can move construction to a containing object while retaining
    /// cleanup on its child. Every source call still occurs exactly once in the qualified plan.
    pub fn write_expanded(
        &self,
        owner: &mut Payload,
        mappings: &BTreeMap<Method, Vec<native::Method>>,
        mutable_end: usize,
    ) -> Result<()> {
        let required = self.methods.iter().copied().collect::<BTreeSet<_>>();
        ensure!(
            required == mappings.keys().copied().collect(),
            "Lifecycle mappings do not cover exactly the source methods"
        );
        let mut methods = Vec::new();
        for source in &self.methods {
            let targets = mappings
                .get(source)
                .context("Missing native lifecycle mapping")?;
            ensure!(
                !targets.is_empty()
                    && targets
                        .iter()
                        .filter(|target| target.constructor != u16::MAX)
                        .count()
                        == usize::from(source.constructor != u16::MAX)
                    && targets
                        .iter()
                        .filter(|target| target.cleanup != u16::MAX)
                        .count()
                        == usize::from(source.cleanup != u16::MAX),
                "Lifecycle translation drops or invents a constructor or cleanup call"
            );
            methods.extend_from_slice(targets);
        }
        native::write(owner, &methods, mutable_end)
    }
}

/// The registered D1 sequence constructor/cleanup methods 1/3 correspond to native 0/2.
/// A checked template can already have those native rows. Rebuild them through the shared
/// serializer so component assembly does not inherit an unrelated lifecycle table.
pub fn sequence(source: &Payload, target: &mut Payload) -> Result<()> {
    let lifecycle = Lifecycle::read(source)?;
    let runtime = source.pointer(16)?;
    let expected = Method {
        target: runtime,
        class: 0x8080079A,
        implementation: 0x8080079A,
        constructor: 1,
        cleanup: 3,
    };
    ensure!(
        lifecycle.methods == [expected],
        "Source sequence lifecycle differs"
    );
    let native_runtime = target.pointer(16)?;
    ensure!(
        target.u32(native_runtime - 4)? == 0x808084D7,
        "Native sequence lifecycle root differs"
    );
    let method = native::Method {
        target: native_runtime,
        implementation: 0x808084D7,
        constructor: 0,
        cleanup: 2,
    };
    let existing = native::read(target)?;
    ensure!(
        existing.is_empty() || existing == [method],
        "Native sequence template has additional lifecycle methods"
    );
    let mut output = target.clone();
    output.0[0x30..0x40].fill(0);
    let mutable_end = output.0.len();
    lifecycle.write(
        &mut output,
        &BTreeMap::from([(expected, method)]),
        mutable_end,
    )?;
    *target = output;
    Ok(())
}
