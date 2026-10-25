//! Validate attached HUD choices before planning private rows and graph copies.
use super::*;
use crate::subclass::hud::{Art, Row};
use sundial::package_authoring::{ability_hud, ability_modifier};

pub(super) fn resolve(
    sources: &Sources<'_>,
    stock: &BTreeMap<u32, StockList>,
    (namespace, place): (&str, &str),
    source: TagHash,
    edits: &[crate::subclass::AttachedAbility],
    template_icon: u16,
) -> AuthoringResult<Vec<AttachedHud>> {
    if edits.is_empty() {
        return Ok(Vec::new());
    }
    let attached =
        ability_hud::attached_glyphs(sources.manager, source.0, crate::subclass::SPAWN_DEPTH)
            .map_err(invalid)?;
    let mut planned = Vec::new();
    for edit in edits {
        let site = attached
            .iter()
            .find(|group| group.graphs.contains(&edit.graph))
            .ok_or_else(|| {
                invalid(format!(
                    "Ability {source} has no attached HUD ability 0x{:08X}",
                    edit.graph
                ))
            })?
            .site;
        let (art, artwork) = icon(sources, stock, edit.icon.as_ref())?;
        let icon = artwork
            .map(|artwork| -> AuthoringResult<_> {
                Ok(NodeIcon {
                    artwork: Some(artwork),
                    color: None,
                    source_row: template_icon,
                    source_container: (sources.icon_container)(template_icon)?,
                })
            })
            .transpose()?;
        let mut glyphs = vec![site.key];
        let entity = read_tag(sources.manager, TagHash(edit.graph), "attached ability")?;
        if let Some(bank) = ability_modifier::entity_bank(&entity).map_err(invalid)? {
            let bank = read_tag(sources.manager, TagHash(bank), "attached ability bank")?;
            for variant in ability_hud::all_variant_glyphs(&bank).map_err(invalid)? {
                if !glyphs.contains(&variant.glyph) {
                    glyphs.push(variant.glyph);
                }
            }
        }
        let rows = glyphs
            .into_iter()
            .map(|glyph| Row {
                key: crate::presentation::text_hash(
                    namespace,
                    &format!("{place}-attached-{:08X}-{glyph:08X}", edit.graph),
                ),
                source: glyph,
                rgb: edit.color,
                theme: None,
                art,
            })
            .collect();
        planned.push(AttachedHud {
            graph: edit.graph,
            rows,
            icon,
        });
    }
    Ok(planned)
}

/// HUD layers travel independently of a donor glyph's colors. A passive node and imported
/// artwork use one layer, while an ability glyph preserves all its native art layers.
fn icon(
    sources: &Sources<'_>,
    stock: &BTreeMap<u32, StockList>,
    icon: Option<&EntryIcon>,
) -> AuthoringResult<(Option<Art>, Option<Icon>)> {
    match icon {
        None => Ok((None, None)),
        Some(EntryIcon::Artwork { artwork }) => Ok((Some(Art::OwnIcon), Some(artwork.clone()))),
        Some(EntryIcon::Ability { subclass, entry }) => {
            let owner = stock
                .get(subclass)
                .ok_or_else(|| invalid("An attached ability's icon donor was not loaded"))?;
            if let Some(glyph) = entry_glyph(sources, owner, *entry)? {
                return Ok((Some(Art::Glyph(glyph)), None));
            }
            let record = read_tag(
                sources.manager,
                owner.node_record(*entry)?,
                "attached ability icon donor",
            )?;
            let container = read_tag(
                sources.manager,
                (sources.icon_container)(native::node_icon(&record)?)?,
                "icon container",
            )?;
            let layer = crate::subclass::hud::primary_layer(sources.manager, &container, false)?;
            Ok((Some(Art::Layer(layer)), None))
        }
    }
}
