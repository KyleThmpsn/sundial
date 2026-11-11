//! Follow a private status display to its original-size source glyph and localized name.
use super::*;

pub(super) fn readback(
    source: &Source,
    staged: &PackageManager,
    stock: &PackageManager,
    source_name: u32,
    name: u32,
) -> serde_json::Value {
    if source_name == 0x811C9DC5 {
        assert_eq!(name, source_name);
        return json!({"empty":true});
    }
    let table = Payload(staged.read_tag(TagHash(0x80D5FFD3)).unwrap());
    assert_ne!(
        name, source_name,
        "source status needs its own display identity"
    );
    let original = Payload(stock.read_tag(TagHash(0x80D5FFD3)).unwrap());
    let written = table
        .array(8, 112, Some(0x80804A59))
        .unwrap()
        .into_iter()
        .map(|at| (table.u32(at).unwrap(), table.bytes::<112>(at).unwrap()))
        .collect::<BTreeMap<_, _>>();
    for at in original.array(8, 112, Some(0x80804A59)).unwrap() {
        assert_eq!(
            written[&original.u32(at).unwrap()],
            original.bytes::<112>(at).unwrap(),
            "stock HUD row changed"
        );
    }
    let rows = table.array(8, 112, Some(0x80804A59)).unwrap();
    let rows = rows
        .into_iter()
        .filter(|&at| table.u32(at).unwrap() == name)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1, "native status entry");
    let layer_tag = TagHash(table.u32(rows[0] + 4).unwrap());
    let layer = Payload(staged.read_tag(layer_tag).unwrap());
    assert!(
        stock.get_entry(layer_tag).is_none(),
        "HUD layer must be private"
    );
    let (size, source_pixels) = source_glyph(source, source_name);
    let textures = check_textures(&layer, (staged, stock), size, &source_pixels);
    let texts = read_texts(staged, name);
    json!({"name":name,"layer":layer_tag.0,"textures":textures,"text":texts,
        "source_glyph_sha256":sha(&source_pixels),"gameplay_verified":false})
}

fn source_glyph(source: &Source, source_name: u32) -> ((u32, u32), Vec<u8>) {
    let mut source_layers = BTreeSet::new();
    for tag in source.classes(0x808022D2) {
        let p = source.read(tag).unwrap();
        for at in p.array(8, 112, Some(0x8080295C)).unwrap() {
            if p.u32(at).unwrap() == source_name {
                source_layers.insert(p.u32(at + 4).unwrap());
            }
        }
    }
    assert_eq!(source_layers.len(), 1);
    let source_layer = source.read(*source_layers.first().unwrap()).unwrap();
    let image = source_layer.pointer(16).unwrap();
    let lanes = source_layer.array(image, 16, Some(0x80802478)).unwrap();
    assert_eq!(lanes.len(), 1);
    let frames = source_layer.array(lanes[0], 4, Some(0x80802B99)).unwrap();
    assert_eq!(frames.len(), 1);
    let texture = source_layer.u32(frames[0]).unwrap();
    let header = source.read(texture).unwrap();
    let descriptor = parhelion_import::d1_roi::texture::Descriptor {
        format: header.u32(0).unwrap(),
        tile_mode: header.u32(4).unwrap(),
        width: header.u16(0x30).unwrap().into(),
        height: header.u16(0x32).unwrap().into(),
        depth: header.u16(0x34).unwrap().into(),
        layers: header.u16(0x36).unwrap().into(),
        mips: header.u8(0x39).unwrap().into(),
    };
    assert_eq!(
        (
            descriptor.format,
            descriptor.depth,
            descriptor.layers,
            descriptor.mips
        ),
        (28, 1, 1, 1)
    );
    let source_bytes = source.read(source.reference(texture).unwrap()).unwrap();
    let source_pixels =
        parhelion_import::d1_roi::texture::decode(&descriptor, &source_bytes.0).unwrap();
    ((descriptor.width, descriptor.height), source_pixels)
}

fn check_textures(
    layer: &Payload,
    (staged, stock): (&PackageManager, &PackageManager),
    source_size: (u32, u32),
    source_pixels: &[u8],
) -> Vec<serde_json::Value> {
    let mut textures = Vec::new();
    let mut matching = 0;
    for lane in layer.array(0x20, 16, Some(0x80804A6A)).unwrap() {
        for at in layer.array(lane, 4, Some(0x80804A6E)).unwrap() {
            let tag = TagHash(layer.u32(at).unwrap());
            let header = Payload(staged.read_tag(tag).unwrap());
            assert_eq!(header.u32(4).unwrap(), 28);
            let size = (
                u32::from(header.u16(14).unwrap()),
                u32::from(header.u16(16).unwrap()),
            );
            let data = TagHash(staged.get_entry(tag).unwrap().reference);
            let pixels = staged.read_tag(data).unwrap();
            assert!(
                stock.get_entry(tag).is_none() && stock.get_entry(data).is_none(),
                "HUD texture must be private"
            );
            if size == source_size {
                assert_eq!(
                    pixels, source_pixels,
                    "retained native glyph differs from D1"
                );
                matching += 1;
            }
            textures.push(json!({"tag":tag.0,"data":data.0,"size":size,"sha256":sha(&pixels)}));
        }
    }
    assert_eq!(matching, 1, "original-size source glyph");
    textures
}

fn read_texts(staged: &PackageManager, name: u32) -> Vec<serde_json::Value> {
    let names = Payload(staged.read_tag(TagHash(0x815B961E)).unwrap());
    let hashes = names.array(8, 4, Some(0x80800070)).unwrap();
    let index = hashes
        .iter()
        .position(|&at| names.u32(at).unwrap() == name)
        .unwrap();
    let mut texts = Vec::new();
    for offset in (LOCALIZATION_DATA_TAG_START..LOCALIZATION_DATA_TAG_END).step_by(4) {
        let tag = TagHash(names.u32(offset).unwrap());
        let data = staged.read_tag(tag).unwrap();
        let text = decode_localized_value_at(&data, index).unwrap();
        assert_eq!(text, "Song of Flame");
        texts.push(json!({"tag":tag.0,"text":text,"sha256":sha(&data)}));
    }
    texts
}
