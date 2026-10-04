//! Private equipment patterns for imported Ghost shells, ships and Sparrows.
use super::*;

pub(in crate::item) fn source(
    sources: &ProjectSources,
    spec: &WeaponCloneSpec,
    definition: &[u8],
) -> AuthoringResult<Option<ResolvedSandboxPatternSource>> {
    if !imported(spec)
        || !matches!(
            spec.kind,
            ItemKind::GhostShell | ItemKind::Ship | ItemKind::Sparrow
        )
    {
        return Ok(None);
    }
    let index = pattern(definition)?
        .ok_or_else(|| invalid("Imported equipment has no native runtime pattern"))?;
    sandbox_pattern_source_at(&sources.stock_sandbox_patterns, index).map(Some)
}

pub(in crate::item) fn pattern(definition: &[u8]) -> AuthoringResult<Option<u16>> {
    let root = shader_translation_root(definition)?;
    let index = read_u16(definition, root + TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET)?;
    Ok((index != u16::MAX).then_some(index))
}

pub(in crate::item) fn set_pattern(definition: &mut [u8], index: u16) -> AuthoringResult<()> {
    if index == u16::MAX || pattern(definition)?.is_none() {
        return Err(invalid(
            "Imported equipment requires an active native pattern selector",
        ));
    }
    let root = shader_translation_root(definition)?;
    write_u16(
        definition,
        root + TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET,
        index,
    )?;
    if pattern(definition)? != Some(index) {
        return Err(validation(
            "Imported equipment did not retain its private pattern selector",
        ));
    }
    Ok(())
}
