//! Native asset labels and search summaries shared by reading surfaces.
use crate::sandbox_perk::projectile;
/// One catalog entry prepared for the picker: its label, detail line and search text.
pub struct AssetChoice {
    pub index: usize,
    pub name: String,
    pub search: String,
}

pub(super) fn asset_choices(catalog: &projectile::catalog::Catalog) -> Vec<AssetChoice> {
    let mut rows = catalog
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let name = entry.discovery_label_with(|_| None, |_| None);
            let detail = format!(
                "{} · {} · 0x{:08X}",
                entry.kind_label(),
                entry.package,
                entry.graph
            );
            let search = format!(
                "{name} {detail} {} {}",
                entry
                    .native_paths
                    .iter()
                    .chain(entry.native_name.iter())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" "),
                entry
                    .contexts
                    .iter()
                    .map(|context| context.path.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_lowercase();
            AssetChoice {
                index,
                name,
                search,
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by_cached_key(|row| {
        (
            catalog.entries[row.index].label_rank(),
            row.name.to_lowercase(),
            row.index,
        )
    });
    rows
}

/// Preserve the native spelling and distinguish a source context from an asset's own name.
pub fn technical_name(entry: &projectile::catalog::Entry) -> String {
    let useful =
        |name: &&String| !name.is_empty() && !name.to_ascii_lowercase().contains("label_globals");
    let direct = entry
        .native_paths
        .iter()
        .chain(entry.native_name.iter())
        .find(useful);
    let source = entry
        .contexts
        .iter()
        .filter(|context| useful(&&context.path))
        .filter(|context| context.name_evidence.is_some() || context.path.ends_with(".tft"))
        .min_by_key(|context| context.depth);
    if let Some((name, prefix)) = direct
        .map(|name| (name.as_str(), ""))
        .or_else(|| source.map(|context| (context.path.as_str(), "Source: ")))
    {
        format!(
            "{prefix}{} · 0x{:08X}",
            crate::package_runtime::tft::asset_label(name),
            entry.graph
        )
    } else {
        format!("0x{:08X}", entry.graph)
    }
}
