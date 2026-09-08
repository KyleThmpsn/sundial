//! Shadowkeep's legacy Oodle 2.3 encoder. This module is built only for 64-bit Windows.

use std::{ffi::c_void, path::Path, ptr, sync::Mutex};

use libloading::Library;

use crate::format::BLOCK_SIZE;

// The installed version-3 DLL identifies its compatible header ABI with this value. Oodle 2.6
// added compression arguments, and newer versions changed the capacity-query signature.
const LEGACY_HEADER_VERSION: u32 = 0x2E03_0030;
const LEGACY_VERSION_FAMILY: u32 = 0x2E03_0000;
const LZH: i32 = 0;
const NORMAL: i32 = 4;
const COMPLETE_DECODE: i32 = 3;

type CheckVersion = unsafe extern "C" fn(u32, *mut u32) -> i32;
type Capacity = unsafe extern "C" fn(i64) -> i64;
type Compress = unsafe extern "C" fn(
    i32,
    *const u8,
    i64,
    *mut u8,
    i32,
    *const c_void,
    *const u8,
    *mut c_void,
) -> i64;
type Decompress = unsafe extern "C" fn(
    *const u8,
    i64,
    *mut u8,
    i64,
    i32,
    i32,
    i32,
    *mut c_void,
    i64,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    i64,
    i32,
) -> i64;

// Keep legacy encoder initialization and calls serialized across encoder instances. No global
// Oodle plugins or options are changed, and every call owns independent input and output buffers.
static NATIVE_CALLS: Mutex<()> = Mutex::new(());

pub(super) struct Compressor {
    // Function pointers remain valid for at least as long as their owning library.
    _library: Library,
    capacity: Capacity,
    compress: Compress,
    decompress: Decompress,
}

impl Compressor {
    pub(super) fn open(path: &Path) -> Result<Self, String> {
        let _guard = NATIVE_CALLS
            .lock()
            .map_err(|_| "The native package encoder lock is poisoned")?;
        // SAFETY: The caller supplies the absolute version-3 DLL path in the selected game
        // installation. The library is retained in Self, and its ABI is checked before encoding.
        let library = unsafe { Library::new(path) }.map_err(|error| {
            format!("Could not load package encoder {}: {error}", path.display())
        })?;
        // SAFETY: Oodle_CheckVersion has the stable 32-bit version / output-pointer C ABI.
        let check_version = unsafe {
            *library
                .get::<CheckVersion>(b"Oodle_CheckVersion\0")
                .map_err(|error| format!("Package encoder has no version check: {error}"))?
        };
        let mut version = 0_u32;
        // SAFETY: The output pointer is valid and writable for one u32. No codec ABI-dependent
        // entry point is invoked until this call verifies the expected legacy header family.
        let accepted = unsafe { check_version(LEGACY_HEADER_VERSION, &mut version) };
        if accepted == 0 || version & 0xFFFF_0000 != LEGACY_VERSION_FAMILY {
            return Err(format!(
                "Package encoder {} reports unsupported Oodle version 0x{version:08X}. Shadowkeep requires the legacy version-3 ABI.",
                path.display()
            ));
        }
        // SAFETY: The checked 2.3 ABI uses a one-argument capacity query, eight-argument compressor,
        // and fourteen-argument decompressor. The owning library outlives the copied pointers.
        let (capacity, compress, decompress) = unsafe {
            (
                *library
                    .get::<Capacity>(b"OodleLZ_GetCompressedBufferSizeNeeded\0")
                    .map_err(|error| format!("Package encoder has no capacity query: {error}"))?,
                *library
                    .get::<Compress>(b"OodleLZ_Compress\0")
                    .map_err(|error| {
                        format!("Package encoder has no compression export: {error}")
                    })?,
                *library
                    .get::<Decompress>(b"OodleLZ_Decompress\0")
                    .map_err(|error| {
                        format!("Package encoder has no verification decoder: {error}")
                    })?,
            )
        };
        Ok(Self {
            _library: library,
            capacity,
            compress,
            decompress,
        })
    }

