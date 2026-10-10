//! A paint-thread service with cancellable worker receipts. No GL calls cross threads.
use super::*;
use crate::model_preview::export::bake::{Baker, Painted, Request as BakeRequest};
use std::{
    collections::VecDeque,
    sync::{Weak, atomic::AtomicU64, mpsc},
    time::Duration,
};

#[derive(Default)]
pub(super) struct Queue {
    pending: Mutex<VecDeque<Job>>,
    epoch: AtomicU64,
}

struct Ticket {
    queue: Weak<Queue>,
    epoch: u64,
    context: egui::Context,
    viewport: egui::ViewportId,
}

impl Ticket {
    fn queue(&self) -> Result<Arc<Queue>, String> {
        self.queue
            .upgrade()
            .filter(|queue| queue.epoch.load(Ordering::Acquire) == self.epoch)
            .ok_or_else(|| "Export cancelled because the preview closed.".into())
    }
    fn enqueue(&self, job: Job) -> Result<(), String> {
        let queue = self.queue()?;
        let mut pending = queue
            .pending
            .lock()
            .map_err(|_| "The export queue is unavailable.")?;
        if queue.epoch.load(Ordering::Acquire) != self.epoch {
            return Err("Export cancelled because the preview closed.".into());
        }
        pending.push_back(job);
        self.context.request_repaint_of(self.viewport);
        Ok(())
    }
    fn wait<T>(&self, receiver: mpsc::Receiver<Result<T, String>>) -> Result<T, String> {
        loop {
            self.queue()?;
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("The preview export service stopped.".into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.context.request_repaint_of(self.viewport)
                }
            }
        }
    }
}

pub(crate) struct Receipt<T> {
    ticket: Ticket,
    receiver: mpsc::Receiver<Result<T, String>>,
}
impl<T> Receipt<T> {
    pub fn wait(self) -> Result<T, String> {
        self.ticket.wait(self.receiver)
    }
}

pub(crate) struct Client {
    ticket: Ticket,
    model: Arc<Model>,
}
impl Baker for Client {
    fn paint(&mut self, request: BakeRequest) -> Result<Painted, String> {
        let (sender, receiver) = mpsc::channel();
        self.ticket.enqueue(Job {
            epoch: self.ticket.epoch,
            kind: Kind::Bake {
                model: self.model.clone(),
                request,
                sender,
            },
        })?;
        self.ticket.wait(receiver)
    }
}

impl Shared {
    fn ticket(&self, context: egui::Context) -> Ticket {
        Ticket {
            queue: Arc::downgrade(&self.1),
            epoch: self.1.epoch.load(Ordering::Acquire),
            viewport: context.viewport_id(),
            context,
        }
    }
    pub fn baker(&self, context: egui::Context, model: Arc<Model>) -> Client {
        Client {
            ticket: self.ticket(context),
            model,
        }
    }
    pub fn image(
        &self,
        context: egui::Context,
        mut frame: Frame,
        size: [usize; 2],
    ) -> Receipt<egui::ColorImage> {
        frame.dyes = Some(
            frame
                .dyes
                .unwrap_or_else(|| shader::dyes(&frame.model, frame.seconds)),
        );
        let ticket = self.ticket(context);
        let (sender, receiver) = mpsc::channel();
        let failed = sender.clone();
        if let Err(error) = ticket.enqueue(Job {
            epoch: ticket.epoch,
            kind: Kind::Image {
                frame,
                size,
                sender,
            },
        }) {
            let _ = failed.send(Err(error));
        }
        Receipt { ticket, receiver }
    }
    pub fn cancel_exports(&self) {
        self.1.epoch.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut pending) = self.1.pending.lock() {
            pending.clear();
        }
    }
    /// Schedule even while the current selection is loading or has no renderable mesh.
    pub fn service(&self, ui: &egui::Ui) {
        let (state, queue) = (self.0.clone(), self.1.clone());
        let (context, viewport) = (ui.ctx().clone(), ui.ctx().viewport_id());
        let callback = eframe::egui_glow::CallbackFn::new(move |_, painter| {
            if let Ok(mut state) = state.lock() {
                // SAFETY: only the painter supplies a current GL context to this service.
                if unsafe { state.exports.step(painter.gl(), &queue) } {
                    context.request_repaint_after_for(Duration::from_millis(1), viewport);
                }
            }
        });
        let rect = egui::Rect::from_min_size(ui.clip_rect().min, egui::vec2(1.0, 1.0));
        ui.painter().add(egui::Shape::Callback(egui::PaintCallback {
            rect,
            callback: Arc::new(callback),
        }));
    }
}

