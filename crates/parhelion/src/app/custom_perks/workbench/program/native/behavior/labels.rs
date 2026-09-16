//! Label list editing at every binding site the stock perks author, not just the kill node.
//!
//! A node can carry several label binding sites, each with four set operations. The kill
//! filter was one such site. The stock perks author 52 of them across 16 node classes, and
//! every one already has a named vocabulary in `activation::LABEL_SITES`, so each list is
//! editable here from the words the game uses at that exact site.
use super::*;
use sundial::package_authoring::sandbox_perk::activation;

/// Every label binding site on this node that has a stock vocabulary, each as its four set
/// operations. The kill node draws its own sites next to its presets and is skipped here.
pub(super) fn draw_sites(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<(), String> {
    if graph.blocks[index].class == trigger::CLASS {
        return Ok(());
    }
    draw_row_sites(ui, graph, index, 0)
}

/// The binding sites of one record row. Nested records can be arrays, and a site's offset
/// is relative to its row.
pub(super) fn draw_row_sites(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    row: usize,
) -> Result<(), String> {
    let class = graph.blocks[index].class;
    let stride = schema::record(class)?.size;
    for (binding, _) in native::labels::bindings(class)? {
        if activation::site_labels(class, binding).is_empty() {
            continue;
        }
        let at = row * stride + binding;
        let lists = native::labels::source(graph, index, at)?;
        let caption = site_caption(class, binding);
        ui.push_id(("label-site", row, binding), |ui| {
            for (operation, name, hint) in activation::LABEL_OPERATIONS {
                if let Some(labels) = draw_labels(
                    ui,
                    class,
                    binding,
                    operation,
                    &format!("{caption} {}", name.to_lowercase()),
                    hint,
                    &lists[operation],
                )? {
                    set_labels(graph, index, at, operation, &labels)?;
                    return Ok(());
                }
            }
            Ok::<_, String>(())
        })
        .inner?;
    }
    for &source in added_label_sites(class) {
        if activation::site_labels(class, source).is_empty() {
            continue;
        }
        let at = row * stride + source;
        let current = read_list(graph, index, at)?;
        ui.push_id(("added-labels", row, source), |ui| {
            if let Some(labels) = draw_labels(
                ui,
                class,
                source,
                0,
                "Adds labels",
                "Labels added to the event when this action's filter passes. Other perks can read them.",
                &current,
            )? {
                set_labels(graph, index, at, 0, &labels)?;
            }
            Ok::<_, String>(())
        })
        .inner?;
    }
    Ok(())
}

/// The label arrays kinds 37 and 54 add to an event. One list each, laid out like the first
/// operation of a binding site, with the compiled set beside it that the compiler rebuilds.
fn added_label_sites(class: u32) -> &'static [usize] {
    match class {
        0x80803E1A => &[0x98],
        0x8080281C => &[0x68],
        _ => &[],
    }
}

/// What a binding site filters on, in the reader's words. Each caption is read off the
/// vocabulary the stock perks use at that site: weapon types where every label is a weapon
/// family, damage sources where the labels are precision, grenade and sword, targets where
/// they are player, boss and combatant, and abilities where they are super and melee.
fn site_caption(class: u32, binding: usize) -> String {
    let known = match (class, binding) {
        // The kind 37 site mixes weapon families (pulse rifle, energy weapon, heavy weapon)
        // with abilities (super, grenade, melee), which together are what dealt the damage.
        (0x80802F5F, 0x28) | (0x80803DDC, 0x28) | (0x80803E1A, 0x28) => Some("Damage source"),
        (0x808029DB, 0x8)
        | (0x808029EC, 0x8)
        | (0x80803DDD, 0x10)
        | (0x80803DF5, 0x10)
        | (0x80803E3E, 0x8)
        | (0x80803E3F, 0x8) => Some("Weapon type"),
        (0x80802F16, 0x48) => Some("Ability"),
        (0x80802F16, 0xC0) | (0x80804D73, 0x0) | (0x80803DE5, 0x8) => Some("Target"),
        (0x80803E3C, 0x30) => Some("Target rank"),
        _ => None,
    };
    if let Some(caption) = known {
        return caption.to_owned();
    }
    fields::describe(class)
        .ok()
        .and_then(|fields| {
            fields
                .into_iter()
                .find(|field| field.offset == binding)
                .map(|field| field.label)
        })
        .filter(|label| label != "Label Filter")
        .unwrap_or_else(|| "Labels".to_owned())
}

/// One label list stored as a count at `at` and a row array linked at `at + 8`.
fn read_list(graph: &Graph, index: usize, at: usize) -> Result<Vec<u32>, String> {
    let owner = graph.blocks.get(index).ok_or("Missing label owner.")?;
    let Some(target) = owner.links.get(&(at + 8)) else {
        return Ok(Vec::new());
    };
    let rows = graph.blocks.get(*target).ok_or("Missing label rows.")?;
    if rows.class != 0x808094B3 {
        return Err("Invalid label array.".into());
    }
    Ok(rows
        .bytes
        .chunks_exact(24)
        .map(|row| u32::from_le_bytes([row[0], row[1], row[2], row[3]]))
        .collect())
}

