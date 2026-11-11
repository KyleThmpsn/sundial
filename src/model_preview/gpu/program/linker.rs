//! Compiles preview programs in helper processes and hands back the driver's program binaries for
//! the painter to load. A driver can do a program's whole compile when the program links or when
//! its status is first read, and on the paint thread that stopped every window. Contexts on worker
//! threads in this process did not help: the NVIDIA driver kept the painter waiting until every
//! worker's compile ended, 23.9 s for one imported weapon's 15 programs. A helper is this
//! executable started with [`ARGUMENT`], so a driver's locks in it never reach the painter.
//! Windows only, through WGL.
use super::{Source, cache::Cache, limit_compiler_threads};
mod message;
use eframe::glow::{self, HasContext};
use message::{read_result, read_text, write_result, write_text};
use std::{
    io::{BufReader, BufWriter, Read, Write},
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Graphics::{
        Gdi::GetDC,
        OpenGL::{
            ChoosePixelFormat, HGLRC, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW, PFD_SUPPORT_OPENGL,
            PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR, SetPixelFormat, wglCreateContext,
            wglDeleteContext, wglGetProcAddress, wglMakeCurrent,
        },
    },
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
        LibraryLoader::{GetProcAddress, LoadLibraryA},
    },
    UI::WindowsAndMessaging::{CreateWindowExW, WS_POPUP},
};

/// The argument that starts this executable as a program compiler.
const ARGUMENT: &str = "--preview-program-compiler";
/// Helper processes, each compiling one program at a time.
const HELPERS: usize = 2;
/// Starts no console window for a helper.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
/// How long a helper may take to make its context and say whether it worked.
const READY_DEADLINE: Duration = Duration::from_secs(30);
/// How long one program may take to compile in a helper. The slowest measured preview program
/// took seconds. A helper past this is stopped and its program fails, rather than leaving the
/// preview waiting for an answer that never comes.
const COMPILE_DEADLINE: Duration = Duration::from_secs(90);

const WGL_CONTEXT_MAJOR_VERSION_ARB: i32 = 0x2091;
const WGL_CONTEXT_MINOR_VERSION_ARB: i32 = 0x2092;
const WGL_CONTEXT_PROFILE_MASK_ARB: i32 = 0x9126;

type CreateContextAttribs =
    unsafe extern "system" fn(windows_sys::Win32::Graphics::Gdi::HDC, HGLRC, *const i32) -> HGLRC;
type Job = (u64, Arc<Source>);
pub(super) type Done = (u64, Result<glow::ProgramBinary, String>);

pub(super) struct Linker {
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Done>,
    /// Helpers still exchanging jobs. Startup can briefly have none while another starts.
    running: Arc<Mutex<usize>>,
    helpers: Vec<Watch>,
}

/// One helper process and when the answer it owes is due, which the paint thread checks.
struct Watch {
    child: Arc<Mutex<Child>>,
    /// While the helper owes an answer, the time it must arrive by.
    due: Arc<Mutex<Option<Instant>>>,
    /// Set when the helper was stopped for missing its deadline.
    late: Arc<AtomicBool>,
}

impl Watch {
    /// Stops the helper once its answer is overdue. Its pipes close, so the thread waiting on
    /// it fails the job and ends.
    fn enforce(&self) {
        let overdue = self
            .due
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some_and(|due| Instant::now() >= due);
        if overdue && !self.late.swap(true, Ordering::AcqRel) {
            self.stop();
        }
    }

    fn stop(&self) {
        let _ = self
            .child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .kill();
    }
}

