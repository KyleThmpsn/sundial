//! Resolve ability-event operands through stable ability identities in both builds.
use super::*;

pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    p: &Payload,
    at: usize,
) -> Result<NativeNode> {
    ensure!(
        p.u32(at - 4)? == 0x808031BF
            && p.u8(at + 5)? == 10
            && (0.0..=1.0).contains(&p.f32(at)?)
            && p.u8(at + 4)? == 255
            && p.u8(at + 6)? == 0
            && p.u32(at + 20)? == 0,
        "Source ability-reference condition has unqualified state or metadata"
    );
    if p.u64(at + 8)? != 0 {
        let path = p.pointer(at + 8)?;
        ensure!(
            p.0[path..].contains(&0),
            "Source ability-reference path is unterminated"
        );
    }
    let pattern = p.u32(at + 16)?;
    ensure!(
        source.reference(pattern)? == 0x80800734,
        "Source ability-reference condition does not select a pattern"
    );
    source.read(pattern)?;
    let assignments = source.runtime_assignments(&mut BTreeMap::new())?;
    let tags = source.classes(0x80803432);
    ensure!(
        tags.len() == 1,
        "Source ability definitions are missing or ambiguous"
    );
    let definitions = source.read(tags[0])?;
    let mut identities = BTreeSet::new();
    let mut matched = None;
    for row in definitions.array(8, 40, Some(0x8080343C))? {
        let identity = definitions.u32(row)?;
        ensure!(
            identities.insert(identity),
            "Duplicate source ability identity"
        );
        if assignments.get(&identity) == Some(&pattern) {
            ensure!(
                matched.replace(identity).is_none(),
                "Source ability pattern has multiple identities"
            );
        }
    }
    let identity = matched.context("Source event pattern has no ability identity")?;
    let tags = native.classes(0x80805CA4);
    ensure!(
        tags.len() == 1,
        "Native ability definitions are missing or ambiguous"
    );
    let definitions = native.tag(tags[0], Some(0x80805CA4))?;
    let mut identities = BTreeSet::new();
    for row in definitions.array(8, 28, Some(0x80805CAA))? {
        ensure!(
            identities.insert(definitions.u32(row + 4)?),
            "Duplicate native ability identity"
        );
    }
    ensure!(
        identities.contains(&identity),
        "Source event ability {identity:08X} has no native definition"
    );
    let roots = native
        .manager
        .lookup
        .named_tags
        .iter()
        .filter(|tag| tag.name == "investment_assets")
        .map(|tag| tag.hash.0)
        .collect::<BTreeSet<_>>();
    ensure!(
        roots.len() == 1,
        "Native investment assets are missing or ambiguous"
    );
    let root = native.tag(*roots.first().context("Native investment assets")?, None)?;
    let assignments = native.tag(root.u32(16)?, Some(0x80809780))?;
    let mut target = None;
    for row in assignments.array(8, 8, None)? {
        if assignments.u32(row)? == identity {
            ensure!(
                target.replace(assignments.u32(row + 4)?).is_none(),
                "Native event ability has multiple runtime assignments"
            );
        }
    }
    let target = target.context("Native event ability has no runtime assignment")?;
    native.tag(target, Some(0x80809C0F))?;

    // Original 76AC40 and native 107C4B0 compare event word zero with the pattern
    // reference at +10. The path is authoring metadata and native stock leaves it null.
    let mut bytes = vec![0; 24];
    bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
    bytes[16..20].copy_from_slice(&target.to_le_bytes());
    Ok(NativeNode { kind: 10, bytes })
}
