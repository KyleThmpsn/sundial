//! One software frame in flight, with stale selection and edit results discarded.
use super::*;

#[derive(Clone, PartialEq)]
pub(super) struct Key {
    pub model: usize,
    pub camera: Camera,
    pub scene: Scene,
    pub size: [usize; 2],
    pub seconds: f32,
    pub style: render::Style,
    pub overrides: Vec<SurfaceOverride>,
}
impl Key {
    fn compatible(&self, other: &Self) -> bool {
        let mut comparison = self.clone();
        comparison.seconds = other.seconds;
        comparison == *other
    }
}

#[derive(Default)]
pub(super) struct Job {
    pending: Option<(Key, mpsc::Receiver<egui::ColorImage>)>,
    rendered: Option<Key>,
}
impl Job {
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        model: Arc<Model>,
        key: Key,
    ) -> Option<(Key, egui::ColorImage)> {
        let mut ready = None;
        if let Some((source, receiver)) = &self.pending {
            match receiver.try_recv() {
                Ok(image) => {
                    if source.compatible(&key) {
                        ready = Some((source.clone(), image));
                        self.rendered = Some(source.clone());
                    }
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.pending.is_none() && self.rendered.as_ref() != Some(&key) {
            let (sender, receiver) = mpsc::channel();
            self.pending = Some((key.clone(), receiver));
            let (repaint, viewport) = (ctx.clone(), ctx.viewport_id());
            std::thread::spawn(move || {
                let image = render::preview_image(
                    &model,
                    key.camera,
                    key.scene,
                    key.size,
                    key.seconds,
                    key.style,
                    &key.overrides,
                );
                let _ = sender.send(image);
                repaint.request_repaint_of(viewport);
            });
        }
        if self.pending.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        ready
    }
}