impl Linker {
    /// Starts helpers whose contexts match the version and profile of the context current on
    /// this thread, the painter's. Returns at once. `None` when the painter cannot load program
    /// binaries or no helper executable can start, and programs then link on the paint thread.
    pub unsafe fn start(gl: &glow::Context) -> Option<Self> {
        // SAFETY: called from a paint callback, where the painter's context is current.
        let (formats, version) = unsafe {
            let formats = gl.get_parameter_i32(glow::NUM_PROGRAM_BINARY_FORMATS);
            let version = [
                gl.get_parameter_i32(glow::MAJOR_VERSION),
                gl.get_parameter_i32(glow::MINOR_VERSION),
                gl.get_parameter_i32(glow::CONTEXT_PROFILE_MASK),
            ];
            // Reading the painter's state leaves no error for its next draw to find.
            for _ in 0..8 {
                if gl.get_error() == glow::NO_ERROR {
                    break;
                }
            }
            (formats, version)
        };
        let program = helper_program()?;
        if formats <= 0 {
            return None;
        }
        let (jobs, queue) = mpsc::channel::<Job>();
        let queue = Arc::new(Mutex::new(queue));
        let (finished, done) = mpsc::channel();
        let running = Arc::new(Mutex::new(0usize));
        let mut helpers = Vec::new();
        for _ in 0..HELPERS {
            let Ok(mut child) = Command::new(&program)
                .arg(ARGUMENT)
                .args(version.map(|value| value.to_string()))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
                .spawn()
            else {
                continue;
            };
            let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
                let _ = child.kill();
                continue;
            };
            close_with_this_process(&child);
            let watch = Watch {
                child: Arc::new(Mutex::new(child)),
                due: Arc::new(Mutex::new(None)),
                late: Arc::new(AtomicBool::new(false)),
            };
            *running.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            let queue = Arc::clone(&queue);
            let finished = finished.clone();
            let count = Arc::clone(&running);
            let (child, due, late) = (
                Arc::clone(&watch.child),
                Arc::clone(&watch.due),
                Arc::clone(&watch.late),
            );
            let spawned = std::thread::Builder::new()
                .name("preview program helper".into())
                .spawn(move || {
                    exchange(&queue, &finished, (input, output), (&due, &late));
                    let mut child = child.lock().unwrap_or_else(PoisonError::into_inner);
                    let _ = child.kill();
                    let _ = child.wait();
                    drop(child);
                    let mut count = count.lock().unwrap_or_else(PoisonError::into_inner);
                    *count -= 1;
                });
            match spawned {
                Ok(_) => helpers.push(watch),
                Err(_) => {
                    *running.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
                    watch.stop();
                }
            }
        }
        if *running.lock().unwrap_or_else(PoisonError::into_inner) == 0 {
            return None;
        }
        Some(Self {
            jobs,
            done,
            running,
            helpers,
        })
    }

    /// Queues `source` to compile as job `id`. False once every helper has stopped.
    pub fn submit(&self, id: u64, source: Arc<Source>) -> bool {
        !self.stopped() && self.jobs.send((id, source)).is_ok()
    }

    /// The jobs finished since the last call. A helper whose answer is overdue is stopped here,
    /// and its job fails on a later call.
    pub fn finished(&self) -> Vec<Done> {
        for helper in &self.helpers {
            helper.enforce();
        }
        self.done.try_iter().collect()
    }

    /// Whether every helper has stopped.
    pub fn stopped(&self) -> bool {
        *self.running.lock().unwrap_or_else(PoisonError::into_inner) == 0
    }
}

impl Drop for Linker {
    fn drop(&mut self) {
        // No paint callback polls these deadlines after fallback. Stop the processes now,
        // leaving their exchange threads to drain the closed pipes and reap them.
        for helper in &self.helpers {
            helper.stop();
        }
    }
}

/// The executable a helper runs: `SUNDIAL_PREVIEW_COMPILER`, or this one when it is Sundial.
/// A test binary would read the argument as a test option, so tests name Sundial explicitly.
fn helper_program() -> Option<PathBuf> {
    if let Some(program) = std::env::var_os("SUNDIAL_PREVIEW_COMPILER") {
        return Some(PathBuf::from(program));
    }
    let program = std::env::current_exe().ok()?;
    program
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.to_ascii_lowercase().starts_with("sundial"))
        .then_some(program)
}

