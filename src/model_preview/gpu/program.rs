//! The preview's shader programs. Surfaces without a native material share one program, and each
//! native material has its own, so no program carries every material's code. A single program
//! holding all of an imported weapon's materials took the driver over 30 seconds to compile, on
//! the paint thread. On Windows, programs compile in helper processes, and the painter loads the
//! driver's program binaries. Elsewhere, where the driver offers
//! `GL_KHR_parallel_shader_compile`, they link on its threads and the paint thread asks whether
//! they are done, which some drivers still answer by waiting. Linked programs are kept per GL
//! context and shared by every preview and export that asks for the same text.
use super::*;
use crate::model_preview::effects::native::{self, Scope};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::PoisonError,
    time::Instant,
};

#[cfg(windows)]
mod linker;

/// Linked programs no preview holds stay this many per context, for a model shown again.
const KEPT: usize = 96;

/// A program's vertex and fragment text, with a hash that tells most different texts apart
/// before they are compared.
#[derive(PartialEq, Eq)]
pub(super) struct Source {
    key: u64,
    vertex: String,
    fragment: String,
}

impl Source {
    fn new(model: &Model, scope: Scope) -> Self {
        let mut fragment = crate::model_preview::effects::with_rows(&FRAGMENT.replace(
            "// EFFECT FUNCTIONS",
            &format!(
                "{}\n{}",
                include_str!("../effects/shader.glsl"),
                native::source(model, false, scope)
            ),
        ));
        if fragment.contains("dFdxFine(") || fragment.contains("dFdyFine(") {
            fragment = fragment.replacen(
                "#version 330 core",
                "#version 330 core\n#extension GL_ARB_derivative_control : require",
                1,
            );
        }
        let mut vertex = crate::model_preview::effects::with_rows(&VERTEX.replace(
            "// NATIVE VERTEX FUNCTIONS",
            &native::source(model, true, scope),
        ));
        if let Some(salt) = crate::model_preview::timing::shader_salt() {
            for text in [&mut vertex, &mut fragment] {
                text.push_str("\n// ");
                text.push_str(salt);
                text.push('\n');
            }
        }
        Self::from_text(vertex, fragment)
    }

    fn from_text(vertex: String, fragment: String) -> Self {
        let mut hasher = DefaultHasher::new();
        vertex.hash(&mut hasher);
        fragment.hash(&mut hasher);
        Self {
            key: hasher.finish(),
            vertex,
            fragment,
        }
    }
}

/// Runs this process as a preview program compiler when it was started as one, returning its exit
/// code. `None` for an ordinary start.
#[cfg(windows)]
pub(super) fn serve_compiler() -> Option<i32> {
    linker::serve()
}

/// The text of every program a model draws with. A model's text can run to megabytes, so the
/// preparation worker writes it.
pub(super) struct Sources {
    base: Arc<Source>,
    /// Each material's program among `variants`, for materials with a native program.
    materials: Vec<Option<usize>>,
    variants: Vec<Arc<Source>>,
}

impl Sources {
    pub fn new(model: &Model) -> Self {
        let base = Arc::new(Source::new(model, Scope::Base));
        let mut variants: Vec<Arc<Source>> = Vec::new();
        let materials = (0..model.effects.len())
            .map(|index| {
                model.effects[index].native.as_ref()?;
                let source = Source::new(model, Scope::Material(index));
                Some(
                    match variants.iter().position(|variant| **variant == source) {
                        Some(at) => at,
                        None => {
                            variants.push(Arc::new(source));
                            variants.len() - 1
                        }
                    },
                )
            })
            .collect();
        Self {
            base,
            materials,
            variants,
        }
    }
}

pub(super) struct Linked {
    pub program: glow::Program,
    pub uniforms: Uniforms,
}

enum Slot {
    Waiting(Arc<Source>),
    Linked(Arc<Linked>),
}

/// One model's programs, held by the preview that draws it.
pub(super) struct Programs {
    base: Slot,
    materials: Vec<Option<usize>>,
    variants: Vec<Slot>,
    ready: bool,
    asked: Instant,
}

