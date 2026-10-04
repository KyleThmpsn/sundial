use super::*;

pub(super) struct Fixture {
    directory: tempfile::TempDir,
    pub cases: Vec<(String, u32)>,
    pub invalid: Vec<(String, u32)>,
}
impl Fixture {
    pub fn manager(&self) -> PackageManager {
        PackageManager::new(
            self.directory.path(),
            tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
            Some(tiger_pkg::PackagePlatform::Win64),
        )
        .unwrap()
    }
}
#[derive(Default)]
pub(super) struct Package(Vec<(u32, u8, u8, Vec<u8>)>);
impl Package {
    pub(super) fn payload_mut(&mut self, tag: u32) -> &mut Vec<u8> {
        &mut self.0[(tag - 0x8080_2000) as usize].3
    }
    pub(super) fn set_reference(&mut self, tag: u32, reference: u32) {
        self.0[(tag - 0x8080_2000) as usize].0 = reference;
    }
    pub(super) fn add(&mut self, class: u32, mut bytes: Vec<u8>) -> u32 {
        let size = bytes.len() as u64;
        bytes[..8].copy_from_slice(&size.to_le_bytes());
        self.raw(class, 8, 0, bytes)
    }
    pub(super) fn raw(&mut self, reference: u32, kind: u8, subtype: u8, bytes: Vec<u8>) -> u32 {
        let tag = 0x8080_2000 + self.0.len() as u32;
        self.0.push((reference, kind, subtype, bytes));
        tag
    }
    pub(super) fn vertex(&mut self, stride: u16, data: Vec<u8>) -> u32 {
        let size = data.len() as u32;
        let payload = self.raw(0, 0, 0, data);
        let mut header = vec![0; 16];
        put(&mut header, 0, &size.to_le_bytes());
        put(&mut header, 4, &stride.to_le_bytes());
        self.raw(payload, 32, 4, header)
    }
    pub(super) fn indices(&mut self, bad: bool) -> u32 {
        let data = [0u16, 1, if bad { 7 } else { 2 }]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        let payload = self.raw(0, 0, 0, data);
        let mut header = vec![0; 16];
        put(&mut header, 8, &6u64.to_le_bytes());
        self.raw(payload, 32, 6, header)
    }
    pub(super) fn write(&self, directory: &Path) {
        let entries = 0x200usize;
        let blocks = entries + self.0.len() * 16 + 32;
        let start = blocks + self.0.len() * 48;
        let mut bytes = vec![0; start];
        put(&mut bytes, 0, &38u16.to_le_bytes());
        put(&mut bytes, 2, &2u16.to_le_bytes());
        put(&mut bytes, 4, &1u16.to_le_bytes());
        for at in [0xB4, 0xD0, entries - 16] {
            put(&mut bytes, at, &(self.0.len() as u32).to_le_bytes());
        }
        put(&mut bytes, 0x110, &((entries - 96) as u32).to_le_bytes());
        for (i, (reference, kind, subtype, data)) in self.0.iter().enumerate() {
            put(&mut bytes, entries + i * 16, &reference.to_le_bytes());
            put(
                &mut bytes,
                entries + i * 16 + 4,
                &((u32::from(*kind) << 9) | (u32::from(*subtype) << 6)).to_le_bytes(),
            );
            put(
                &mut bytes,
                entries + i * 16 + 8,
                &((i as u64) | ((data.len() as u64) << 28)).to_le_bytes(),
            );
            let offset = bytes.len() as u32;
            put(&mut bytes, blocks + i * 48, &offset.to_le_bytes());
            put(
                &mut bytes,
                blocks + i * 48 + 4,
                &(data.len() as u32).to_le_bytes(),
            );
            bytes.extend_from_slice(data);
        }
        let size = bytes.len() as u32;
        put(&mut bytes, 0x164, &size.to_le_bytes());
        std::fs::write(directory.join("w64_preview_0001_0.pkg"), bytes).unwrap();
    }
}
pub(super) fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    bytes[at..at + value.len()].copy_from_slice(value);
}
pub(super) fn floats(bytes: &mut [u8], at: usize, values: &[f32]) {
    for (i, value) in values.iter().enumerate() {
        put(bytes, at + i * 4, &value.to_le_bytes());
    }
}
pub(super) fn array(
    bytes: &mut Vec<u8>,
    at: usize,
    class: u32,
    rows: &[u8],
    stride: usize,
) -> usize {
    let count = (rows.len() / stride) as u64;
    let header = bytes.len();
    put(bytes, at, &count.to_le_bytes());
    put(bytes, at + 8, &((header - at - 8) as i64).to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&class.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(rows);
    header + 16
}
pub(super) fn layouts(package: &mut Package) {
    let declarations: &[(u8, &[&[[u8; 3]]])] = &[
        (7, &[&[[0, 0, 33], [5, 0, 10], [3, 0, 11], [6, 0, 11]]]),
        (13, &[&[[0, 0, 3], [5, 0, 2], [3, 0, 3]]]),
        (20, &[&[[0, 0, 3], [2, 0, 6], [3, 0, 3], [5, 0, 2]]]),
        (
            19,
            &[
                &[[0, 0, 11], [2, 0, 6]],
                &[[5, 0, 10], [3, 0, 11], [6, 0, 11]],
            ],
        ),
        (60, &[&[[0, 0, 8]], &[[3, 0, 11], [5, 1, 12]]]),
    ];
    let total: usize = declarations.iter().map(|(_, streams)| streams.len()).sum();
    let mut elements = vec![0; 0x18];
    let sets = array(&mut elements, 8, 0x8080_72AF, &vec![0; total * 16], 16);
    let mut mapping = vec![0; 0x18];
    let rows = array(
        &mut mapping,
        8,
        0x8080_72AC,
        &vec![0; declarations.len() * 0x1C],
        0x1C,
    );
    let mut set = 0usize;
    for (i, (id, streams)) in declarations.iter().enumerate() {
        mapping[rows + i * 0x1C] = *id;
        for slot in 0..4 {
            let index = if let Some(stream) = streams.get(slot) {
                let data = stream.iter().flatten().copied().collect::<Vec<_>>();
                array(&mut elements, sets + set * 16, 0x8080_72B2, &data, 3);
                set += 1;
                (set - 1) as u32
            } else {
                u32::MAX
            };
            put(
                &mut mapping,
                rows + i * 0x1C + 8 + slot * 4,
                &index.to_le_bytes(),
            );
        }
    }
    let elements = package.add(0x8080_72AD, elements);
    let mapping = package.add(0x8080_72A9, mapping);
    let mut root = vec![0; 0x30];
    put(&mut root, 0xC, &elements.to_le_bytes());
    put(&mut root, 0x28, &mapping.to_le_bytes());
    package.add(0x8080_72A6, root);
}

/// A regular mesh and a separate cloth owner must both survive entity traversal.
pub(super) fn cloth_entity() -> (Package, u32, [u32; 2]) {
    let mut package = Package::default();
    layouts(&mut package);
    let mut models = Vec::new();
    let mut components = Vec::new();
    for (header_class, data_class, lower) in [
        (0x8080_72B8u32, 0x8080_72BDu32, 0.0f32),
        (0x8080_7273, 0x8080_7286, -2.0),
    ] {
        let data = [
            [0.0, lower, 0.0],
            [1.0, lower, 0.0],
            [0.0, lower + 1.0, 1.0],
        ]
        .into_iter()
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect();
        let stream = package.vertex(12, data);
        let model = entity(&mut package, 0, 0, [stream, u32::MAX], false);
        models.push(model);
        let mut component = vec![0; 0x400];
        put(&mut component, 0x10, &0x30i64.to_le_bytes());
        put(&mut component, 0x18, &0x68i64.to_le_bytes());
        put(&mut component, 0x3c, &header_class.to_le_bytes());
        put(&mut component, 0x7c, &data_class.to_le_bytes());
        put(&mut component, 0x80 + 0x1dc, &model.to_le_bytes());
        let tag = package.add(RESOURCE, component);
        components.extend_from_slice(&tag.to_le_bytes());
        components.extend_from_slice(&[0; 8]);
    }
    let mut root = vec![0; 0x28];
    array(&mut root, 0x10, 0x8080_9C04, &components, 12);
    let root = package.add(ENTITY, root);
    (package, root, models.try_into().unwrap())
}
fn entity(package: &mut Package, layout: u16, stage: usize, vertices: [u32; 2], bad: bool) -> u32 {
    let indices = package.indices(bad);
    let mut bytes = vec![0; 0x80];
    floats(&mut bytes, 0x50, &[2.0, 3.0, 4.0]);
    floats(&mut bytes, 0x60, &[10.0, 20.0, 30.0]);
    floats(&mut bytes, 0x6C, &[2.0]);
    floats(&mut bytes, 0x70, &[1.0, 1.0, 0.0, 0.0]);
    if layout == 13 {
        floats(&mut bytes, 0x70, &[2.0, 1.0, 0.25, 0.5]);
    }
    let mesh = array(&mut bytes, 0x10, 0x8080_7378, &[0; 0x88], 0x88);
    for (slot, tag) in vertices.into_iter().enumerate() {
        put(&mut bytes, mesh + slot * 4, &tag.to_le_bytes());
    }
    put(&mut bytes, mesh + 0x10, &indices.to_le_bytes());
    for s in 0..24 {
        put(
            &mut bytes,
            mesh + 0x28 + s * 2,
            &(if s <= stage { 0i16 } else { 1 }).to_le_bytes(),
        );
    }
    // Inactive stages deliberately have different declarations.
    put(&mut bytes, mesh + 0x58 + stage * 2, &layout.to_le_bytes());
    let mut part = [0; 0x20];
    put(&mut part, 4, &(-1i16).to_le_bytes());
    put(&mut part, 6, &3u16.to_le_bytes());
    put(&mut part, 12, &3u32.to_le_bytes());
    part[0x1A] = 255;
    array(&mut bytes, mesh + 0x18, 0x8080_737E, &part, 0x20);
    package.add(MODEL, bytes)
}
pub(super) fn build() -> Fixture {
    let mut package = Package::default();
    layouts(&mut package);
    let float = [
        [0.0f32, 0.0, 0.0, 0.25, 0.75, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 1.0],
    ]
    .into_iter()
    .flatten()
    .flat_map(f32::to_le_bytes)
    .collect();
    let float = package.vertex(32, float);
    let packed = [
        [0i16, 0, 0, 0, 0, 32767, 0, 0, 32767, 0, 32767, 0, 0, 0],
        [32767, 0, 0, 0, 0, 0, 0, 0, 32767, 0, 32767, 0, 0, 0],
        [0, 32767, 32767, 0, 32767, 0, 0, 0, 32767, 0, 32767, 0, 0, 0],
    ];
    let packed = package.vertex(
        28,
        packed
            .into_iter()
            .flatten()
            .flat_map(i16::to_le_bytes)
            .collect(),
    );
    let mut skin = Vec::new();
    let mut second = Vec::new();
    for p in [[0i16, 0, 0, 0], [32767, 0, 0, 0], [0, 32767, 32767, 0]] {
        skin.extend(p.into_iter().flat_map(i16::to_le_bytes));
        skin.extend([1, 2, 64, 191]);
        second.extend(
            [0i16, 0, 0, 0, 32767, 0, 32767, 0, 0, 0]
                .into_iter()
                .flat_map(i16::to_le_bytes),
        );
    }
    let skin = package.vertex(12, skin);
    let second = package.vertex(20, second);
    let mut cases = vec![
        (
            "float-stage".into(),
            entity(&mut package, 13, 0, [float, 0], false),
        ),
        (
            "fallback-stage".into(),
            entity(&mut package, 13, 9, [float, 0], false),
        ),
        (
            "packed-single".into(),
            entity(&mut package, 7, 0, [packed, 0], false),
        ),
        (
            "two-influence".into(),
            entity(&mut package, 19, 0, [skin, second], false),
        ),
    ];
    let invalid = vec![
        (
            "invalid-index".into(),
            entity(&mut package, 13, 0, [float, 0], true),
        ),
        (
            "unknown-layout".into(),
            entity(&mut package, 250, 0, [float, 0], false),
        ),
    ];
    let indices = package.indices(false);
    let mut opaque = vec![0; 0x38];
    array(&mut opaque, 8, 0x8080_719B, &[], 8);
    array(&mut opaque, 0x18, 0x8080_719A, &[], 12);
    array(&mut opaque, 0x28, 0x8080_7199, &[], 16);
    let opaque = package.add(0x8080_7194, opaque);
    let mut root = vec![0; 0x80];
    put(&mut root, 8, &opaque.to_le_bytes());
    floats(&mut root, 0x60, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0]);
    array(&mut root, 0x10, 0x8080_0014, &[], 4);
    let mut draw = [0u8; 0x20];
    draw[2] = 7;
    draw[6] = 3;
    put(&mut draw, 8, &indices.to_le_bytes());
    put(&mut draw, 12, &packed.to_le_bytes());
    put(&mut draw, 0x18, &3u32.to_le_bytes());
    array(&mut root, 0x20, 0x8080_7193, &draw, 0x20);
    cases.push(("static-single".into(), package.add(0x8080_71A7, root)));
    let pos = package.vertex(
        8,
        [[0i16, 0, 0, 0], [64, 0, 0, 0], [0, 64, 8192, 0]]
            .into_iter()
            .flatten()
            .flat_map(i16::to_le_bytes)
            .collect(),
    );
    let secondary = package.vertex(
        12,
        [0i16, 0, 32767, 0, 0x3400, 0x3A00]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .cycle()
            .take(36)
            .collect(),
    );
    let mut terrain = vec![0; 0xB0];
    floats(&mut terrain, 0x30, &[1024.0, 2048.0, 8192.0, 1.0]);
    for (i, tag) in [pos, secondary, indices].into_iter().enumerate() {
        put(&mut terrain, 0x68 + i * 4, &tag.to_le_bytes());
    }
    let mut group = [0; 0x60];
    floats(&mut group, 0x20, &[1.0, 1.0, 0.0, 0.0]);
    array(&mut terrain, 0x58, 0x8080_7154, &group, 0x60);
    let mut part = [0; 12];
    put(&mut part, 8, &3u16.to_le_bytes());
    array(&mut terrain, 0x80, 0x8080_7152, &part, 12);
    cases.push(("terrain".into(), package.add(0x8080_714F, terrain)));
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    Fixture {
        directory,
        cases,
        invalid,
    }
}
