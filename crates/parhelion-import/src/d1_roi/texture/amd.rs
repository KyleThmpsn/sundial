//! Direct, statically linked AMD surface layout and copy calls.
use anyhow::{Context, Result, ensure};
use std::{ffi::c_void, ptr::NonNull};

pub const SOURCE_URL: &str = "https://github.com/GPUOpen-Drivers/pal/tree/7ce51b199c72021b8d027c50b55da515fd9b2e0b/src/core/imported/addrlib";
pub const LICENSE: &str = include_str!("amd/LICENSE.txt");

#[repr(C)]
pub(super) struct Request {
    pub width: u32,
    pub height: u32,
    pub slices: u32,
    pub bits: u32,
    pub mode: u32,
    pub mip: u32,
    pub mips: u32,
    pub base_pitch: u32,
    pub volume: u32,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct Surface {
    pub bytes: u64,
    pub pitch: u32,
    height: u32,
    slices: u32,
    mode: u32,
    kind: u32,
    pub alignment: u32,
    bits: u32,
    banks: u32,
    bank_width: u32,
    bank_height: u32,
    aspect: u32,
    split: u32,
    pipe: u32,
}

const _: () = assert!(std::mem::size_of::<Request>() == 36);
const _: () = assert!(std::mem::size_of::<Surface>() == 64);

unsafe extern "C" {
    fn parhelion_addr_create() -> *mut c_void;
    fn parhelion_addr_destroy(library: *mut c_void);
    fn parhelion_addr_surface(
        library: *mut c_void,
        request: *const Request,
        result: *mut Surface,
    ) -> i32;
    fn parhelion_addr_copy(
        library: *mut c_void,
        surface: *const Surface,
        width: u32,
        height: u32,
        first_slice: u32,
        slices: u32,
        source: *const u8,
        source_size: usize,
        target: *mut u8,
        target_size: usize,
    ) -> i32;
}

/// Each decoding operation owns a separate AMD handle. No shared mutable native state.
pub(super) struct Library(NonNull<c_void>);

impl Library {
    pub fn new() -> Result<Self> {
        // SAFETY: The linked constructor takes no pointers and returns an owned handle or null.
        let pointer = unsafe { parhelion_addr_create() };
        Ok(Self(
            NonNull::new(pointer).context("Create AMD texture address library")?,
        ))
    }

    pub fn surface(&self, request: &Request) -> Result<Surface> {
        let mut result = Surface::default();
        // SAFETY: Private repr(C) records match bridge.h and remain valid throughout the call.
        let status = unsafe { parhelion_addr_surface(self.0.as_ptr(), request, &mut result) };
        ensure!(
            status == 0,
            "AMD surface layout failed for mip {}: {status}",
            request.mip
        );
        ensure!(
            result.alignment.is_power_of_two() && result.bytes > 0,
            "AMD surface layout returned invalid storage bounds"
        );
        Ok(result)
    }

    pub fn copy(
        &self,
        surface: &Surface,
        dimensions: (u32, u32),
        slices: (u32, u32),
        source: &[u8],
        target: &mut [u8],
    ) -> Result<()> {
        // SAFETY: The slices are disjoint and valid for their full lengths. The native bridge
        // validates dimensions, slice bounds and every source address before copying an element.
        let status = unsafe {
            parhelion_addr_copy(
                self.0.as_ptr(),
                surface,
                dimensions.0,
                dimensions.1,
                slices.0,
                slices.1,
                source.as_ptr(),
                source.len(),
                target.as_mut_ptr(),
                target.len(),
            )
        };
        ensure!(status == 0, "AMD texture address readback failed: {status}");
        Ok(())
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: This handle was created by the linked library and has exactly one owner.
        unsafe { parhelion_addr_destroy(self.0.as_ptr()) };
    }
}