impl Programs {
    pub fn new(sources: Sources) -> Self {
        Self {
            base: Slot::Waiting(sources.base),
            materials: sources.materials,
            variants: sources.variants.into_iter().map(Slot::Waiting).collect(),
            ready: false,
            asked: Instant::now(),
        }
    }

    /// Whether every program has linked. Never waits on the driver where it links on its own
    /// threads. Elsewhere it links at most one program per call, so frames stay short.
    pub unsafe fn poll(&mut self, gl: &glow::Context) -> Result<bool, String> {
        if self.ready {
            return Ok(true);
        }
        let mut ready = true;
        // SAFETY: the caller supplies the paint thread's current context.
        unsafe {
            with_library(gl, |library| {
                let mut budget = true;
                for slot in std::iter::once(&mut self.base).chain(self.variants.iter_mut()) {
                    if let Slot::Waiting(source) = slot {
                        match library.poll(gl, source, &mut budget)? {
                            Some(linked) => *slot = Slot::Linked(linked),
                            None => ready = false,
                        }
                    }
                }
                library.collect(gl);
                Ok::<_, String>(())
            })?;
        }
        if ready {
            self.ready = true;
            let elapsed = self.asked.elapsed();
            crate::model_preview::timing::mark(
                || {
                    format!(
                        "shaders ready: {} programs after {:.0} ms",
                        1 + self.variants.len(),
                        elapsed.as_secs_f64() * 1000.0
                    )
                },
                "paint",
            );
        }
        Ok(ready)
    }

    /// The program a group drawn with material `effect` uses, once every program has linked.
    pub fn of(&self, effect: Option<usize>) -> Option<&Linked> {
        let slot = effect
            .and_then(|index| self.materials.get(index).copied().flatten())
            .and_then(|variant| self.variants.get(variant))
            .unwrap_or(&self.base);
        match slot {
            Slot::Linked(linked) => Some(linked),
            Slot::Waiting(_) => None,
        }
    }
}

enum State {
    Queued,
    /// Linking on the paint thread's context.
    Linking {
        program: glow::Program,
        shaders: [glow::Shader; 2],
    },
    /// Linking on a worker, as this job.
    #[cfg_attr(not(windows), allow(dead_code))]
    Submitted(u64),
    Linked(Arc<Linked>),
    Failed(String),
}

struct Entry {
    source: Arc<Source>,
    state: State,
    used: u64,
}

/// The programs of one GL context. Programs belong to the context that linked them, and eframe
/// draws every viewport with one context.
struct Library {
    context: usize,
    parallel: bool,
    #[cfg(windows)]
    linker: Option<linker::Linker>,
    #[cfg(windows)]
    jobs: u64,
    entries: Vec<Entry>,
    clock: u64,
}

static LIBRARIES: Mutex<Vec<Library>> = Mutex::new(Vec::new());

unsafe fn with_library<T>(gl: &glow::Context, work: impl FnOnce(&mut Library) -> T) -> T {
    let context = std::ptr::from_ref(gl) as usize;
    let mut libraries = LIBRARIES.lock().unwrap_or_else(PoisonError::into_inner);
    let at = match libraries
        .iter()
        .position(|library| library.context == context)
    {
        Some(at) => at,
        None => {
            let extensions = gl.supported_extensions();
            let parallel = extensions.contains("GL_KHR_parallel_shader_compile")
                || extensions.contains("GL_ARB_parallel_shader_compile");
            if parallel {
                // SAFETY: the extension that defines this call is present on this context.
                unsafe { gl.max_shader_compiler_threads(u32::MAX) };
            }
            libraries.push(Library {
                context,
                parallel,
                #[cfg(windows)]
                // SAFETY: the painter's context is current, as `with_library` requires.
                linker: unsafe { linker::Linker::start(gl) },
                #[cfg(windows)]
                jobs: 0,
                entries: Vec::new(),
                clock: 0,
            });
            libraries.len() - 1
        }
    };
    let library = &mut libraries[at];
    // SAFETY: the painter's context is current, as `with_library` requires.
    unsafe { library.receive(gl) };
    work(library)
}