/// Feeds one helper jobs from the shared queue and returns their results, one at a time, until
/// the helper fails or the queue closes. The helper first says whether its context started.
/// Each answer is due by a deadline in `due`, which [`Watch::enforce`] holds the helper to.
fn exchange(
    queue: &Mutex<mpsc::Receiver<Job>>,
    finished: &mpsc::Sender<Done>,
    (input, output): (std::process::ChildStdin, std::process::ChildStdout),
    (due, late): (&Mutex<Option<Instant>>, &AtomicBool),
) {
    let owe = |deadline: Option<Duration>| {
        *due.lock().unwrap_or_else(PoisonError::into_inner) =
            deadline.map(|deadline| Instant::now() + deadline);
    };
    let mut input = BufWriter::new(input);
    let mut output = BufReader::new(output);
    let mut ready = [0u8];
    owe(Some(READY_DEADLINE));
    let started = output.read_exact(&mut ready).is_ok() && ready[0] == 1;
    owe(None);
    if !started {
        return;
    }
    loop {
        let job = queue.lock().unwrap_or_else(PoisonError::into_inner).recv();
        let Ok((id, source)) = job else {
            return;
        };
        owe(Some(COMPILE_DEADLINE));
        let sent = write_text(&mut input, &source.vertex)
            .and_then(|()| write_text(&mut input, &source.fragment))
            .and_then(|()| input.flush());
        let answer = sent.and_then(|()| read_result(&mut output));
        owe(None);
        // A helper that stops mid-job fails that job, rather than leaving it unanswered.
        let (result, lost) = match answer {
            Ok(result) => (result, false),
            Err(_) if late.load(Ordering::Acquire) => (
                Err(format!(
                    "The preview program compiler did not answer within {} s and was stopped",
                    COMPILE_DEADLINE.as_secs()
                )),
                true,
            ),
            Err(error) => (
                Err(format!("The preview program compiler stopped: {error}")),
                true,
            ),
        };
        if finished.send((id, result)).is_err() || lost {
            return;
        }
    }
}

/// Puts a helper in a job object that ends its processes when this process ends, so a helper
/// stuck in its driver cannot outlive Sundial. Best effort: a helper left outside the job still
/// exits once its input closes.
fn close_with_this_process(child: &Child) {
    /// A job handle, usable from any thread, kept open for the life of the process.
    struct Job(HANDLE);
    // SAFETY: a kernel object handle is not tied to the thread that opened it.
    unsafe impl Send for Job {}
    // SAFETY: the handle is only passed to thread-safe kernel calls.
    unsafe impl Sync for Job {}
    static JOB: OnceLock<Option<Job>> = OnceLock::new();
    let job = JOB.get_or_init(|| {
        // SAFETY: the job is created unnamed with default security, and its limits come from a
        // fully initialized structure of the size passed.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set == 0 {
                CloseHandle(job);
                return None;
            }
            Some(Job(job))
        }
    });
    if let Some(job) = job {
        // SAFETY: both handles are open, the child's for as long as `child` lives.
        unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) };
    }
}

