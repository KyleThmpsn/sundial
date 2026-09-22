//! Kill-filter editing on a single native condition, independent of its source perk.
use super::labels::{Fold, draw_site_group, set_labels, site_caption, unset_operations};
use super::*;
use crate::app::custom_perks::workbench::controls::{COLUMN_WIDTH, cell};
use sundial::package_authoring::sandbox_perk::activation::PerkActivation;

pub(super) const CLASS: u32 = 0x80803DE7;
/// The kill's own labels, such as precision or shotgun.
const LABELS: usize = 0xD0;
/// What was killed, such as a boss, or the standard exclusions that stop a perk counting a
/// player's own grenade or sparrow as a kill.
const VICTIM: usize = 8;
const WEAPON: usize = 0x141;

pub(super) fn draw(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<(), String> {
    // Each filter is its own labelled row. Left in the caller's horizontal flow they ran
    // side by side across the pane, spread by their label columns, and the last one clipped
    // at the edge. A column of its own keeps them stacked and whole at any width.
    ui.vertical(|ui| rows(ui, graph, index)).inner
}

fn rows(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<(), String> {
    let lists = native::labels::source(graph, index, LABELS)?;
    let owning = graph.blocks[index].bytes[WEAPON] != 0;
    let selected = PerkActivation::from_filter(&lists[0], owning);
    let mut choice = selected;
    // The preset leads the filters it writes, in the same label column and at the same
    // width, so the row reads as the summary of the ones below it.
    let hint = "Changes this condition's kill category and weapon requirement. Other conditions and effects keep their own settings.";
    cell(ui, "Kill Trigger", hint, |ui| {
        egui::ComboBox::from_id_salt("kill-trigger")
            .width(COLUMN_WIDTH)
            .truncate()
            .selected_text(choice.map_or("Custom Kill Filter", PerkActivation::label))
            .show_ui(ui, |ui| {
                for option in PerkActivation::ALL {
                    ui.selectable_value(&mut choice, Some(option), option.label());
                }
            })
            .response
            .on_hover_text(hint);
        pickers::name_combo(ui, "kill-trigger", "Kill Trigger");
    });
    if choice != selected
        && let Some(choice) = choice
    {
        set(graph, index, choice.labels(), choice.requires_weapon())?;
        return Ok(());
    }
    // The five presets reach five of the 56 kill filters the stock perks use, and they only
    // touch one of this node's eight label lists. The node filters on the kill's own labels
    // at +0xD0 and on what was killed at +8, each through four set operations, so every list
    // is editable here from the vocabulary the game uses at that exact site.
    // Both sites fold together, so the node offers one way to reach every empty operation
    // rather than one beside each site.
    let fold = Fold::of(ui, index, 0);
    let mut unset = 0;
    for (group, binding) in [LABELS, VICTIM].into_iter().enumerate() {
        // The preset and the two sites are three separate readings. Run together they read
        // as one list of nine near identical rows, so a site stands off from the one before
        // it by more than the gap between its own rows.
        let lists = native::labels::source(graph, index, binding)?;
        unset += unset_operations(&lists);
        if !fold.unfolded && lists.iter().all(Vec::is_empty) {
            continue;
        }
        ui.add_space(if group == 0 { 2.0 } else { 10.0 });
        // Both sites offer the same four set operations, so the operation alone names two
        // rows the same. The site's own word leads, as it does on every other node.
        let caption = site_caption(graph.blocks[index].class, binding);
        if let Some((operation, labels)) = draw_site_group(
            ui,
            graph.blocks[index].class,
            binding,
            &caption,
            &lists,
            fold.unfolded,
        )? {
            set_labels(graph, index, binding, operation, &labels)?;
            return Ok(());
        }
    }
    fold.draw(ui, unset);
    Ok(())
}

fn set(
    graph: &mut Graph,
    index: usize,
    labels: &[u32],
    requires_weapon: bool,
) -> Result<(), String> {
    if graph
        .blocks
        .get(index)
        .is_none_or(|block| block.class != CLASS || block.count.is_some())
    {
        return Err("The selected condition is not a kill filter.".into());
    }
    let mut changed = graph.clone();
    set_labels(&mut changed, index, LABELS, 0, labels)?;
    changed.blocks[index].bytes[WEAPON] = u8::from(requires_weapon);
    changed.validate()?;
    *graph = changed;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::sandbox_perk::activation;

    #[test]
    fn folded_empty_filter_groups_do_not_leave_a_blank_row() {
        let bytes = native::template(true, 2).unwrap();
        let mut graph = Graph::read(&bytes, 0, CLASS).unwrap();
        for binding in [LABELS, VICTIM] {
            for operation in 0..4 {
                set_labels(&mut graph, 0, binding, operation, &[]).unwrap();
            }
        }
        set(
            &mut graph,
            0,
            PerkActivation::PrecisionWeaponKill.labels(),
            true,
        )
        .unwrap();
        let before = graph.clone();
        let ctx = egui::Context::default();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    crate::app::style::perk_workbench_style(ui);
                    draw(ui, &mut graph, 0).unwrap();
                });
            },
        );
        let rect = |name: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == name => {
                        Some(text.galley.rect.translate(text.pos.to_vec2()))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing {name}"))
        };
        let last_filter = rect("Kill matches any");
        let more = rect("7 more filters");
        assert!(
            more.top() - last_filter.bottom() <= 12.0,
            "Hidden filters reserved a blank row"
        );
        assert!(more.top() >= last_filter.bottom());
        assert_eq!(graph, before);
    }

    #[test]
    fn changing_a_kill_filter_preserves_shared_siblings_and_other_requirements() {
        let bytes = native::template(true, 2).unwrap();
        let source = Graph::read(&bytes, 0, CLASS).unwrap();
        let mut graph = source.clone();
        // A second source condition deliberately shares all of the first one's references.
        let sibling = graph.blocks.len();
        graph.blocks.push(graph.blocks[0].clone());
        for choice in PerkActivation::ALL {
            let mut edited = graph.clone();
            set(&mut edited, 0, choice.labels(), choice.requires_weapon()).unwrap();
            assert_eq!(&edited.blocks[1..graph.blocks.len()], &graph.blocks[1..]);
            let mut expected = graph.blocks[0].clone();
            expected.bytes[LABELS..LABELS + 8]
                .copy_from_slice(&(choice.labels().len() as u64).to_le_bytes());
            expected.bytes[WEAPON] = u8::from(choice.requires_weapon());
            if let Some(target) = edited.blocks[0].links.get(&(LABELS + 8)) {
                expected.links.insert(LABELS + 8, *target);
            } else {
                expected.links.remove(&(LABELS + 8));
            }
            assert_eq!(edited.blocks[0], expected);
            assert_eq!(
                native::labels::source(&edited, 0, LABELS).unwrap()[0],
                choice.labels()
            );
            assert_eq!(
                native::labels::source(&edited, sibling, LABELS).unwrap(),
                native::labels::source(&graph, sibling, LABELS).unwrap()
            );
            // Relocation retains the new filter and validates the full node.
            let reread = Graph::read(&edited.emit().unwrap(), 0, CLASS).unwrap();
            reread.validate_node(true, 2).unwrap();
            assert_eq!(
                native::labels::source(&reread, 0, LABELS).unwrap()[0],
                choice.labels()
            );
        }
    }

    #[test]
    fn a_composed_kill_filter_the_presets_cannot_reach_round_trips() {
        // The stock perks use 56 distinct kill filters and the five presets reach five of
        // them. The rest are ordinary label sets, so composing one has to survive a write
        // and a reload the same way a preset does.
        let vocabulary = activation::site_labels(CLASS, LABELS);
        assert!(
            vocabulary.len() >= 40,
            "the kill vocabulary should cover what the stock perks filter on, found {}",
            vocabulary.len()
        );
        let find = |wanted: &str| {
            vocabulary
                .iter()
                .find(|(_, name, _)| *name == wanted)
                .unwrap_or_else(|| panic!("the stock perks filter kills on {wanted}"))
                .0
        };
        let mut labels = vec![find("shotgun"), find("sniper rifle")];
        labels.sort_unstable();
        assert!(
            PerkActivation::from_filter(&labels, false).is_none(),
            "this set is deliberately one no preset reaches"
        );
        let bytes = native::template(true, 2).unwrap();
        let mut graph = Graph::read(&bytes, 0, CLASS).unwrap();
        set_labels(&mut graph, 0, LABELS, 0, &labels).unwrap();
        let reread = Graph::read(&graph.emit().unwrap(), 0, CLASS).unwrap();
        reread.validate_node(true, 2).unwrap();
        let mut stored = native::labels::source(&reread, 0, LABELS).unwrap()[0].clone();
        stored.sort_unstable();
        assert_eq!(stored, labels);
        // Clearing every label is the any-kill case rather than an error.
        set_labels(&mut graph, 0, LABELS, 0, &[]).unwrap();
        assert!(native::labels::source(&graph, 0, LABELS).unwrap()[0].is_empty());
    }

    #[test]
    fn editing_one_label_list_leaves_the_other_seven_alone() {
        // The kill node carries two binding sites with four set operations each. Writing one
        // list must not disturb the rest, or changing an exclusion would silently drop the
        // category the perk fires on.
        let bytes = native::template(true, 2).unwrap();
        let mut graph = Graph::read(&bytes, 0, CLASS).unwrap();
        let victim = activation::site_labels(CLASS, VICTIM);
        assert!(
            !victim.is_empty(),
            "the stock perks filter on what was killed, so that site has a vocabulary"
        );
        let excluded = vec![victim[0].0];
        set_labels(&mut graph, 0, LABELS, 0, &[0x962E_A19B]).unwrap();
        set_labels(&mut graph, 0, VICTIM, 2, &excluded).unwrap();
        let reread = Graph::read(&graph.emit().unwrap(), 0, CLASS).unwrap();
        reread.validate_node(true, 2).unwrap();
        assert_eq!(
            native::labels::source(&reread, 0, LABELS).unwrap()[0],
            vec![0x962E_A19B]
        );
        assert_eq!(
            native::labels::source(&reread, 0, VICTIM).unwrap()[2],
            excluded
        );
        // The lists neither edit touched stay empty rather than inheriting either write.
        assert!(native::labels::source(&reread, 0, LABELS).unwrap()[2].is_empty());
        assert!(native::labels::source(&reread, 0, VICTIM).unwrap()[0].is_empty());
    }

    #[test]
    fn every_site_label_is_named_and_counted() {
        for (class, offset, labels) in activation::LABEL_SITES {
            assert!(!labels.is_empty(), "{class:08X}+{offset:X} has no labels");
            for (hash, name, uses) in *labels {
                assert!(!name.trim().is_empty(), "label 0x{hash:08X} has no name");
                assert!(*uses > 0, "{name} is offered with no stock use");
                assert_eq!(activation::site_label_name(*hash), Some(*name));
            }
        }
        // A label no stock perk uses at any site is not offered.
        assert_eq!(activation::site_label_name(0x1234_5678), None);
        assert!(activation::site_labels(0x1234_5678, 0).is_empty());
    }
}
