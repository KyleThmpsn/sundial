use super::*;

const CHILD_FLAG: &str = "PARHELION_VIEW_LIFETIME_CHILD";
const CHILD_ROOT: &str = "PARHELION_VIEW_LIFETIME_ROOT";
const TEST_NAME: &str =
    "workflow::package_views::tests::native::source_decoder_is_not_loaded_from_temporary_view";
const SOURCE_NAME: &str = "w64_lifetime_source_058c_0.pkg";
const IGNORED_NAME: &str = "w64_lifetime_excluded_058d_0.pkg";

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and Oodle3, runs an isolated child and writes temporary files only"]
fn source_decoder_is_not_loaded_from_temporary_view() {
    if std::env::var_os(CHILD_FLAG).is_some() {
        check_fresh_decoder_lifetime();
        return;
    }
    // Isolate tiger's lazy static from other tests and keep executable/CWD DLL lookup empty.
    // The parent owns the whole fixture until the child exits and releases its global DLL.
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("view-lifetime-test.exe");
    fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let output = std::process::Command::new(&executable)
        .args([
            TEST_NAME,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_FLAG, "1")
        .env(CHILD_ROOT, root.path())
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fresh decoder lifetime check failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn check_fresh_decoder_lifetime() {
    let root = PathBuf::from(std::env::var_os(CHILD_ROOT).unwrap());
    let installed = PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").expect("configure Shadowkeep packages"),
    );
    let packages = root.join("source/packages");
    let views = root.join("views");
    fs::create_dir_all(&packages).unwrap();
    fs::create_dir(&views).unwrap();
    initialize_source_decoder(&packages).unwrap(); // A codec-free first call must not consume initialization.
    let runtime = installed
        .parent()
        .unwrap()
        .join("bin/x64/oo2core_3_win64.dll");
    let bin = root.join("source/bin/x64");
    fs::create_dir_all(&bin).unwrap();
    let stable_runtime = bin.join("oo2core_3_win64.dll");
    fs::copy(runtime, &stable_runtime).unwrap();
    let payload = vec![0x5A; 31_013];
    let source_bytes = compressed_source_package(&packages, &payload);
    fs::write(packages.join(SOURCE_NAME), &source_bytes).unwrap();
    fs::write(
        packages.join(IGNORED_NAME),
        b"excluded malformed authored data",
    )
    .unwrap();

    let view =
        FilteredPackageView::create_in_root(&packages, &[IGNORED_NAME.to_owned()], &views).unwrap();
    let view_root = view.path().parent().unwrap().to_path_buf();
    assert!(
        view_root.join("bin/x64/oo2core_3_win64.dll").is_file(),
        "compression still needs the per-view runtime path"
    );
    assert!(!view.path().join(IGNORED_NAME).exists());
    let manager = sundial::package_authoring::open_shadowkeep_package_manager(view.path()).unwrap();
    assert_eq!(
        manager
            .read_tag(tiger_pkg::TagHash::new(0x058C, 0))
            .unwrap(),
        payload
    );
    assert_eq!(
        loaded_oodle_path().canonicalize().unwrap(),
        stable_runtime.canonicalize().unwrap()
    );
    drop(manager);
    view.close()
        .expect("source-anchored decoder must not pin the temporary view DLL");
    assert!(!view_root.exists());
    assert_eq!(fs::read(packages.join(SOURCE_NAME)).unwrap(), source_bytes);
    assert_eq!(
        fs::read(packages.join(IGNORED_NAME)).unwrap(),
        b"excluded malformed authored data"
    );
}

fn compressed_source_package(packages: &Path, payload: &[u8]) -> Vec<u8> {
    use crate::format::{
        PackageLayout, append_aligned, append_opaque_trailer,
        build_test_package_with_physical_payload,
    };
    use sha1::{Digest, Sha1};

    let source = build_test_package_with_physical_payload(0x058C, 0, payload).unwrap();
    let layout = PackageLayout::parse(&source).unwrap();
    let encoded = crate::block_codec::PackageBlockEncoder::open_for_packages(packages)
        .unwrap()
        .encode(0x058C, payload)
        .unwrap();
    assert_eq!(encoded.flags, 1);
    let mut bytes = layout
        .sparse_overlay_metadata_prefix(&source)
        .unwrap()
        .to_vec();
    let offset = append_aligned(&mut bytes, &encoded.stored);
    let row = layout.block_table_offset;
    bytes[row..row + 4].copy_from_slice(&(offset as u32).to_le_bytes());
    bytes[row + 4..row + 8].copy_from_slice(&(encoded.stored.len() as u32).to_le_bytes());
    bytes[row + 10..row + 12].copy_from_slice(&encoded.flags.to_le_bytes());
    bytes[row + 12..row + 32].copy_from_slice(&Sha1::digest(&encoded.stored));
    layout.update_package_tables_hash(&mut bytes).unwrap();
    append_opaque_trailer(&mut bytes, layout.opaque_trailer(&source).unwrap()).unwrap();
    layout.set_file_size(&mut bytes).unwrap();
    PackageLayout::parse(&bytes).unwrap();
    bytes
}

fn loaded_oodle_path() -> PathBuf {
    use std::{ffi::c_void, os::windows::ffi::OsStringExt};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn GetModuleFileNameW(module: *mut c_void, path: *mut u16, capacity: u32) -> u32;
    }
    let name = "oo2core_3_win64.dll\0".encode_utf16().collect::<Vec<_>>();
    let mut path = vec![0u16; 32_768];
    // SAFETY: The module-name buffer is NUL-terminated, and the output buffer is writable for
    // the supplied number of UTF-16 units. The module remains owned by tiger's global decoder.
    let length = unsafe {
        let module = GetModuleHandleW(name.as_ptr());
        assert!(!module.is_null());
        GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32)
    } as usize;
    assert!(length > 0 && length < path.len());
    PathBuf::from(std::ffi::OsString::from_wide(&path[..length]))
}