/// Runs this process as a program compiler when it was started as one, returning its exit code.
/// It makes a hidden context with the version and profile it was given, says whether that
/// worked, then compiles each vertex and fragment text pair from standard input and writes the
/// result to standard output, until its input closes.
pub(super) fn serve() -> Option<i32> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) != Some(ARGUMENT) {
        return None;
    }
    let [_, major, minor, profile] = arguments.as_slice() else {
        return Some(2);
    };
    let (Ok(major), Ok(minor), Ok(profile)) = (major.parse(), minor.parse(), profile.parse())
    else {
        return Some(2);
    };
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = BufWriter::new(std::io::stdout().lock());
    // SAFETY: this process has no other GL context, and the one made here stays current on this
    // thread for the whole run.
    let Some(gl) = (unsafe { context(major, minor, profile) }) else {
        let _ = output.write_all(&[0]).and_then(|()| output.flush());
        return Some(1);
    };
    // SAFETY: this helper's sole context is current on its compilation thread.
    unsafe { limit_compiler_threads(&gl) };
    // SAFETY: the helper's sole context remains current while its cache is used.
    let cache = unsafe { Cache::new(&gl) };
    if output
        .write_all(&[1])
        .and_then(|()| output.flush())
        .is_err()
    {
        return Some(1);
    }
    loop {
        let Ok(vertex) = read_text(&mut input) else {
            return Some(0);
        };
        let Ok(fragment) = read_text(&mut input) else {
            return Some(0);
        };
        let source = Source::from_text(vertex, fragment);
        // SAFETY: the helper's context is current on this thread.
        let result = unsafe { cache.binary(&gl, &source) };
        let written = write_result(&mut output, &result);
        if written.and_then(|()| output.flush()).is_err() {
            return Some(0);
        }
    }
}

/// A context with the requested version and profile on a hidden window, made current here.
unsafe fn context(major: i32, minor: i32, profile: i32) -> Option<glow::Context> {
    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    // SAFETY: each handle is checked before use. The window and contexts last as long as the
    // helper process.
    unsafe {
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            [0u16].as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        if window.is_null() {
            return None;
        }
        let dc = GetDC(window);
        let mut descriptor: PIXELFORMATDESCRIPTOR = std::mem::zeroed();
        descriptor.nSize = std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as u16;
        descriptor.nVersion = 1;
        descriptor.dwFlags = PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
        descriptor.iPixelType = PFD_TYPE_RGBA;
        descriptor.cColorBits = 32;
        descriptor.cDepthBits = 24;
        descriptor.cStencilBits = 8;
        let format = ChoosePixelFormat(dc, &descriptor);
        if dc.is_null() || format <= 0 || SetPixelFormat(dc, format, &descriptor) == 0 {
            return None;
        }
        // The extension that makes a versioned context loads only through a current context.
        let legacy = wglCreateContext(dc);
        if legacy.is_null() || wglMakeCurrent(dc, legacy) == 0 {
            return None;
        }
        let create = wglGetProcAddress(c"wglCreateContextAttribsARB".as_ptr().cast())
            .filter(|function| !matches!(*function as usize, 1 | 2 | 3 | usize::MAX));
        let mut attributes = vec![
            WGL_CONTEXT_MAJOR_VERSION_ARB,
            major,
            WGL_CONTEXT_MINOR_VERSION_ARB,
            minor,
        ];
        if profile != 0 {
            attributes.extend([WGL_CONTEXT_PROFILE_MASK_ARB, profile]);
        }
        attributes.push(0);
        let context = create.map_or(std::ptr::null_mut(), |create| {
            let create = std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                CreateContextAttribs,
            >(create);
            create(dc, std::ptr::null_mut(), attributes.as_ptr())
        });
        wglMakeCurrent(std::ptr::null_mut(), std::ptr::null_mut());
        wglDeleteContext(legacy);
        if context.is_null() || wglMakeCurrent(dc, context) == 0 {
            return None;
        }
        let opengl = LoadLibraryA(c"opengl32.dll".as_ptr().cast());
        Some(glow::Context::from_loader_function_cstr(|name| {
            // WGL returns small sentinels for core functions, which opengl32 exports itself.
            match wglGetProcAddress(name.as_ptr().cast()) {
                Some(function) if !matches!(function as usize, 1 | 2 | 3 | usize::MAX) => {
                    function as *const std::ffi::c_void
                }
                _ => GetProcAddress(opengl, name.as_ptr().cast())
                    .map_or(std::ptr::null(), |function| {
                        function as *const std::ffi::c_void
                    }),
            }
        }))
    }
}
