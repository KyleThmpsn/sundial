//! Match the material identities consumed by contact selection, independent of group labels.
use super::*;
use sundial::package_authoring::sandbox_perk::program::NativeAssetResourcePatch;

struct Material {
    tag: u32,
    name: u32,
}

fn lineage(mut tag: u32, mut read: impl FnMut(u32) -> Result<Payload>) -> Result<Vec<Material>> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    while tag != u32::MAX {
        ensure!(
            tag != 0 && seen.insert(tag),
            "Projectile material ancestry is invalid"
        );
        let p = read(tag)?;
        ensure!(
            p.u64(0)? == p.0.len() as u64,
            "Projectile material extent differs"
        );
        result.push(Material {
            tag,
            name: p.u32(8)?,
        });
        tag = p.u32(12)?;
    }
    Ok(result)
}

fn fields(p: &Payload, group: usize, old: bool) -> Result<Vec<usize>> {
    let rows = p.array(
        group + if old { 0x20 } else { 0x18 },
        16,
        Some(if old { 0x808001BF } else { 0x80809FC7 }),
    )?;
    let mut fields = Vec::new();
    for row in rows {
        ensure!(
            p.u64(row)? == 0 && p.u32(row + 12)? == 0,
            "Projectile material uses an indirect path"
        );
        fields.push(row + 8);
    }
    let fallback = group + if old { 0x30 } else { 0x28 };
    ensure!(
        p.u64(fallback)? == 0,
        "Projectile fallback material uses an indirect path"
    );
    fields.push(fallback + 8);
    Ok(fields)
}

pub(super) struct Mapping {
    pub patches: Vec<NativeAssetResourcePatch>,
    pub fingerprints: Vec<Value>,
    pub groups: Vec<Value>,
    /// Native filters and fallback for each inserted source group, by source position.
    pub inserted: BTreeMap<usize, (Vec<u32>, u32)>,
}

impl Mapping {
    /// The one native physical material whose whole name ancestry equals the source filter's.
    /// An inserted group has no carrier filter to retarget, so the search covers every native
    /// material, as fusion steering filter `80A88DE1` finds `80BFDC95`.
    fn find(&mut self, source: &Source, native: &mut Reader, from: u32) -> Result<(u32, u32)> {
        if from == u32::MAX {
            return Ok((u32::MAX, u32::MAX));
        }
        let wanted = lineage(from, |tag| {
            ensure!(
                source.reference(tag)? == 0x80800519,
                "Source physical material class differs"
            );
            source.read(tag)
        })?
        .into_iter()
        .map(|m| m.name)
        .collect::<Vec<_>>();
        let mut found = Vec::new();
        for tag in native.classes(0x80808BCB) {
            let names = lineage(tag, |tag| Ok((*native.tag(tag, Some(0x80808BCB))?).clone()))?;
            if names.iter().map(|m| m.name).eq(wanted.iter().copied()) {
                found.push(names);
            }
        }
        let [found] = found.as_slice() else {
            anyhow::bail!("Inserted projectile material has no unique native counterpart");
        };
        for material in found {
            let p = native.tag(material.tag, Some(0x80808BCB))?;
            self.fingerprints.push(fingerprint(material.tag, &p));
        }
        Ok((found[0].tag, found[0].name))
    }

    fn resolve(
        &mut self,
        source: &Source,
        native: &mut Reader,
        from: u32,
        to: u32,
    ) -> Result<(u32, u32)> {
        if from == u32::MAX || to == u32::MAX {
            ensure!(from == to, "Projectile material presence differs");
            return Ok((to, u32::MAX));
        }
        let source = lineage(from, |tag| {
            ensure!(
                source.reference(tag)? == 0x80800519,
                "Source physical material class differs"
            );
            source.read(tag)
        })?;
        let target = lineage(to, |tag| {
            let p = native.tag(tag, Some(0x80808BCB))?;
            self.fingerprints.push(fingerprint(tag, &p));
            Ok((*p).clone())
        })?;
        let first = source.first().context("Empty source material ancestry")?;
        let matches = target
            .iter()
            .enumerate()
            .filter(|(_, m)| m.name == first.name)
            .collect::<Vec<_>>();
        let [(index, selected)] = matches.as_slice() else {
            anyhow::bail!("Native material has no unique source-identity ancestor");
        };
        ensure!(
            source
                .iter()
                .map(|m| m.name)
                .eq(target[*index..].iter().map(|m| m.name)),
            "Projectile material ancestry differs"
        );
        Ok((selected.tag, first.name))
    }
}

