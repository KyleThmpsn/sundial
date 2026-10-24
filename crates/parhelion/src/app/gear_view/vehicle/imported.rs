//! Cached editor diagnostics for content-pinned vehicle graphs. Build validation remains separate.
use parhelion_import::GraphReference;
use std::sync::{Arc, Mutex, mpsc};

type Key = (GraphReference, u32);
type Result = std::result::Result<(), String>;

#[derive(Default)]
struct Validation {
    pending: Option<(Key, mpsc::Receiver<Result>)>,
    completed: Option<(Key, Result)>,
}

pub(super) fn show(ui: &mut egui::Ui, source: Option<(&GraphReference, u32)>) {
    let state = ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<Validation>>>(egui::Id::new(
            "imported-vehicle-validation",
        ))
        .clone()
    });
    let Ok(mut state) = state.lock() else {
        return;
    };
    let wanted = source.map(|(graph, entity)| (graph.clone(), entity));
    if state
        .completed
        .as_ref()
        .is_some_and(|(key, _)| Some(key) != wanted.as_ref())
    {
        state.completed = None;
    }
    if let Some((key, receiver)) = &state.pending {
        let result = match receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "Vehicle compatibility check stopped before returning a result.".into(),
            )),
        };
        if let Some(result) = result {
            if Some(key) == wanted.as_ref() {
                state.completed = Some((key.clone(), result));
            }
            state.pending = None;
        }
    }
    let Some(wanted) = wanted else {
        return;
    };
    if let Some((_, result)) = &state.completed {
        if let Err(error) = result {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        return;
    }
    // Keep one reader in flight. A newer selection waits for it, discards its stale result,
    // then starts only the latest request. No payload reads or worker joins run on the UI thread.
    if state.pending.is_none() {
        let (sender, receiver) = mpsc::channel();
        let (graph, entity) = wanted.clone();
        let ctx = ui.ctx().clone();
        state.pending = Some((wanted, receiver));
        std::thread::spawn(move || {
            let result = graph
                .validate_vehicle(entity)
                .map_err(|error| format!("{error:#}"));
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }
    ui.weak("Checking Vehicle Compatibility…");
}
