//! A non-executable PE fixture with version/default resources and explicit capability strings.
use super::*;

pub(crate) fn install() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("bin/x64")).unwrap();
    fs::create_dir(directory.path().join("packages")).unwrap();
    fs::write(directory.path().join("destiny2.exe"), []).unwrap();
    fs::write(directory.path().join("bin/x64/oo2core_3_win64.dll"), []).unwrap();
    directory
}

pub(crate) fn module(brand: &str, dawn: bool, missing: Option<usize>) -> Vec<u8> {
    let defaults = if dawn {
        br#"{"version":6,"experiments":{"omega":{"coo_executor":false}}}"#.as_slice()
    } else {
        br#"{"version":6}"#.as_slice()
    };
    build(brand, dawn, missing, defaults)
}

pub(crate) fn module_with_schema(brand: &str, schema: u64) -> Vec<u8> {
    build(
        brand,
        false,
        None,
        format!("{{\"version\":{schema}}}").as_bytes(),
    )
}

fn build(brand: &str, dawn: bool, missing: Option<usize>, defaults: &[u8]) -> Vec<u8> {
    let version = version_resource(brand, brand.trim().eq_ignore_ascii_case("Dawn"));
    let mut resources = vec![0; 160];
    directory(&mut resources, 0, &[(10, 32, true), (16, 96, true)]);
    directory(&mut resources, 32, &[(101, 56, true)]);
    directory(&mut resources, 56, &[(1033, 80, false)]);
    directory(&mut resources, 96, &[(1, 120, true)]);
    directory(&mut resources, 120, &[(1033, 144, false)]);
    for (entry, payload) in [(80, defaults), (144, version.as_slice())] {
        let offset = resources.len();
        u32_at(&mut resources, entry, 0x1000 + offset as u32);
        u32_at(&mut resources, entry + 4, payload.len() as u32);
        resources.extend_from_slice(payload);
        align(&mut resources, 4);
    }
    if dawn {
        resources.extend_from_slice(b"ev=coo_script mission=omega result=loaded format=lua\0ev=coo_executor mission=omega mode=composition\0Sunrise/scripts/omega.lua\0coo_executor\0");
    }
    for (index, (marker, _)) in PACKAGE_AUTHORING_RUNTIME_MARKERS.iter().enumerate() {
        if Some(index) != missing {
            resources.extend_from_slice(marker);
            resources.push(0);
        }
    }
    let resource_size = resources.len() as u32;
    align(&mut resources, 512);
    let mut image = vec![0; 512];
    u16_at(&mut image, 0, 0x5a4d);
    u32_at(&mut image, 0x3c, 0x80);
    u32_at(&mut image, 0x80, 0x4550);
    u16_at(&mut image, 0x84, 0x8664);
    u16_at(&mut image, 0x86, 1);
    u16_at(&mut image, 0x94, 240);
    u16_at(&mut image, 0x96, 0x2022);
    let optional = 0x98;
    u16_at(&mut image, optional, 0x20b);
    u32_at(&mut image, optional + 32, 4096);
    u32_at(&mut image, optional + 36, 512);
    u32_at(
        &mut image,
        optional + 56,
        (0x1000 + resource_size).next_multiple_of(4096),
    );
    u32_at(&mut image, optional + 60, 512);
    u32_at(&mut image, optional + 108, 16);
    u32_at(&mut image, optional + 128, 0x1000);
    u32_at(&mut image, optional + 132, resource_size);
    image[0x188..0x190].copy_from_slice(b".rsrc\0\0\0");
    u32_at(&mut image, 0x190, resource_size);
    u32_at(&mut image, 0x194, 0x1000);
    u32_at(&mut image, 0x198, resources.len() as u32);
    u32_at(&mut image, 0x19c, 512);
    u32_at(&mut image, 0x1ac, 0x4000_0040);
    image.extend_from_slice(&resources);
    image
}

fn version_resource(brand: &str, dawn: bool) -> Vec<u8> {
    let fields = ["ProductName", "FileDescription"].map(|key| {
        let text = if key == "FileDescription" && brand == "Dawn" {
            "Dawn v.01"
        } else {
            brand
        };
        let text: Vec<_> = text
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        block(key, &text, (text.len() / 2) as u16, 1, &[])
    });
    let table = block("040904B0", &[], 0, 1, &fields);
    let strings = block("StringFileInfo", &[], 0, 1, &[table]);
    let (ms, ls) = if dawn { (1, 0) } else { (3, 2 << 16) };
    let fixed: Vec<_> = [
        0xfeef04bd_u32,
        0x10000,
        ms,
        ls,
        ms,
        ls,
        0x3f,
        0,
        0x40004,
        2,
        0,
        0,
        0,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    block("VS_VERSION_INFO", &fixed, 52, 0, &[strings])
}

fn block(key: &str, value: &[u8], value_length: u16, kind: u16, children: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0; 6];
    bytes.extend(key.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
    align(&mut bytes, 4);
    bytes.extend_from_slice(value);
    align(&mut bytes, 4);
    for child in children {
        bytes.extend_from_slice(child);
        align(&mut bytes, 4);
    }
    let length = bytes.len() as u16;
    u16_at(&mut bytes, 0, length);
    u16_at(&mut bytes, 2, value_length);
    u16_at(&mut bytes, 4, kind);
    bytes
}

fn directory(bytes: &mut [u8], at: usize, entries: &[(u32, u32, bool)]) {
    u16_at(bytes, at + 14, entries.len() as u16);
    for (index, (id, offset, child)) in entries.iter().enumerate() {
        u32_at(bytes, at + 16 + index * 8, *id);
        u32_at(
            bytes,
            at + 20 + index * 8,
            *offset | if *child { 0x8000_0000 } else { 0 },
        );
    }
}

fn align(bytes: &mut Vec<u8>, alignment: usize) {
    bytes.resize(bytes.len().next_multiple_of(alignment), 0);
}
fn u16_at(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn u32_at(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
