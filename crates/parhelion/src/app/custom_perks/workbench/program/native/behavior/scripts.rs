//! Installed object-behavior scripts for effect kind 48.
use super::*;
use crate::app::custom_perks::workbench::controls::sized;
use sundial::package_authoring::sandbox_perk::action::native::fields::scripts::{self, Script};

pub(super) const CLASS: u32 = 0x80802D0A;
const PATH: usize = 0x8;
const TAG: usize = 0x10;

pub(super) fn draw(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<(), String> {
    use sundial::investment::discovery::scripts::Choice;
    let installed = ui
        .ctx()
        .data(|data| {
            data.get_temp::<Option<Arc<Vec<Choice>>>>(egui::Id::new("installed-behavior-scripts"))
        })
        .flatten();
    let tag = current_tag(graph, index)?;
    let mut chosen = tag;
    // A script neither list names is typed under the list as its path and tag, once Other
    // Value… is chosen or while the node already runs one.
    let listed = installed
        .as_ref()
        .is_some_and(|choices| choices.iter().any(|choice| choice.tag == tag))
        || scripts::by_tag(tag).is_some();
    let typed = super::super::Typed::new(ui, "behavior-script", listed);
    let stored_path = current_path(graph, index);
    let mut typed_path = stored_path.clone();
    let mut typed_tag = tag;
    let mut chose_listed = false;
    crate::app::style::tiles(ui, |ui, width| {
        crate::app::style::tile(
            ui,
            width,
            "game-script",
            "Game Script",
            "Scripts in the installed packages. Some need specific game state.",
            false,
            |ui| {
                let current = scripts::by_tag(tag);
                let resolved = installed
                    .as_ref()
                    .and_then(|choices| choices.iter().find(|choice| choice.tag == tag));
                let title = resolved
                    .map(|choice| choice.title.clone())
                    .or_else(|| current.map(Script::title))
                    .unwrap_or_else(|| format!("Script 0x{tag:08X}"));
                // A combo takes the width of its selected text and `width` only sets a floor, so
                // a long script name would run to the edge of the pane. The allocation bounds it
                // and the file path stays on hover.
                let hover = resolved
                    .map(|script| format!("{title}\n{}", script.path))
                    .unwrap_or_else(|| {
                        current.map_or_else(
                            || title.clone(),
                            |script| format!("{title}\n{}", script.path),
                        )
                    });
                sized(ui, ui.available_width(), |ui| {
                    // The list holds a search box, so only a choice or a click outside closes it.
                    egui::ComboBox::from_id_salt("behavior-script")
                        .width(ui.available_width())
                        .truncate()
                        .selected_text(title)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show_ui(ui, |ui| {
                            let query_id = ui.make_persistent_id("script-query");
                            let mut query = ui
                                .data(|data| data.get_temp::<String>(query_id).unwrap_or_default());
                            let search = ui.add(
                                egui::TextEdit::singleline(&mut query)
                                    .hint_text("Search Game Scripts"),
                            );
                            pickers::name_response(ui, &search, "Search Game Scripts");
                            ui.data_mut(|data| data.insert_temp(query_id, query.clone()));
                            if let Some(choices) = &installed {
                                let matches = choices
                                    .iter()
                                    .filter(|script| {
                                        pickers::matches(
                                            &query.to_lowercase(),
                                            &format!(
                                                "{} {} {} {:08X}",
                                                script.title,
                                                script.path,
                                                script.stock_perks,
                                                script.tag
                                            ),
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                ui.weak(if matches.len() == 1 {
                                    "1 Script".to_owned()
                                } else {
                                    format!("{} Scripts", matches.len())
                                });
                                for script in matches {
                                    let detail = if script.stock_perks.is_empty() {
                                        "No stock perk uses it.".to_owned()
                                    } else {
                                        format!("Stock perks: {}", script.stock_perks)
                                    };
                                    chose_listed |= ui
                                        .selectable_value(&mut chosen, script.tag, &script.title)
                                        .on_hover_text(format!(
                                            "{}\n{detail}\n0x{:08X}",
                                            script.path, script.tag
                                        ))
                                        .clicked();
                                }
                            } else {
                                ui.weak("Reading installed scripts…");
                                for script in scripts::SCRIPTS {
                                    if pickers::matches(
                                        &query.to_lowercase(),
                                        &format!("{} {}", script.path, script.perks),
                                    ) {
                                        chose_listed |= ui
                                            .selectable_value(
                                                &mut chosen,
                                                script.tag,
                                                script.title(),
                                            )
                                            .on_hover_text(format!(
                                                "{}\nStock perks: {}",
                                                script.path, script.perks
                                            ))
                                            .clicked();
                                    }
                                }
                            }
                            chose_listed |= typed.row(ui);
                        })
                        .response
                        .on_hover_text(hover);
                    if chose_listed {
                        egui::Popup::close_all(ui);
                    }
                    pickers::name_combo(ui, "behavior-script", "Game Script");
                });
            },
        );
        if typed.shown {
            crate::app::style::tile(ui, width, "script-path", "Script Path", "", false, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut typed_path)
                        .desired_width(ui.available_width())
                        .hint_text("content\\…\\name.object_behaviors.tft"),
                );
                pickers::name_response(ui, &response, "Script Path");
            });
            crate::app::style::tile(ui, width, "script-tag", "Script Tag", "", false, |ui| {
                let response = crate::app::custom_perks::workbench::controls::hex_key(
                    ui,
                    "script-tag",
                    &mut typed_tag,
                );
                pickers::name_response(ui, &response, "Script Tag");
            });
        }
    });
    if chose_listed && chosen != tag {
        typed.listed(ui);
    }
    // A typed script is written once both its path and tag are there.
    if chosen == tag
        && typed.shown
        && typed_tag != 0
        && !typed_path.trim().is_empty()
        && (typed_tag != tag || typed_path.trim() != stored_path)
    {
        set_reference(graph, index, typed_path.trim(), typed_tag)?;
        return Ok(());
    }
    if chosen != tag {
        if let Some(script) = installed
            .as_ref()
            .and_then(|choices| choices.iter().find(|script| script.tag == chosen))
        {
            set_reference(graph, index, &script.path, script.tag)?;
        } else if let Some(script) = scripts::by_tag(chosen) {
            set(graph, index, script)?;
        }
    }
    Ok(())
}

/// The path the node stores beside its tag, or nothing when it has none.
fn current_path(graph: &Graph, index: usize) -> String {
    graph.blocks[index]
        .links
        .get(&PATH)
        .and_then(|target| graph.blocks.get(*target))
        .map(|block| {
            String::from_utf8_lossy(&block.bytes)
                .trim_end_matches('\0')
                .to_owned()
        })
        .unwrap_or_default()
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
    set_reference(graph, index, script.path, script.tag)
}

fn set_reference(graph: &mut Graph, index: usize, path: &str, tag: u32) -> Result<(), String> {
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
    let mut path = path.as_bytes().to_vec();
    path.push(0);
    changed.blocks[target].bytes = path;
    changed.blocks[index].bytes[TAG..TAG + 4].copy_from_slice(&tag.to_le_bytes());
    changed.validate()?;
    *graph = changed;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::sandbox_perk::action::native::NodeKind as NativeNodeKind;

    #[test]
    fn installed_script_outside_stock_examples_roundtrips_without_changing_other_fields() {
        let mut graph = Graph::read(
            &native::template(NativeNodeKind::Effect(48)).unwrap(),
            0,
            CLASS,
        )
        .unwrap();
        let original = graph.blocks[0].bytes.clone();
        let path = r"content\new\another_script.object_behaviors.tft";
        let tag = 0x8123_4567;
        set_reference(&mut graph, 0, path, tag).unwrap();
        let reread = Graph::read(&graph.emit().unwrap(), 0, CLASS).unwrap();
        assert_eq!(current_tag(&reread, 0).unwrap(), tag);
        let target = reread.blocks[0].links[&PATH];
        assert_eq!(&reread.blocks[target].bytes[..path.len()], path.as_bytes());
        assert_eq!(&reread.blocks[0].bytes[..PATH], &original[..PATH]);
        assert_eq!(&reread.blocks[0].bytes[TAG + 4..], &original[TAG + 4..]);
    }

    #[test]
    fn choosing_a_script_writes_its_path_and_tag_and_survives_a_reload() {
        let bytes = native::template(NativeNodeKind::Effect(48)).unwrap();
        let mut graph = Graph::read(&bytes, 0, CLASS).unwrap();
        for script in scripts::SCRIPTS {
            set(&mut graph, 0, script).unwrap();
            let reread = Graph::read(&graph.emit().unwrap(), 0, CLASS).unwrap();
            reread.validate_node(NativeNodeKind::Effect(48)).unwrap();
            assert_eq!(current_tag(&reread, 0).unwrap(), script.tag);
            let target = reread.blocks[0].links[&PATH];
            let stored = String::from_utf8(reread.blocks[target].bytes.clone()).unwrap();
            assert_eq!(stored.trim_end_matches('\0'), script.path);
        }
        let mut other = Graph::read(
            &native::template(NativeNodeKind::Effect(42)).unwrap(),
            0,
            0x80803E2F,
        )
        .unwrap();
        assert!(set(&mut other, 0, &scripts::SCRIPTS[0]).is_err());
    }
}
