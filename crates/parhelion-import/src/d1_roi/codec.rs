//! Streaming Oodle 3 reads stop at input exhaustion, including short D1 blocks.
use anyhow::{Context, Result, ensure};
use libloading::Library;
use std::{ffi::c_void, path::PathBuf};

type Create = unsafe extern "C" fn(i32, isize, *mut c_void, isize) -> *mut c_void;
type Destroy = unsafe extern "C" fn(*mut c_void);
type Decode = unsafe extern "C" fn(
    *mut c_void,
    *mut Step,
    *mut u8,
    isize,
    isize,
    isize,
    *const u8,
    isize,
    i32,
    i32,
    i32,
    i32,
) -> i32;

#[derive(Default)]
#[repr(C)]
struct Step {
    decoded: i32,
    consumed: i32,
    quantum_decoded: i32,
    quantum_consumed: i32,
}

pub(super) struct Decoder {
    _library: Library,
    create: Create,
    destroy: Destroy,
    decode: Decode,
}

struct Instance {
    pointer: *mut c_void,
    destroy: Destroy,
}
impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: The pointer came from this library's successful Create call and is owned here.
        unsafe { (self.destroy)(self.pointer) };
    }
}

impl Decoder {
    pub fn open() -> Result<Self> {
        let name = if cfg!(windows) {
            "oo2core_3_win64.dll"
        } else {
            "liblinoodle3.so"
        };
        let paths = if let Some(path) = std::env::var_os("PARHELION_D1_OODLE") {
            vec![PathBuf::from(path)]
        } else {
            vec![
                std::env::current_exe()?.with_file_name(name),
                std::env::current_dir()?.join(name),
                PathBuf::from(name),
            ]
        };
        let mut failures = Vec::new();
        for path in paths {
            // SAFETY: This is the user-provided decoder, following the same library lookup as Tiger.
            let library = match unsafe { Library::new(&path) } {
                Ok(library) => library,
                Err(error) => {
                    failures.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            // SAFETY: These are Oodle 3's exported C entry points. The library outlives the pointers.
            let (create, destroy, decode) = unsafe {
                (
                    *library.get::<Create>(b"OodleLZDecoder_Create")?,
                    *library.get::<Destroy>(b"OodleLZDecoder_Destroy")?,
                    *library.get::<Decode>(b"OodleLZDecoder_DecodeSome")?,
                )
            };
            return Ok(Self {
                _library: library,
                create,
                destroy,
                decode,
            });
        }
        anyhow::bail!(
            "Rise of Iron needs the Oodle 3 decoder. Set PARHELION_D1_OODLE to its path. {}",
            failures.join("\n")
        )
    }

    pub fn decode(&self, stored: &[u8]) -> Result<Vec<u8>> {
        const CAPACITY: usize = 0x40000;
        ensure!(
            !stored.is_empty() && stored.len() <= CAPACITY,
            "Invalid compressed block size"
        );
        // SAFETY: Unknown compressor -1 selects the codec from the input. Null scratch requests
        // library-owned memory. The instance is destroyed on every exit while the library lives.
        let pointer = unsafe { (self.create)(-1, CAPACITY as isize, std::ptr::null_mut(), 0) };
        ensure!(
            !pointer.is_null(),
            "Could not create Rise of Iron block decoder"
        );
        let instance = Instance {
            pointer,
            destroy: self.destroy,
        };
        // The decoder writes only the quantum it decodes within the room offered below, measured
        // over every block of three packages, so the slack is never reached.
        let mut output = vec![0; CAPACITY + 64];
        let (mut consumed, mut decoded) = (0usize, 0usize);
        while consumed < stored.len() {
            crate::cancellation::check()?;
            ensure!(
                decoded < CAPACITY,
                "Compressed block exceeds its decoded capacity"
            );
            let mut step = Step::default();
            // SAFETY: Both slices remain allocated for the call. Positions and available lengths
            // are bounded above. Fuzz-safe and CRC checks are enabled, with both decode phases.
            let ok = unsafe {
                (self.decode)(
                    instance.pointer,
                    &mut step,
                    output.as_mut_ptr(),
                    decoded as isize,
                    CAPACITY as isize,
                    (CAPACITY - decoded) as isize,
                    stored[consumed..].as_ptr(),
                    (stored.len() - consumed) as isize,
                    1,
                    1,
                    0,
                    3,
                )
            };
            ensure!(
                ok != 0 && step.decoded > 0 && step.consumed > 0,
                "Rise of Iron block decompression failed"
            );
            decoded = decoded
                .checked_add(step.decoded as usize)
                .context("Decoded size overflow")?;
            consumed = consumed
                .checked_add(step.consumed as usize)
                .context("Compressed size overflow")?;
            ensure!(
                decoded <= CAPACITY && consumed <= stored.len(),
                "Decoder exceeded block bounds"
            );
        }
        output.truncate(decoded);
        Ok(output)
    }
}