/// One label list at one binding site. Returns the new set only when it changed, so the
/// caller writes at most one list per frame and the others keep their stored contents.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_labels(
    ui: &mut egui::Ui,
    class: u32,
    binding: usize,
    operation: usize,
    name: &str,
    hint: &str,
    current: &[u32],
) -> Result<Option<Vec<u32>>, String> {
    let vocabulary = activation::site_labels(class, binding);
    if vocabulary.is_empty() {
        return Ok(None);
    }
    let mut chosen = current.to_vec();
    let summary = if chosen.is_empty() {
        "none".to_owned()
    } else {
        chosen
            .iter()
            .map(|hash| {
                activation::site_label_name(*hash)
                    .map_or_else(|| format!("0x{hash:08X}"), plain_label)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut changed = false;
    let salt = ("labels", class, binding, operation);
    // A site can carry nineteen labels. The default popup shows nine and hides the rest
    // behind a scroll with no visible bar, so the popup is tall enough for every stock list.
    egui::ComboBox::from_id_salt(salt)
        .width(240.0)
        .height(480.0)
        .selected_text(format!("{name}: {summary}"))
        .show_ui(ui, |ui| {
            ui.small(hint);
            for (hash, label, uses) in vocabulary {
                let mut on = chosen.contains(hash);
                if ui
                    .checkbox(&mut on, plain_label(label))
                    .on_hover_text(format!(
                        "{uses} stock perks use this label here. The engine's token is \"{label}\"."
                    ))
                    .changed()
                {
                    changed = true;
                    if on {
                        chosen.push(*hash);
                    } else {
                        chosen.retain(|candidate| candidate != hash);
                    }
                }
            }
        })
        .response
        .on_hover_text(hint);
    pickers::name_combo(ui, salt, name);
    if changed {
        chosen.sort_unstable();
        chosen.dedup();
        return Ok(Some(chosen));
    }
    Ok(None)
}

/// A label as a reader sees it: the engine's token with its underscores opened and each
/// word capitalized, so "projectile_melee" reads "Projectile Melee". The token itself stays
/// in the hover text and in the vocabulary, since it is what the game matches on.
pub(super) fn plain_label(token: &str) -> String {
    token
        .split(['_', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Writes one of the four label lists at one binding site, leaving the other three alone.
/// With operation 0 it also writes a single added-label array laid out the same way.
pub(super) fn set_labels(
    graph: &mut Graph,
    index: usize,
    binding: usize,
    operation: usize,
    labels: &[u32],
) -> Result<(), String> {
    let at = binding + operation * 16;
    let mut changed = graph.clone();
    changed.create_target(index, at + 8, 0x808094B3, true)?;
    let rows = changed.blocks[index].links[&(at + 8)];
    changed.resize_array(rows, labels.len())?;
    for (row, label) in labels.iter().enumerate() {
        let offset = row * 24;
        changed.blocks[rows].bytes[offset..offset + 4].copy_from_slice(&label.to_le_bytes());
        changed.blocks[rows].bytes[offset + 16..offset + 24]
            .copy_from_slice(&u64::from(activation::LABEL_GLOBALS).to_le_bytes());
    }
    changed.validate()?;
    *graph = changed;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every stock binding site with a vocabulary is reachable from its node's own editor,
    /// not only the kill node's, and a write there survives a reload.
    #[test]
    fn every_stock_binding_site_is_editable_from_its_node() {
        let mut sites = 0;
        let mut nested = 0;
        for (class, offset, vocabulary) in activation::LABEL_SITES {
            let kind = nodes::CONDITIONS
                .iter()
                .map(|node| (true, node))
                .chain(nodes::EFFECTS.iter().map(|node| (false, node)))
                .find(|(_, node)| node.class == *class);
            let bindings = native::labels::bindings(*class).unwrap();
            let added = added_label_sites(*class).contains(offset);
            assert!(
                bindings.iter().any(|(source, _)| source == offset) || added,
                "{class:08X}+{offset:X} is a vocabulary site with no binding"
            );
            // A site on a node starts from that node's template. A site on a nested record,
            // which the workbench reaches through the node that owns it, starts from an
            // empty record of that class.
            let mut graph = match kind {
                Some((condition, node)) => {
                    Graph::read(&native::template(condition, node.kind).unwrap(), 0, *class)
                        .unwrap()
                }
                None => {
                    nested += 1;
                    Graph {
                        blocks: vec![native::Block {
                            class: *class,
                            count: None,
                            bytes: vec![0; schema::record(*class).unwrap().size],
                            links: BTreeMap::new(),
                        }],
                    }
                }
            };
            let labels = vec![vocabulary[0].0];
            set_labels(&mut graph, 0, *offset, 0, &labels).unwrap();
            let reread = Graph::read(&graph.emit().unwrap(), 0, *class).unwrap();
            if let Some((condition, node)) = kind {
                reread.validate_node(condition, node.kind).unwrap();
            } else {
                reread.validate().unwrap();
            }
            let stored = if added {
                read_list(&reread, 0, *offset).unwrap()
            } else {
                native::labels::source(&reread, 0, *offset).unwrap()[0].clone()
            };
            assert_eq!(stored, labels, "{class:08X}+{offset:X}");
            // A site reads by what it filters on, never by its offset.
            let caption = site_caption(*class, *offset);
            assert!(
                !caption.is_empty() && !caption.contains("0x") && !caption.contains('+'),
                "{class:08X}+{offset:X} reads as {caption:?}"
            );
            sites += 1;
        }
        assert!(sites >= 20, "found only {sites} editable sites");
        assert!(
            nested >= 1,
            "the nested weapon-label sites should be covered"
        );
    }
}