impl Library {
    /// Loads the binaries the helpers compiled.
    unsafe fn receive(&mut self, gl: &glow::Context) {
        #[cfg(windows)]
        if let Some(linker) = &self.linker {
            let stopped = linker.stopped();
            for (job, result) in linker.finished() {
                let Some(at) = self
                    .entries
                    .iter()
                    .position(|entry| matches!(entry.state, State::Submitted(id) if id == job))
                else {
                    continue;
                };
                // SAFETY: the painter's context is current, and the binary came from a context
                // with its version and profile.
                let loaded = result.map(|binary| unsafe { load(gl, &binary) });
                self.entries[at].state = match loaded {
                    Ok(Some(linked)) => State::Linked(Arc::new(linked)),
                    Err(error) => State::Failed(error),
                    Ok(None) => {
                        // The driver refused its own binary. The paint thread links from here on.
                        self.abandon_linker();
                        return;
                    }
                };
            }
            // Jobs left with helpers that all stopped will never finish.
            if stopped {
                self.abandon_linker();
            }
        }
        let _ = gl;
    }

    /// Stops handing programs to the helpers. Programs they hold link on the paint thread.
    #[cfg(windows)]
    fn abandon_linker(&mut self) {
        self.linker = None;
        for entry in &mut self.entries {
            if matches!(entry.state, State::Submitted(_)) {
                entry.state = State::Queued;
            }
        }
    }

    /// Queues `source` with the workers, returning its job, or `None` without them.
    fn submit(&mut self, source: &Arc<Source>) -> Option<u64> {
        #[cfg(windows)]
        if let Some(linker) = &self.linker {
            self.jobs += 1;
            if linker.submit(self.jobs, Arc::clone(source)) {
                return Some(self.jobs);
            }
            // Every helper stopped. The paint thread links from here on.
            self.abandon_linker();
        }
        let _ = source;
        None
    }

    /// The linked program for `source`, or `None` while it compiles. `source` becomes the kept
    /// copy, so later calls find it without comparing text. Without parallel linking, a program
    /// links only while `budget` holds, which this spends.
    unsafe fn poll(
        &mut self,
        gl: &glow::Context,
        source: &mut Arc<Source>,
        budget: &mut bool,
    ) -> Result<Option<Arc<Linked>>, String> {
        self.clock += 1;
        let at = match self
            .entries
            .iter()
            .position(|entry| Arc::ptr_eq(&entry.source, source) || *entry.source == **source)
        {
            Some(at) => at,
            None => {
                self.entries.push(Entry {
                    source: Arc::clone(source),
                    state: State::Queued,
                    used: 0,
                });
                self.entries.len() - 1
            }
        };
        *source = Arc::clone(&self.entries[at].source);
        self.entries[at].used = self.clock;
        if matches!(self.entries[at].state, State::Queued)
            && let Some(job) = self.submit(source)
        {
            self.entries[at].state = State::Submitted(job);
        }
        let parallel = self.parallel;
        let entry = &mut self.entries[at];
        // SAFETY: the caller supplies the current context these programs belong to.
        unsafe {
            if matches!(entry.state, State::Queued) {
                if !parallel && !std::mem::take(budget) {
                    return Ok(None);
                }
                entry.state = start(gl, &entry.source, false).unwrap_or_else(State::Failed);
            }
            if let State::Linking { program, shaders } = entry.state {
                if parallel && !gl.get_program_completion_status(program) {
                    return Ok(None);
                }
                entry.state = match finish(gl, program, shaders) {
                    Ok(linked) => State::Linked(Arc::new(linked)),
                    Err(error) => State::Failed(error),
                };
            }
        }
        match &entry.state {
            State::Linked(linked) => Ok(Some(Arc::clone(linked))),
            State::Failed(error) => Err(error.clone()),
            State::Queued | State::Linking { .. } | State::Submitted(_) => Ok(None),
        }
    }

    /// Deletes the least recently asked-for programs no preview holds, past the kept count.
    unsafe fn collect(&mut self, gl: &glow::Context) {
        while self.entries.len() > KEPT {
            let unused = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| match &entry.state {
                    State::Linked(linked) => Arc::strong_count(linked) == 1,
                    State::Failed(_) | State::Queued => true,
                    State::Linking { .. } | State::Submitted(_) => false,
                })
                .min_by_key(|(_, entry)| entry.used)
                .map(|(at, _)| at);
            let Some(at) = unused else {
                break;
            };
            if let State::Linked(linked) = self.entries.remove(at).state {
                // SAFETY: no preview holds this program, and it belongs to this context.
                unsafe { gl.delete_program(linked.program) };
            }
        }
    }
}

