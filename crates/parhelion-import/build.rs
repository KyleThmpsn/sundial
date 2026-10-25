//! Build the pinned public AMD library and the small, private C interface to Rust.
use sha2::{Digest, Sha256};
use std::{env, fs, io::Cursor, path::PathBuf};

fn main() {
    let owner = PathBuf::from("src/d1_roi/texture/amd");
    let archive_path = owner.join("addrlib.zip");
    println!("cargo:rerun-if-changed={}", archive_path.display());
    println!(
        "cargo:rerun-if-changed={}",
        owner.join("bridge.cpp").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        owner.join("bridge.h").display()
    );
    let bytes = fs::read(&archive_path).expect("Read bundled AMD source");
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        digest, "3e593fcb5fba136aee1dfbecee37863f1c29cf4e357d965d22b285dbed2536ef",
        "AMD source archive differs"
    );
    let root =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory")).join("amd-addrlib");
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("Open AMD source archive");
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("Read AMD source member");
        let name = entry
            .enclosed_name()
            .expect("AMD source path stays inside output");
        assert!(!entry.is_dir(), "AMD source archive contains only files");
        let path = root.join(name);
        fs::create_dir_all(path.parent().expect("AMD member parent"))
            .expect("Create AMD source folder");
        let mut file = fs::File::create(path).expect("Create AMD source file");
        std::io::copy(&mut entry, &mut file).expect("Extract AMD source file");
    }
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++14")
        .warnings(false)
        .define("ADDR_SI_BUILD", "1")
        .define("NDEBUG", None)
        // The public headers default to GNU calling-convention attributes. This private,
        // static library and its callbacks all use the compiler's ordinary C convention.
        .define("ADDR_CDECL", "")
        .define("ADDR_STDCALL", "")
        .define("ADDR_FASTCALL", "")
        .define(
            if env::var("CARGO_CFG_TARGET_ENDIAN").as_deref() == Ok("little") {
                "LITTLEENDIAN_CPU"
            } else {
                "BIGENDIAN_CPU"
            },
            None,
        );
    for directory in ["inc", "src", "src/core", "src/chip/r800"] {
        build.include(root.join(directory));
    }
    for file in [
        "src/addrinterface.cpp",
        "src/core/addrlib.cpp",
        "src/core/addrlib1.cpp",
        "src/core/addrlib2.cpp",
        "src/core/addrobject.cpp",
        "src/core/addrelemlib.cpp",
        "src/core/coord.cpp",
        "src/r800/egbaddrlib.cpp",
        "src/r800/siaddrlib.cpp",
    ] {
        build.file(root.join(file));
    }
    build
        .file(owner.join("bridge.cpp"))
        .compile("parhelion_addr");
}
