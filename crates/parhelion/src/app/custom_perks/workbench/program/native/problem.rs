//! Located authoring choices and warnings over reachable native records.
use super::*;

/// Link offsets stay valid when an editor makes a shared allocation private. Allocation
/// indices and emit/read indices do not, so neither is used as a navigation identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app::custom_perks::workbench) struct Target {
    pub path: Vec<usize>,
    pub row: usize,
    pub offset: Option<usize>,
}

pub(in crate::app::custom_perks::workbench) struct Check {
    pub message: String,
    pub blocking: bool,
    pub target: Target,
}

pub(in crate::app::custom_perks::workbench) fn checks(graph: &Graph) -> Vec<Check> {
    collect(graph).unwrap_or_default()
}

fn collect(graph: &Graph) -> Result<Vec<Check>, String> {
    // Structural failures have their own blocking diagnostics. Validate before walking,
    // then retain paths in this graph instead of reading a reordered emitted graph.
    graph.emit()?;
    let mut pending = vec![(0, Vec::new())];
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    while let Some((index, path)) = pending.pop() {
        if !seen.insert(index) {
            continue;
        }
        let block = &graph.blocks[index];
        if block.class != 0 {
            check_block(graph, block, &path, &mut result)?;
        }
        for (&at, &child) in block.links.iter().rev() {
            let mut next = path.clone();
            next.push(at);
            pending.push((child, next));
        }
    }
    Ok(result)
}

fn check_block(
    graph: &Graph,
    block: &native::Block,
    path: &[usize],
    result: &mut Vec<Check>,
) -> Result<(), String> {
    let stride = schema::record(block.class)?.size;
    let described = fields::describe(block.class)?;
    for row in 0..block.count.unwrap_or(1) {
        if block.class == 0x8080_3E30
            && block
                .links
                .get(&(row * stride + 0x10))
                .is_none_or(|child| graph.blocks[*child].count == Some(0))
        {
            result.push(Check {
                message: "The counter has no contributing conditions. It can still be changed by actions that update the effect's counter.".into(),
                blocking: false,
                target: Target { path: path.to_vec(), row, offset: None },
            });
        }
        for field in &described {
            let Some(bytes) = field.bytes(block, row) else {
                continue;
            };
            let blocking = fields::unset(block.class, field, bytes);
            if !blocking && !fields::empty_selection(block.class, field, bytes) {
                continue;
            }
            let label = plain_field_label(block.class, &field.label);
            let title = node_title(block);
            result.push(Check {
                message: if blocking {
                    format!("Choose the {label} for {title}.")
                } else {
                    format!(
                        "The {label} selection for {title} is empty. It selects no events or slots."
                    )
                },
                blocking,
                target: Target {
                    path: path.to_vec(),
                    row,
                    offset: Some(field.offset),
                },
            });
        }
    }
    Ok(())
}

fn id() -> egui::Id {
    egui::Id::new("native-field-target")
}

#[derive(Clone)]
struct Reveal {
    target: Target,
    shown: bool,
}

pub(in crate::app::custom_perks::workbench) fn reveal(ctx: &egui::Context, target: Target) {
    ctx.data_mut(|data| {
        data.insert_temp(
            id(),
            Reveal {
                target,
                shown: false,
            },
        );
    });
}

pub(in crate::app::custom_perks::workbench) fn finish(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| {
        let state = data.get_temp::<Reveal>(id());
        data.remove::<Reveal>(id());
        state.is_some_and(|state| state.shown)
    })
}

fn target(ui: &egui::Ui) -> Option<Target> {
    ui.data(|data| data.get_temp::<Reveal>(id()))
        .filter(|state| !state.shown)
        .map(|state| state.target)
}

pub(super) fn contains(ui: &egui::Ui, path: &[usize]) -> bool {
    target(ui).is_some_and(|target| target.path.starts_with(path))
}

pub(super) fn contains_row(ui: &egui::Ui, path: &[usize], row: usize, stride: usize) -> bool {
    target(ui).is_some_and(|target| {
        target.path.starts_with(path)
            && if target.path == path {
                target.row == row
            } else {
                target.path[path.len()] / stride == row
            }
    })
}

/// Reveal the entire body in this frame, so a scroll target does not land inside a
/// still-clipped opening animation. The header retains its normal stored open state.
pub(super) fn open_header(ui: &egui::Ui, id: egui::Id, reveal: bool) -> Option<bool> {
    if reveal {
        ui.ctx().animate_bool_with_time(id, true, 0.0);
        Some(true)
    } else {
        None
    }
}

pub(super) fn field_response(
    ui: &egui::Ui,
    path: &[usize],
    row: usize,
    offset: usize,
    response: &egui::Response,
) {
    if let Some(target) = target(ui)
        .filter(|target| target.path == path && target.row == row && target.offset == Some(offset))
    {
        scroll(ui, response, target);
    }
}

pub(super) fn node_response(ui: &egui::Ui, path: &[usize], response: &egui::Response) {
    if let Some(target) = target(ui).filter(|target| target.path == path && target.offset.is_none())
    {
        scroll(ui, response, target);
    }
}

fn scroll(ui: &egui::Ui, response: &egui::Response, target: Target) {
    response.scroll_to_me(Some(egui::Align::Center));
    ui.data_mut(|data| {
        data.insert_temp(
            id(),
            Reveal {
                target,
                shown: true,
            },
        )
    });
}
