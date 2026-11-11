//! Keep the matching portrait or body when switching views. Only the active view owns a GPU upload.
use super::*;

pub(super) struct View {
    source: Wanted,
    model: Arc<Model>,
    bounds: Option<([f32; 3], [f32; 3])>,
}

fn counterparts(a: &Wanted, b: &Wanted) -> bool {
    a.0 == b.0
        && a.3 == b.3
        && match (&a.1, &b.1) {
            (Subject::Character(a), Subject::Character(b)) => {
                a.portrait != b.portrait && a.appearance == b.appearance && a.armor == b.armor
            }
            _ => false,
        }
}

impl Still {
    pub(super) fn change_source(&mut self, next: &Wanted) {
        let ready = self.cached.take().filter(|view| view.source == *next);
        let previous = self
            .shown
            .as_ref()
            .zip(self.model.as_ref())
            .filter(|(source, _)| {
                self.wanted.as_ref() == Some(source) && counterparts(source, next)
            })
            .map(|(source, model)| View {
                source: source.clone(),
                model: Arc::clone(model),
                bounds: self.bounds,
            });
        self.clear_model();
        self.cached = previous;
        if let Some(view) = ready {
            self.camera = next.1.camera();
            self.model = Some(view.model);
            self.bounds = view.bounds;
            self.shown = Some(view.source);
            model_preview::timing::mark(|| format!("cached {}", next.1.label()), "ui");
        }
    }
}
