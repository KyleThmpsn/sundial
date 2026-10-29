//! Compiles preview programs in helper processes and hands back the driver's program binaries for
//! the painter to load. A driver can do a program's whole compile when the program links or when
//! its status is first read, and on the paint thread that stopped every window. Contexts on worker
//! threads in this process did not help: the NVIDIA driver kept the painter waiting until every
//! worker's compile ended, 23.9 s for one imported weapon's 15 programs. A helper is this
//! executable started with [`ARGUMENT`], so a driver's locks in it never reach the painter.
//! Windows only, through WGL.
use super::{Source, binary_now};
use eframe::glow::{self, HasContext};
use std::{
    io::{BufReader, BufWriter, Read, Write},
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use windows_sys::Win32::{
    Graphics::{
        Gdi::GetDC,
        OpenGL::{
            ChoosePixelFormat, HGLRC, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW, PFD_SUPPORT_OPENGL,
            PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR, SetPixelFormat, wglCreateContext,
            wglDeleteContext, wglGetProcAddress, wglMakeCurrent,
        },
    },
    System::LibraryLoader::{GetProcAddress, LoadLibraryA},
    UI::WindowsAndMessaging::{CreateWindowExW, WS_POPUP},
};

/// The argument that starts this executable as a program compiler.
const ARGUMENT: &str = "--preview-program-compiler";
/// Helper processes, each compiling one program at a time.
const HELPERS: usize = 3;
/// Starts no console window for a helper.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
    /// Set once every helper has stopped, so jobs left with them will never finish.
    stopped: Arc<AtomicBool>,
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
        let stopped = Arc::new(AtomicBool::new(false));
        for _ in 0..HELPERS {
            let Ok(mut child) = Command::new(&program)
                .arg(ARGUMENT)
                .args(version.map(|value| value.to_string()))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
            else {
                continue;
            };
            let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
                let _ = child.kill();
                continue;
            };
            *running.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            let queue = Arc::clone(&queue);
            let finished = finished.clone();
            let (helpers, all_stopped) = (Arc::clone(&running), Arc::clone(&stopped));
            let spawned = std::thread::Builder::new()
                .name("preview program helper".into())
                .spawn(move || {
                    exchange(&queue, &finished, input, output);
                    let _ = child.kill();
                    let _ = child.wait();
                    let mut helpers = helpers.lock().unwrap_or_else(PoisonError::into_inner);
                    *helpers -= 1;
                    if *helpers == 0 {
                        all_stopped.store(true, Ordering::Release);
                    }
                });
            if spawned.is_err() {
                *running.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
            }
        }
        if *running.lock().unwrap_or_else(PoisonError::into_inner) == 0 {
            return None;
        }
        Some(Self {
            jobs,
            done,
            stopped,
        })
    }

    /// Queues `source` to compile as job `id`. False once every helper has stopped.
    pub fn submit(&self, id: u64, source: Arc<Source>) -> bool {
        !self.stopped() && self.jobs.send((id, source)).is_ok()
    }

    /// The jobs finished since the last call.
    pub fn finished(&self) -> Vec<Done> {
        self.done.try_iter().collect()
    }

    /// Whether every helper has stopped.
    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
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
fn exchange(
    queue: &Mutex<mpsc::Receiver<Job>>,
    finished: &mpsc::Sender<Done>,
    input: std::process::ChildStdin,
    output: std::process::ChildStdout,
) {
    let mut input = BufWriter::new(input);
    let mut output = BufReader::new(output);
    let mut ready = [0u8];
    if output.read_exact(&mut ready).is_err() || ready[0] != 1 {
        return;
    }
    loop {
        let job = queue.lock().unwrap_or_else(PoisonError::into_inner).recv();
        let Ok((id, source)) = job else {
            return;
        };
        let sent = write_text(&mut input, &source.vertex)
            .and_then(|()| write_text(&mut input, &source.fragment))
            .and_then(|()| input.flush());
        // A helper that stops mid-job fails that job, rather than leaving it unanswered.
        let (result, lost) = match sent.and_then(|()| read_result(&mut output)) {
            Ok(result) => (result, false),
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

fn write_text(writer: &mut impl Write, text: &str) -> std::io::Result<()> {
    let length = u32::try_from(text.len()).map_err(std::io::Error::other)?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(text.as_bytes())
}

fn read_bytes(reader: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let mut bytes = vec![0; u32::from_le_bytes(length) as usize];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// A binary as its format and bytes, or an error as its text, after a byte saying which.
fn read_result(reader: &mut impl Read) -> std::io::Result<Result<glow::ProgramBinary, String>> {
    let mut kind = [0u8];
    reader.read_exact(&mut kind)?;
    if kind[0] == 1 {
        let mut format = [0u8; 4];
        reader.read_exact(&mut format)?;
        let buffer = read_bytes(reader)?;
        return Ok(Ok(glow::ProgramBinary {
            buffer,
            format: u32::from_le_bytes(format),
        }));
    }
    Ok(Err(
        String::from_utf8_lossy(&read_bytes(reader)?).into_owned()
    ))
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
    let numbers: Vec<i32> = arguments[1..]
        .iter()
        .filter_map(|value| value.parse().ok())
        .collect();
    let &[major, minor, profile] = numbers.as_slice() else {
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
    if output
        .write_all(&[1])
        .and_then(|()| output.flush())
        .is_err()
    {
        return Some(1);
    }
    loop {
        let (Ok(vertex), Ok(fragment)) = (read_bytes(&mut input), read_bytes(&mut input)) else {
            return Some(0);
        };
        let source = Source::from_text(
            String::from_utf8_lossy(&vertex).into_owned(),
            String::from_utf8_lossy(&fragment).into_owned(),
        );
        // SAFETY: the helper's context is current on this thread.
        let result = unsafe { binary_now(&gl, &source) };
        let written = match result {
            Ok(binary) => output
                .write_all(&[1])
                .and_then(|()| output.write_all(&binary.format.to_le_bytes()))
                .and_then(|()| {
                    output.write_all(&(binary.buffer.len() as u32).to_le_bytes())?;
                    output.write_all(&binary.buffer)
                }),
            Err(error) => output
                .write_all(&[0])
                .and_then(|()| write_text(&mut output, &error)),
        };
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