struct Job {
    epoch: u64,
    kind: Kind,
}
enum Kind {
    Image {
        frame: Frame,
        size: [usize; 2],
        sender: mpsc::Sender<Result<egui::ColorImage, String>>,
    },
    Bake {
        model: Arc<Model>,
        request: BakeRequest,
        sender: mpsc::Sender<Result<Painted, String>>,
    },
}
enum Active {
    Image {
        epoch: u64,
        capture: Box<Capture>,
        sender: mpsc::Sender<Result<egui::ColorImage, String>>,
    },
    Bake {
        epoch: u64,
        renderer: Box<bake::Renderer>,
        sender: mpsc::Sender<Result<Painted, String>>,
    },
}
impl Active {
    fn epoch(&self) -> u64 {
        match self {
            Self::Image { epoch, .. } | Self::Bake { epoch, .. } => *epoch,
        }
    }
    unsafe fn cancel(self, gl: &glow::Context) {
        // SAFETY: active jobs only create resources on the service's current context.
        unsafe {
            match self {
                Self::Image { capture, .. } => capture.delete(gl),
                Self::Bake { renderer, .. } => {
                    renderer.finish(gl);
                }
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Worker {
    active: Option<Active>,
}
impl Worker {
    pub unsafe fn step(&mut self, gl: &glow::Context, queue: &Queue) -> bool {
        // SAFETY: restore both framebuffer targets after all successful and failed passes.
        unsafe {
            let draw = NonZeroU32::new(gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING) as u32)
                .map(glow::NativeFramebuffer);
            let read = NonZeroU32::new(gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING) as u32)
                .map(glow::NativeFramebuffer);
            self.advance(gl, queue);
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, draw);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, read);
            self.active.is_some() || queue.pending.lock().is_ok_and(|jobs| !jobs.is_empty())
        }
    }

    unsafe fn advance(&mut self, gl: &glow::Context, queue: &Queue) {
        // SAFETY: only Worker::step calls this on its current paint context.
        unsafe {
            if self
                .active
                .as_ref()
                .is_some_and(|job| job.epoch() != queue.epoch.load(Ordering::Acquire))
            {
                self.active.take().unwrap().cancel(gl);
            }
            if self.active.is_none() {
                let job = queue
                    .pending
                    .lock()
                    .ok()
                    .and_then(|mut jobs| jobs.pop_front());
                if let Some(Job { epoch, kind }) = job {
                    if epoch != queue.epoch.load(Ordering::Acquire) {
                        return;
                    }
                    self.active = match kind {
                        Kind::Image {
                            frame,
                            size,
                            sender,
                        } => match Capture::new(gl, frame, size) {
                            Ok(capture) => Some(Active::Image {
                                epoch,
                                capture: Box::new(capture),
                                sender,
                            }),
                            Err(error) => {
                                let _ = sender.send(Err(error));
                                None
                            }
                        },
                        Kind::Bake {
                            model,
                            request,
                            sender,
                        } => match bake::Renderer::new(gl, &model, request) {
                            Ok(renderer) => Some(Active::Bake {
                                epoch,
                                renderer: Box::new(renderer),
                                sender,
                            }),
                            Err(error) => {
                                let _ = sender.send(Err(error));
                                None
                            }
                        },
                    };
                }
            }
            let Some(mut active) = self.active.take() else {
                return;
            };
            let mut failed = false;
            let done = match &mut active {
                Active::Image {
                    capture, sender, ..
                } => match capture.step(gl) {
                    Ok(Some(image)) => {
                        let _ = sender.send(Ok(image));
                        true
                    }
                    Ok(None) => false,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        true
                    }
                },
                Active::Bake {
                    renderer, sender, ..
                } => match renderer.step(gl) {
                    Ok(done) => done,
                    Err(error) => {
                        failed = true;
                        let _ = sender.send(Err(error));
                        true
                    }
                },
            };
            if done {
                match active {
                    Active::Image { capture, .. } => capture.delete(gl),
                    Active::Bake {
                        renderer, sender, ..
                    } => {
                        let painted = renderer.finish(gl);
                        if !failed {
                            let _ = sender.send(Ok(painted));
                        }
                    }
                }
            } else {
                self.active = Some(active);
            }
        }
    }
}

