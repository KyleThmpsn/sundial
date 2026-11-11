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
    /// A continuous viewer orbit may display a completed earlier yaw frame.
    pub orbit: bool,
}
impl Key {
    fn compatible(&self, other: &Self) -> bool {
        let mut comparison = self.clone();
        comparison.seconds = other.seconds;
        if self.orbit && other.orbit {
            comparison.camera.yaw = other.camera.yaw;
        }
        comparison == *other
    }
}

#[derive(Default)]
pub(super) struct Job {
    pending: Option<(u64, Key, mpsc::Receiver<egui::ColorImage>)>,
    rendered: Option<Key>,
    generation: u64,
}
impl Job {
    /// Keep the one render in flight until it finishes, but never accept its old result.
    pub fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.rendered = None;
    }

    pub fn update(
        &mut self,
        ctx: &egui::Context,
        model: Arc<Model>,
        key: Key,
    ) -> Option<(Key, egui::ColorImage)> {
        let mut ready = None;
        if let Some((generation, source, receiver)) = &self.pending {
            match receiver.try_recv() {
                Ok(image) => {
                    if *generation == self.generation && source.compatible(&key) {
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
            self.pending = Some((self.generation, key.clone(), receiver));
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
