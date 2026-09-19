//! The behavior script an effect of kind 48 runs, chosen by name from the ones stock perks ship.
use super::*;
use crate::app::custom_perks::workbench::controls::sized;
use sundial::package_authoring::sandbox_perk::action::native::fields::scripts::{self, Script};

pub(super) const CLASS: u32 = 0x80802D0A;
const PATH: usize = 0x8;
const TAG: usize = 0x10;
/// A script reads by its file name, which is longer than the shared value column holds, so
/// this control is given the extra room rather than eliding most of every name.
const SCRIPT_WIDTH: f32 = 260.0;

pub(super) fn draw(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<(), String> {
    let tag = current_tag(graph, index)?;
    let mut chosen = tag;
    super::super::super::properties::field(
        ui,
        "Game Script",
        "One of the game's own scripts, named by its file. The stock perks that run it are listed beside each one.",
        |ui| {
            let current = scripts::by_tag(tag);
            let title = current.map_or_else(|| format!("Script 0x{tag:08X}"), Script::title);
            // A combo takes the width of its selected text and `width` only sets a floor, so
            // a long script name would run to the edge of the pane. The allocation bounds it
            // and the file path stays on hover.
            let hover = current.map_or_else(
                || title.clone(),
                |script| format!("{title}\n{}", script.path),
            );
            sized(ui, SCRIPT_WIDTH, |ui| {
                egui::ComboBox::from_id_salt("behavior-script")
                    .width(SCRIPT_WIDTH)
                    .truncate()
                    .selected_text(title)
                    .show_ui(ui, |ui| {
                        for script in scripts::SCRIPTS {
                            ui.selectable_value(&mut chosen, script.tag, script.title())
                                .on_hover_text(if script.perks.is_empty() {
                                    format!(
                                        "{}\n{} stock perk nodes run it.",
                                        script.path, script.uses
                                    )
                                } else {
                                    format!(
                                        "{}\n{} stock perk nodes run it: {}.",
                                        script.path, script.uses, script.perks
                                    )
                                });
                        }
                    })
                    .response
                    .on_hover_text(hover);
                pickers::name_combo(ui, "behavior-script", "Game Script");
            });
            Ok::<(), String>(())
        },
    )?;
    if chosen != tag
        && let Some(script) = scripts::by_tag(chosen)
    {
        set(graph, index, script)?;
    }
    Ok(())
}

fn current_tag(graph: &Graph, index: usize) -> Result<u32, String> {
    let bytes = graph.blocks[index]
        .bytes
        .get(TAG..TAG + 4)
        .ok_or("The runtime operation record is truncated.")?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Points the node at a script: its path in the string block the node owns, and its tag.
pub(super) fn set(graph: &mut Graph, index: usize, script: &Script) -> Result<(), String> {
    if graph
        .blocks
        .get(index)
        .is_none_or(|block| block.class != CLASS)
    {
        return Err("The selected action does not run a game script.".into());
    }
    let mut changed = graph.clone();
    // Always allocate the string rather than writing through the one already linked. Two
    // nodes that named the same script share a single path allocation, so writing in place
    // rewrote the other node's path while only this node's tag moved, leaving it running a
    // script its own path no longer names. The allocation this replaces is dropped when the
    // edit is committed.
    changed.create_target(index, PATH, 0, false)?;
    let target = changed.blocks[index].links[&PATH];
    let mut path = script.path.as_bytes().to_vec();
    path.push(0);
    changed.blocks[target].bytes = path;
    changed.blocks[index].bytes[TAG..TAG + 4].copy_from_slice(&script.tag.to_le_bytes());
    changed.validate()?;
    *graph = changed;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_a_script_writes_its_path_and_tag_and_survives_a_reload() {
        let bytes = native::template(false, 48).unwrap();
        let mut graph = Graph::read(&bytes, 0, CLASS).unwrap();
        for script in scripts::SCRIPTS {
            set(&mut graph, 0, script).unwrap();
            let reread = Graph::read(&graph.emit().unwrap(), 0, CLASS).unwrap();
            reread.validate_node(false, 48).unwrap();
            assert_eq!(current_tag(&reread, 0).unwrap(), script.tag);
            let target = reread.blocks[0].links[&PATH];
            let stored = String::from_utf8(reread.blocks[target].bytes.clone()).unwrap();
            assert_eq!(stored.trim_end_matches('\0'), script.path);
        }
        let mut other = Graph::read(&native::template(false, 42).unwrap(), 0, 0x80803E2F).unwrap();
        assert!(set(&mut other, 0, &scripts::SCRIPTS[0]).is_err());
    }
}
