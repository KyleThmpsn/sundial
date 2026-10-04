//! Apply ordinary shader authoring to private copies of the pinned source material graph.
use super::*;
use crate::dye::{DyeSurface, slot_of_key, surface_edit, texture_edit};
use crate::tag_payload::{array_at, write_bytes, write_u32};

/// Bind a checked source graph to this recipe's private registrations. Copies may share every
/// input payload while keeping distinct item identities, dye keys and material edits.
pub(super) fn bind(graph: &mut Value, spec: &WeaponCloneSpec) -> AuthoringResult<()> {
    if spec.kind != ItemKind::Shader {
        return Err(invalid("Source shader requires a shader recipe"));
    }
    graph["item_hash"] = spec.identity.item_hash.into();
    let mut keys = BTreeSet::new();
    for dye in graph["dyes"]
        .as_array_mut()
        .ok_or_else(|| invalid("Source shader channels missing"))?
    {
        let channel = dye["channel"]
            .as_u64()
            .ok_or_else(|| invalid("Source shader channel missing"))?;
        if !matches!(channel, 0..=2 | 4..=15) {
            return Err(invalid("Source shader channel unsupported"));
        }
        let key = fnv1_name_hash(&format!("parhelion/{}/dye/{channel}", spec.namespace));
        if [0, u32::MAX, 0x811C9DC5].contains(&key) || !keys.insert(key) {
            return Err(invalid("Source shader dye identity collision"));
        }
        dye["manifest"] = key.into();
    }
    Ok(())
}

pub(super) fn edit(
    nodes: &mut [linking::Node],
    graph: &Value,
    spec: &WeaponCloneSpec,
    manager: &sundial::package_authoring::PackageManager,
) -> AuthoringResult<BTreeMap<String, Vec<TagHash>>> {
    let mut borrowed = BTreeMap::new();
    for dye in graph["dyes"]
        .as_array()
        .ok_or_else(|| invalid("Source shader channels missing"))?
    {
        let key = i8::try_from(
            dye["channel"]
                .as_i64()
                .ok_or_else(|| invalid("Source shader channel missing"))?,
        )
        .map_err(|e| invalid(e.to_string()))?;
        let (gear, channel) =
            slot_of_key(key).ok_or_else(|| invalid("Source shader channel unsupported"))?;
        let name = format!("dye-{key}-scope");
        let at = nodes
            .iter()
            .position(|n| n.symbol == name)
            .ok_or_else(|| invalid("Source shader scope missing"))?;
        crate::shader::normalize_scope(graph, &mut nodes[at].payload).map_err(invalid)?;
        sync_constants(nodes, at, key)?;
        if spec.overrides.render_dye_rows.as_ref().is_some_and(|rows| {
            !rows
                .iter()
                .flatten()
                .any(|r| crate::shader::source_channel(r.dye_reference_index) == Some(key))
        }) {
            continue;
        }
        let edits = &spec.overrides;
        let writes = DyeSurface::ALL
            .into_iter()
            .filter_map(|s| surface_edit(&edits.dye_edits, gear, channel, s))
            .flat_map(|e| e.writes())
            .chain(
                texture_edit(&edits.dye_texture_edits, gear, channel)
                    .into_iter()
                    .flat_map(|e| e.writes()),
            )
            .collect::<Vec<_>>();
        crate::shader::edits::apply(&mut nodes[at].payload, &writes)?;
        if !writes.is_empty() {
            let constants = nodes
                .iter_mut()
                .find(|n| n.symbol == format!("dye-{key}-constants"))
                .ok_or_else(|| invalid("Source shader constant buffer missing"))?;
            if constants.payload.len() != 27 * 16 {
                return Err(invalid("Source shader constant buffer layout differs"));
            }
            for &(vector, lane, value) in &writes {
                write_bytes(
                    &mut constants.payload,
                    vector * 16 + lane * 4,
                    &value.to_le_bytes(),
                )?;
            }
        }
        if let Some(edit) = texture_edit(&edits.dye_texture_edits, gear, channel) {
            let scope = &mut nodes[at];
            for (index, tag) in [edit.detail, edit.normal].into_iter().enumerate() {
                let Some(tag) = tag else {
                    continue;
                };
                let original = format!("dye-{key}-texture-{index}");
                let offset = scope
                    .patches
                    .iter()
                    .find(|(_, s)| s == &original)
                    .map(|(o, _)| *o)
                    .ok_or_else(|| invalid("Source shader texture binding missing"))?;
                scope.patches.retain(|(o, _)| *o != offset);
                if let Some(symbol) = crate::shader::texture_symbol(tag) {
                    scope.patch(offset, symbol)?;
                } else {
                    let header = TagHash(tag);
                    let entry = manager
                        .get_entry(header)
                        .filter(|e| e.file_type == 32 && matches!(e.file_subtype, 1..=3))
                        .ok_or_else(|| invalid("Shader texture is not an installed texture"))?;
                    let required = borrowed.entry(name.clone()).or_insert_with(Vec::new);
                    required.extend([header, TagHash(entry.reference)]);
                    let payload = manager
                        .read_tag(header)
                        .map_err(|e| invalid(e.to_string()))?;
                    if let Some(extra) = loading::streamed_texture_payload(&payload)? {
                        required.push(extra);
                    }
                    write_u32(&mut scope.payload, offset, tag)?;
                }
            }
        }
    }
    Ok(borrowed)
}

fn sync_constants(nodes: &mut [linking::Node], at: usize, key: i8) -> AuthoringResult<()> {
    // Old pinned graphs can gain native component mappings during normalization.
    // Keep immutable/static buffer uploads consistent with the inline initial values.
    let (count, _, rows, class) = array_at(&nodes[at].payload, 0x88)?;
    if count != 27 || class != 0x80800090 {
        return Err(invalid("Source shader initial vectors differ"));
    }
    let initial = nodes[at]
        .payload
        .get(rows..rows + count * 16)
        .ok_or_else(|| invalid("Source shader initial vectors are truncated"))?
        .to_vec();
    let constants = nodes
        .iter_mut()
        .find(|n| n.symbol == format!("dye-{key}-constants"))
        .ok_or_else(|| invalid("Source shader constant buffer missing"))?;
    if constants.payload.len() != initial.len() {
        return Err(invalid("Source shader constant buffer layout differs"));
    }
    constants.payload = initial;
    Ok(())
}