/// Compiles and links `source`, waiting for the driver, and returns the program's binary, as a
/// helper process does.
#[cfg(windows)]
unsafe fn binary_now(gl: &glow::Context, source: &Source) -> Result<glow::ProgramBinary, String> {
    // SAFETY: the caller's context is current on its thread.
    unsafe {
        let State::Linking { program, shaders } = start(gl, source, true)? else {
            return Err("The preview program did not start linking.".into());
        };
        check(gl, program, shaders)?;
        let binary = gl.get_program_binary(program);
        gl.delete_program(program);
        binary
            .filter(|binary| !binary.buffer.is_empty())
            .ok_or_else(|| "The driver returned no binary for a preview program.".into())
    }
}

/// A program from a worker's binary, or `None` when the driver refuses it.
#[cfg(windows)]
unsafe fn load(gl: &glow::Context, binary: &glow::ProgramBinary) -> Option<Linked> {
    // SAFETY: the caller's context is current, and a refused program is deleted here.
    unsafe {
        let program = gl.create_program().ok()?;
        gl.program_binary(program, binary);
        if !gl.get_program_link_status(program) {
            gl.delete_program(program);
            return None;
        }
        Some(Linked {
            program,
            uniforms: Uniforms::locate(gl, program),
        })
    }
}

/// Compiles and links without asking how either went, which would wait for the driver. A
/// `retrievable` program can give its binary afterwards.
unsafe fn start(gl: &glow::Context, source: &Source, retrievable: bool) -> Result<State, String> {
    // SAFETY: objects are created on the supplied context and released on failure.
    unsafe {
        let program = gl.create_program()?;
        let mut shaders = Vec::with_capacity(2);
        for (kind, text) in [
            (glow::VERTEX_SHADER, &source.vertex),
            (glow::FRAGMENT_SHADER, &source.fragment),
        ] {
            let shader = match gl.create_shader(kind) {
                Ok(shader) => shader,
                Err(error) => {
                    for shader in shaders {
                        gl.delete_shader(shader);
                    }
                    gl.delete_program(program);
                    return Err(error);
                }
            };
            gl.shader_source(shader, text);
            gl.compile_shader(shader);
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        gl.transform_feedback_varyings(program, &["vSortDepth"], glow::INTERLEAVED_ATTRIBS);
        if retrievable {
            gl.program_binary_retrievable_hint(program, true);
        }
        gl.link_program(program);
        Ok(State::Linking {
            program,
            shaders: [shaders[0], shaders[1]],
        })
    }
}

unsafe fn finish(
    gl: &glow::Context,
    program: glow::Program,
    shaders: [glow::Shader; 2],
) -> Result<Linked, String> {
    // SAFETY: the program and shaders were created on this context by `start`.
    unsafe {
        check(gl, program, shaders)?;
        Ok(Linked {
            program,
            uniforms: Uniforms::locate(gl, program),
        })
    }
}

/// Waits for the link, then releases the shaders, and the program too when it failed.
unsafe fn check(
    gl: &glow::Context,
    program: glow::Program,
    shaders: [glow::Shader; 2],
) -> Result<(), String> {
    // SAFETY: the program and shaders were created on this context by `start`.
    unsafe {
        let linked = gl.get_program_link_status(program);
        let log = (!linked).then(|| {
            let mut log = String::new();
            for shader in shaders {
                if !gl.get_shader_compile_status(shader) {
                    log.push_str(&gl.get_shader_info_log(shader));
                }
            }
            log.push_str(&gl.get_program_info_log(program));
            log
        });
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        if let Some(log) = log {
            eprintln!("Model preview shader failed to compile or link: {log}");
            gl.delete_program(program);
            return Err(
                "Software rendering is active because this model's GPU shader could not initialize."
                    .into(),
            );
        }
        Ok(())
    }
}
