//! The source ammunition HUD silhouette as the recipe's HUD artwork.
use super::*;

/// The keys that can name the source silhouette, in order: the content row's explicit HUD key at
/// `+88` (`811C9DC5` when it has none), the row's type name at `+20`, the pattern's content key,
/// then the weapon type's own name. Glyph keys are FNV-1 type names such as `fusion_rifle` and
/// `machinegun`, so the type is tried with its words joined by an underscore and by nothing.
fn candidates(content: &Content, kind: &str) -> Result<Vec<(&'static str, u32)>> {
    let mut keys = vec![
        ("explicit", content.owner.u32(content.row + 0x88)?),
        ("row_type", content.owner.u32(content.row + 0x20)?),
        ("content_key", content.key),
    ];
    if let Some(kind) = crate::weapon_type::from_text(kind) {
        let words = kind.to_lowercase();
        for name in [words.replace(' ', "_"), words.replace(' ', "")] {
            keys.push(("weapon_type", key(&name)));
        }
    }
    Ok(keys)
}

/// The source weapon's silhouette, fitted to the native ammunition HUD artwork, and its report.
/// Glyph tables draw the weapon silhouettes at several sizes. The one drawn at the native
/// ammunition HUD width is preferred, and a key drawn at one size only is fitted from it.
pub(super) fn prepare(
    source: &Source,
    export: &Value,
    kind: &str,
    folder: &Path,
) -> Result<(Value, Option<Vec<u8>>)> {
    let content = content_row(source, export)?;
    for (field, key) in candidates(&content, kind)? {
        if matches!(key, 0 | u32::MAX | 0x811C9DC5) {
            continue;
        }
        let glyphs = source.hud_glyphs(key)?;
        let sized = glyphs
            .iter()
            .filter(|(_, image)| image.width() == crate::hud_icon::SIZE[0])
            .collect::<Vec<_>>();
        let candidates = if sized.is_empty() {
            glyphs.iter().collect()
        } else {
            sized
        };
        let (table, glyph) = match candidates.as_slice() {
            [] => continue,
            [glyph] => glyph,
            _ => {
                return Ok((
                    json!({"status":"source_only","field":field,"key":key,
                        "reason":"Several source HUD glyphs of the same size name this key"}),
                    None,
                ));
            }
        };
        let png = crate::hud_icon::fit(glyph)?;
        fs::create_dir_all(folder)?;
        fs::write(folder.join("hud.png"), &png)?;
        glyph.save(folder.join("hud-source.png"))?;
        return Ok((
            json!({"status":"translated","field":field,"key":key,"table":table,
                "content_keyed":content.keyed,"source_dimensions":[glyph.width(),glyph.height()],
                "dimensions":crate::hud_icon::SIZE,
                "method":"Source glyph uniformly fitted and centered on a transparent canvas"}),
            Some(png),
        ));
    }
    Ok((
        json!({"status":"source_only","reason":"No source HUD glyph names this weapon's content"}),
        None,
    ))
}