struct Capture {
    objects: resource::Objects,
    state: State,
    frame: Frame,
    size: [i32; 2],
    framebuffer: glow::Framebuffer,
}
impl Capture {
    unsafe fn new(gl: &glow::Context, frame: Frame, size: [usize; 2]) -> Result<Self, String> {
        let mut objects = resource::Objects::default();
        // SAFETY: allocation is confined to the current paint context and checked dimensions.
        let result = unsafe {
            (|| {
                let maximum = gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE).max(0) as usize;
                if size.iter().any(|&axis| axis == 0 || axis > maximum) {
                    return Err("The requested image exceeds this GPU's size limit.".into());
                }
                let size = size.map(|axis| axis as i32);
                let framebuffer = objects.framebuffer(gl)?;
                let texture = objects.texture(gl)?;
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    size[0],
                    size[1],
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::NEAREST as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::NEAREST as i32,
                );
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(texture),
                    0,
                );
                if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                    return Err("Could not allocate the saved image's GPU target.".into());
                }
                Ok((framebuffer, size))
            })()
        };
        match result {
            Ok((framebuffer, size)) => Ok(Self {
                objects,
                state: State::default(),
                frame,
                size,
                framebuffer,
            }),
            Err(error) => {
                // SAFETY: partial handles were allocated above on this context.
                unsafe { objects.delete(gl) };
                Err(error)
            }
        }
    }
    unsafe fn step(&mut self, gl: &glow::Context) -> Result<Option<egui::ColorImage>, String> {
        // SAFETY: draw_to and readback use this capture's owned framebuffer.
        unsafe {
            if !self
                .state
                .draw_to(gl, &self.frame, self.size, Some(self.framebuffer), [0, 0])
            {
                return self
                    .state
                    .fallback
                    .as_ref()
                    .map_or(Ok(None), |(_, reason)| Err(reason.clone()));
            }
            let [width, height] = self.size.map(|n| n as usize);
            let mut rgba = vec![0_u8; width * height * 4];
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(self.framebuffer));
            gl.read_buffer(glow::COLOR_ATTACHMENT0);
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(
                0,
                0,
                self.size[0],
                self.size[1],
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut rgba)),
            );
            if gl.get_error() != glow::NO_ERROR {
                return Err("The GPU could not read the saved image.".into());
            }
            let mut pixels = Vec::with_capacity(width * height);
            for row in rgba.chunks_exact(width * 4).rev() {
                pixels.extend(
                    row.chunks_exact(4)
                        .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])),
                );
            }
            Ok(Some(egui::ColorImage::new([width, height], pixels)))
        }
    }
    unsafe fn delete(mut self, gl: &glow::Context) {
        // SAFETY: all resources belong to this current capture context.
        unsafe {
            if let Some(uploaded) = self.state.model.take() {
                uploaded.delete(gl)
            }
            if let Some(target) = self.state.target.take() {
                target.delete(gl)
            }
            if let Some((program, _)) = self.state.program.take() {
                gl.delete_program(program)
            }
            self.objects.delete(gl);
        }
    }
}
