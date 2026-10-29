//! Read the physical bytes Wwise streams, independently of package decompression.
use super::*;
use std::io::{Read, Seek, SeekFrom};

fn read(file: &mut fs::File, offset: u64, size: usize) -> CheckResult<Payload> {
    let mut bytes = vec![0; size];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut bytes)?;
    Ok(Payload(bytes))
}

pub(super) fn check(reader: &Reader, tag: u32, expected: &Payload) -> CheckResult<Value> {
    let hash = tiger_pkg::TagHash(tag);
    let package = reader
        .manager
        .package_paths
        .get(&hash.pkg_id())
        .ok_or("Streamed medium package")?;
    let mut file = fs::File::open(&package.path)?;
    let header = read(&mut file, 0, 0x168)?;
    assert_eq!(header.u16(0)?, 38);
    assert_eq!(header.u16(4)?, hash.pkg_id());
    let entries = u64::from(header.u32(0x110)?) + 96;
    assert!(u32::from(hash.entry_index()) < header.u32(0xB4)?);
    let entry = read(&mut file, entries + u64::from(hash.entry_index()) * 16, 16)?;
    assert_eq!(entry.u32(0)?, tag);
    let kind = entry.u32(4)?;
    assert_eq!(((kind >> 9) & 0x7F, (kind >> 6) & 7), (26, 6));
    let layout = entry.u64(8)?;
    let first = (layout & 0x3FFF) as usize;
    let within = (((layout >> 14) & 0x3FFF) << 4) as usize;
    let size = usize::try_from((layout >> 28) as u32)?;
    assert_eq!(size, expected.0.len());
    assert_eq!(within % 256, 0);
    assert!(size > 0);
    let allocated = read(&mut file, entries - 16, 4)?.u32(0)?;
    let blocks = entries + u64::from(allocated) * 16 + 32;
    let count = (within + size).div_ceil(0x40000);
    assert!(first + count <= header.u32(0xD0)? as usize);
    let mut consumed = 0;
    let mut next = None;
    let mut records = Vec::new();
    for index in first..first + count {
        let block = read(&mut file, blocks + index as u64 * 48, 48)?;
        assert_eq!(block.u16(8)?, header.u16(0x20)?);
        assert_eq!(
            block.u16(10)? & 15,
            0,
            "Wwise stream touches an encoded block"
        );
        let start = if index == first { within } else { 0 };
        let take = (size - consumed).min(0x40000 - start);
        assert!(start + take <= block.u32(4)? as usize);
        let offset = u64::from(block.u32(0)?) + start as u64;
        if let Some(previous) = next {
            assert_eq!(offset, previous, "Wwise media blocks are not contiguous");
        } else {
            assert_eq!(offset % 256, 0);
        }
        let bytes = read(&mut file, offset, take)?;
        assert_eq!(&bytes.0, &expected.0[consumed..consumed + take]);
        records.push(json!({"index":index,"offset":offset,"bytes":take,
            "patch":block.u16(8)?,"flags":block.u16(10)?,
            "sha256":hex::encode(Sha256::digest(&bytes.0))}));
        consumed += take;
        next = Some(offset + take as u64);
    }
    assert_eq!(consumed, size);
    Ok(json!({"package":package.filename,"tag":tag,"bytes":size,
        "entry_sha256":hex::encode(Sha256::digest(&entry.0)),"blocks":records}))
}