/// Whether source group `s` and carrier group `n` have materials that pair: lists of one length
/// whose carrier filters each have the source material's identity among their ancestors.
pub(super) fn pairs(
    source: &Source,
    native: &mut Reader,
    (from, to): (&graph::Owner, &graph::Owner),
    (s, n): (usize, usize),
) -> Result<bool> {
    let group = |owner: &graph::Owner, index: usize, old: bool| -> Result<usize> {
        owner
            .payload
            .array(
                owner.definition + if old { 0x220 } else { 0x218 },
                if old { 96 } else { 88 },
                None,
            )?
            .get(index)
            .copied()
            .context("Projectile material group index")
    };
    let source_fields = fields(&from.payload, group(from, s, true)?, true)?;
    let native_fields = fields(&to.payload, group(to, n, false)?, false)?;
    if source_fields.len() != native_fields.len() {
        return Ok(false);
    }
    let mut scratch = Mapping {
        patches: Vec::new(),
        fingerprints: Vec::new(),
        groups: Vec::new(),
        inserted: BTreeMap::new(),
    };
    for (sf, nf) in source_fields.into_iter().zip(native_fields) {
        let resolved = scratch.resolve(source, native, from.payload.u32(sf)?, to.payload.u32(nf)?);
        match resolved {
            Ok(_) => {}
            Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
            Err(_) => return Ok(false),
        }
    }
    Ok(true)
}

pub(super) fn prepare(
    source: &Source,
    native: &mut Reader,
    from: &graph::Owner,
    to: &graph::Owner,
    paired: &[Option<usize>],
) -> Result<Mapping> {
    let source_groups = from
        .payload
        .array(from.definition + 0x220, 96, Some(0x8080239D))?;
    let native_groups = to
        .payload
        .array(to.definition + 0x218, 88, Some(0x808037CD))?;
    ensure!(
        source_groups.len() == paired.len(),
        "Projectile material group count differs"
    );
    let mut result = Mapping {
        patches: Vec::new(),
        fingerprints: Vec::new(),
        groups: Vec::new(),
        inserted: BTreeMap::new(),
    };
    let mut unnamed = BTreeSet::new();
    for (ordinal, (s, &index)) in source_groups.into_iter().zip(paired).enumerate() {
        // A source group the carrier lacks takes its materials when it is inserted.
        let Some(index) = index else {
            let fields = fields(&from.payload, s, true)?;
            let (last, filters) = fields.split_last().context("Projectile material list")?;
            let mut tags = Vec::new();
            let mut names = Vec::new();
            for &field in filters {
                let (tag, name) = result.find(source, native, from.payload.u32(field)?)?;
                tags.push(tag);
                names.push(name);
            }
            let (fallback, name) = result.find(source, native, from.payload.u32(*last)?)?;
            names.push(name);
            result.inserted.insert(ordinal, (tags, fallback));
            result.groups.push(json!({"ordinal":ordinal,"inserted":true,
                "source_name":from.payload.u32(s + 24)?,"materials":names}));
            continue;
        };
        let n = *native_groups
            .get(index)
            .context("Paired projectile group is absent")?;
        let source_fields = fields(&from.payload, s, true)?;
        let native_fields = fields(&to.payload, n, false)?;
        ensure!(
            source_fields.len() == native_fields.len(),
            "Projectile material list differs"
        );
        let mut names = Vec::new();
        for (sf, nf) in source_fields.into_iter().zip(native_fields) {
            let before = to.payload.u32(nf)?;
            let (after, name) = result.resolve(source, native, from.payload.u32(sf)?, before)?;
            names.push(name);
            if before != after {
                result.patches.push(NativeAssetResourcePatch {
                    binding_hash: MOVEMENT,
                    resource_index: 0,
                    offset: u32::try_from(
                        nf.checked_sub(to.instance)
                            .context("Material precedes movement resource")?,
                    )?,
                    expected: before.to_le_bytes().to_vec(),
                    bytes: after.to_le_bytes().to_vec(),
                    imported_particle: None,
                });
            }
        }
        let source_name = from.payload.u32(s + 24)?;
        let native_name = to.payload.u32(n + 16)?;
        // Group pairing owns name identity. A named pair with different names is a rename.
        if source_name == 0x811C9DC5 {
            ensure!(
                names.iter().any(|n| *n != u32::MAX) && unnamed.insert(names.clone()),
                "Unnamed projectile material groups are ambiguous"
            );
        }
        result.groups.push(json!({"ordinal":ordinal,"native_ordinal":index,"source_name":source_name,"native_name":native_name,"materials":names}));
    }
    Ok(result)
}
