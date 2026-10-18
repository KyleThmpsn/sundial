//! Checked calls to the source platform's XG layout implementation.
use super::*;

#[cfg(not(windows))]
pub(super) fn linearize(
    _: &Payload,
    _: &[u8],
    _: &[(usize, usize)],
    _: Option<&Path>,
) -> Result<Vec<u8>> {
    anyhow::bail!("Tiled Xbox textures require the Windows XG library")
}

#[cfg(windows)]
pub(super) fn linearize(
    header: &Payload,
    raw: &[u8],
    levels: &[(usize, usize)],
    path: Option<&Path>,
) -> Result<Vec<u8>> {
    use std::ffi::c_void;
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;
    type Layout = unsafe extern "system" fn(*mut c_void, *mut u8) -> i32;
    type Copy =
        unsafe extern "system" fn(*mut c_void, *mut u8, u32, u32, *const u8, u32, u32) -> i32;
    type Create = unsafe extern "system" fn(*const u32, *mut *mut c_void) -> i32;
    let configured = path
        .map(Path::to_owned)
        .or_else(|| std::env::var_os("PARHELION_D1_XG").map(PathBuf::from));
    let path = configured.unwrap_or_else(|| PathBuf::from("xgpc.dll"));
    // SAFETY: The configured platform library exposes the checked 2014 or 2018
    // XG ABI. It stays loaded until all objects and calls below have finished.
    let library = unsafe { libloading::Library::new(&path) }
        .with_context(|| format!("Load Xbox texture library {}", path.display()))?;
    let descriptor = [
        u32::from(header.u16(0x30)?),
        u32::from(header.u16(0x32)?),
        u32::from(header.u8(0x39)?),
        u32::from(header.u16(0x36)?),
        header.u32(0)?,
        1,
        0,
        0,
        8,
        0,
        if header.u16(0x36)? == 6 { 4 } else { 0 },
        0,
        0,
        header.u32(4)?,
        0,
    ];
    ensure!(
        matches!(descriptor[13], 13 | 14),
        "Unsupported Xbox tile mode {}",
        descriptor[13]
    );
    let mut object = std::ptr::null_mut();
    // SAFETY: Both descriptor and output storage remain live. Symbols are
    // resolved with their SDK signatures before use.
    let (status, shift) = unsafe {
        let version = library.get::<unsafe extern "system" fn(u32)>(b"XGSetHardwareVersion\0");
        let shift = if let Ok(set) = version {
            set(0);
            2
        } else {
            0
        };
        let create = library.get::<Create>(b"XGCreateTexture2DComputer\0")?;
        (create(descriptor.as_ptr(), &mut object), shift)
    };
    ensure!(
        status >= 0 && !object.is_null(),
        "Xbox texture layout creation failed: {status:08X}"
    );
    struct Computer {
        object: *mut c_void,
        release: Release,
    }
    impl Drop for Computer {
        fn drop(&mut self) {
            // SAFETY: This wrapper owns the one XG reference returned by Create.
            unsafe {
                (self.release)(self.object);
            }
        }
    }
    // SAFETY: Create returned a live XGTextureAddressComputer. Slot 1 is Release
    // in both supported SDKs. The 2018 ABI adds two addressing methods before Copy.
    let (computer, layout, copy) = unsafe {
        let vtable = *(object as *const *const usize);
        (
            Computer {
                object,
                release: std::mem::transmute::<usize, Release>(*vtable.add(1)),
            },
            std::mem::transmute::<usize, Layout>(*vtable.add(2)),
            std::mem::transmute::<usize, Copy>(*vtable.add(7 + shift)),
        )
    };
    let mut storage = vec![0u64; 0x1740 / 8];
    // SAFETY: The layout buffer has the larger SDK size and eight-byte alignment.
    let status = unsafe { layout(computer.object, storage.as_mut_ptr().cast()) };
    ensure!(
        status >= 0 && storage[0] == raw.len() as u64,
        "Xbox tiled footprint differs from the complete source payload"
    );
    let mut result = Vec::new();
    for layer in 0..descriptor[3] {
        for (mip, &(row, length)) in levels.iter().enumerate() {
            crate::cancellation::check()?;
            let start = result.len();
            result.resize(start + length, 0);
            // SAFETY: The source size matched XG's complete resource layout.
            // Destination dimensions, row pitch and slice size are exact for
            // this mip. Both buffers remain live and cannot overlap.
            let status = unsafe {
                copy(
                    computer.object,
                    result[start..].as_mut_ptr(),
                    0,
                    layer * descriptor[2] + mip as u32,
                    raw.as_ptr(),
                    u32::try_from(row)?,
                    u32::try_from(length)?,
                )
            };
            ensure!(
                status >= 0,
                "Xbox texture subresource copy failed: {status:08X}"
            );
        }
    }
    Ok(result)
}