    pub(super) fn compress(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        if plaintext.is_empty() || plaintext.len() > BLOCK_SIZE {
            return Err(format!(
                "Native package compression requires 1 to {BLOCK_SIZE} bytes"
            ));
        }
        let _guard = NATIVE_CALLS
            .lock()
            .map_err(|_| "The native package encoder lock is poisoned")?;
        // Tiger reads compressed blocks as complete logical spans, including the final block.
        // Raw fallback still stores the original unpadded input in the parent encoder.
        let mut padded = vec![0_u8; BLOCK_SIZE];
        padded[..plaintext.len()].copy_from_slice(plaintext);
        let raw_length = i64::try_from(padded.len())
            .map_err(|_| "The package compression input is too large for Oodle")?;
        // SAFETY: The legacy capacity function takes exactly one positive i64 byte count.
        let required = unsafe { (self.capacity)(raw_length) };
        let capacity = checked_native_length(required, BLOCK_SIZE * 2, "compression capacity")?;
        if capacity < padded.len() {
            return Err("Native package compression returned an undersized capacity".into());
        }
        let mut compressed = Vec::new();
        compressed.try_reserve_exact(capacity).map_err(|error| {
            format!("Could not allocate the package compression buffer: {error}")
        })?;
        compressed.resize(capacity, 0);
        // SAFETY: Both buffers are valid, separate allocations. Output has the full capacity
        // required by this same DLL. Null optional arguments select native defaults without a
        // dictionary or long-range matcher. Legacy 2.3 has no scratch arguments.
        let written = unsafe {
            (self.compress)(
                LZH,
                padded.as_ptr(),
                raw_length,
                compressed.as_mut_ptr(),
                NORMAL,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
            )
        };
        let stored_length = checked_native_length(written, capacity, "compressed output")?;
        compressed.truncate(stored_length);
        let stored_length = i64::try_from(stored_length)
            .map_err(|_| "The compressed package block is too large for verification")?;
        let mut verified = vec![0_u8; BLOCK_SIZE];
        // SAFETY: The decoder reads exactly the initialized compressed output and writes into a
        // separate full-size logical block. Fuzz safety is enabled. No callbacks or shared scratch
        // memory are provided, and phase 3 performs the complete decode in this call.
        let decoded = unsafe {
            (self.decompress)(
                compressed.as_ptr(),
                stored_length,
                verified.as_mut_ptr(),
                raw_length,
                1,
                0,
                0,
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                COMPLETE_DECODE,
            )
        };
        if decoded != raw_length || verified != padded {
            return Err(format!(
                "Native package compression failed exact round-trip verification. Decoder returned {decoded} of {raw_length} expected bytes."
            ));
        }
        Ok(compressed)
    }
}

fn checked_native_length(value: i64, maximum: usize, label: &str) -> Result<usize, String> {
    usize::try_from(value)
        .ok()
        .filter(|&length| length > 0 && length <= maximum)
        .ok_or_else(|| format!("Native package {label} returned invalid size {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_lengths_reject_failure_overflow_and_out_of_bounds_values() {
        for invalid in [i64::MIN, -1, 0, 1025, i64::MAX] {
            assert!(checked_native_length(invalid, 1024, "test").is_err());
        }
        assert_eq!(checked_native_length(1, 1024, "test"), Ok(1));
        assert_eq!(checked_native_length(1024, 1024, "test"), Ok(1024));
    }

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES with the matching Oodle3 game DLL"]
    fn installed_legacy_encoder_round_trips_short_full_and_noisy_blocks() {
        let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("set PARHELION_CLEAN_STOCK_PACKAGES");
        let path = Path::new(&packages)
            .parent()
            .unwrap()
            .join("bin/x64/oo2core_3_win64.dll");
        let compressor = Compressor::open(&path).expect("open the installed legacy codec");
        let mut seed = 0x1234_5678_u32;
        let noisy = (0..BLOCK_SIZE)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect::<Vec<_>>();
        for plaintext in [vec![0xA5], vec![0; 32_000], vec![0xA5; BLOCK_SIZE], noisy] {
            let compressed = compressor.compress(&plaintext).unwrap();
            assert_eq!(compressor.compress(&plaintext).unwrap(), compressed);
            let (expected_bytes, expected_flags) = if compressed.len() < plaintext.len() {
                (compressed.clone(), super::super::COMPRESSED_FLAGS)
            } else {
                (plaintext.clone(), super::super::FULL_RAW_FLAGS)
            };
            let stored =
                super::super::select_verified_storage(&plaintext, Some(compressed)).unwrap();
            assert_eq!(stored.stored, expected_bytes);
            assert_eq!(stored.flags, expected_flags);
            assert_eq!(stored.gcm_tag, [0; 16]);
        }
    }
}
