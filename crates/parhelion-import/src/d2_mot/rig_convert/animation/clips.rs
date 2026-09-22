//! Checked clip-container lowering. Encoded track data stays byte-for-byte intact.
//! Unsupported curves, event records and quantization transforms fail explicitly.
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

type ArrayField = (usize, usize, u32, u32);
mod events;
mod vectors;

fn word(data: &mut Payload, at: usize, value: u32) {
    data.0[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn array(
    source: &Payload,
    native: &mut Payload,
    at: usize,
    stride: usize,
    source_class: u32,
    native_class: u32,
) -> Result<()> {
    source.array_range(at, stride, Some(source_class))?;
    if source.u64(at)? == 0 {
        ensure!(source.u64(at + 8)? == 0, "empty clip array has a pointer");
        return Ok(());
    }
    let header = source.pointer(at + 8)?;
    ensure!(
        header >= 4 && source.u32(header - 4)? == 0x80809FB8,
        "clip array allocation marker differs"
    );
    word(native, header - 4, 0x80809FBD);
    word(native, header + 8, native_class);
    Ok(())
}

fn stream(source: &Payload, native: &mut Payload, at: usize) -> Result<Value> {
    let start = source.pointer(at)?;
    ensure!(start >= 4, "clip stream has no type header");
    let class = source.u32(start - 4)?;
    if class == 0x80808B2A {
        array(source, native, start, 64, 0x80808B2D, 0x80808F5C)?;
        word(native, start - 4, 0x80808F59);
        return Ok(json!({"field":at,"source_class":"80808B2A",
            "native_class":"80808F59","encoded_tracks_preserved":true}));
    }
    let curve = match class {
        0x80808B30 => Some((0x80808F5F, 4, 0x80808B3A, 0x80808F69)),
        0x80808B32 => Some((0x80808F61, 6, 0x80808B3B, 0x80808F6A)),
        0x80808B34 => Some((0x80808F63, 4, 0x80808B3D, 0x80808F6C)),
        _ => None,
    };
    if let Some((target, stride, from, to)) = curve {
        array(source, native, start, 8, 0x80808B3F, 0x80808F6E)?;
        array(source, native, start + 16, stride, from, to)?;
        word(native, start - 4, target);
        return Ok(json!({"field":at,"source_class":format!("{class:08X}"),
            "native_class":format!("{target:08X}"),"encoded_tracks_preserved":true}));
    }
    if class == 0x80808B48 {
        return position(source, native, at, start);
    }
    let (target, encoding, fields): (u32, u16, &[ArrayField]) = match class {
        0x80808B40 => (0x80808F6F, 3, &[(0x38, 2, 0x8080000A, 0x8080000A)]),
        0x80808B42 => (
            0x80808F71,
            2,
            &[
                (0x18, 2, 0x8080000A, 0x8080000A),
                (0x28, 4, 0x8080000F, 0x8080000F),
                (0x38, 4, 0x8080000F, 0x8080000F),
            ],
        ),
        0x80808B43 => (
            0x80808F72,
            1,
            &[
                (0x10, 2, 0x80800006, 0x80800006),
                (0x20, 2, 0x80800006, 0x80800006),
                (0x30, 1, 0x80800009, 0x80800009),
                (0x40, 1, 0x80800009, 0x80800009),
                (0x50, 4, 0x8080000F, 0x8080000F),
                (0x60, 4, 0x8080000F, 0x8080000F),
                (0x70, 2, 0x80800006, 0x80800006),
            ],
        ),
        0x80808B46 => (0x80808F75, 3, &[(0x30, 2, 0x8080000A, 0x8080000A)]),
        0x80808B4C => (
            0x80808F7C,
            2,
            &[
                (0x10, 2, 0x80808B52, 0x80808F82),
                (0x20, 2, 0x8080000A, 0x8080000A),
                (0x30, 4, 0x8080000F, 0x8080000F),
                (0x40, 4, 0x8080000F, 0x8080000F),
            ],
        ),
        _ => anyhow::bail!("clip stream {class:08X} at {at:X} requires a codec converter"),
    };
    ensure!(
        source.u16(start)? == encoding,
        "clip encoding discriminator differs"
    );
    for &(offset, stride, from, to) in fields {
        array(source, native, start + offset, stride, from, to)?;
    }
    word(native, start - 4, target);
    Ok(json!({"field":at,"source_class":format!("{class:08X}"),
        "native_class":format!("{target:08X}"),"encoded_tracks_preserved":true}))
}

fn position(source: &Payload, native: &mut Payload, at: usize, start: usize) -> Result<Value> {
    ensure!(
        source.u16(start)? == 1,
        "position encoding discriminator differs"
    );
    // The modern variable-position layout adds per-track scale and bias arrays.
    // Its predecessor has the same encoded streams but no transform arrays.
    // Identity transforms can be removed without decoding or losing precision.
    let scales = source.array(start + 0x50, 4, Some(0x8080000F))?;
    let biases = source.array(start + 0x60, 4, Some(0x8080000F))?;
    ensure!(
        scales.len() == biases.len() && scales.len() == usize::from(source.u16(start + 2)?),
        "position quantization transform count differs"
    );
    let mut identity = true;
    for (&scale, &bias) in scales.iter().zip(&biases) {
        identity &= source.f32(scale)? == 1.0 && source.f32(bias)? == 0.0;
    }
    if !identity {
        return vectors::convert(source, native, at, start);
    }
    for (offset, stride, class) in [
        (0x10, 2, 0x80800006),
        (0x20, 2, 0x80800006),
        (0x30, 1, 0x80800009),
        (0x40, 1, 0x80800009),
        (0x50, 4, 0x8080000F),
        (0x60, 4, 0x8080000F),
        (0x70, 2, 0x80800006),
    ] {
        array(source, native, start + offset, stride, class, class)?;
    }
    let descriptor = start + 0x50;
    let count = source.u64(start + 0x70)?;
    native.0[descriptor..descriptor + 8].copy_from_slice(&count.to_le_bytes());
    let delta = if count == 0 {
        0
    } else {
        source.pointer(start + 0x78)? as i64 - (descriptor + 8) as i64
    };
    native.0[descriptor + 8..descriptor + 16].copy_from_slice(&delta.to_le_bytes());
    native.0[start + 0x60..start + 0x80].fill(0);
    word(native, start - 4, 0x80808F78);
    Ok(
        json!({"field":at,"source_class":"80808B48","native_class":"80808F78",
        "encoded_tracks_preserved":true,"identity_quantization_removed":true}),
    )
}

/// Convert only independently validated codec layouts. This payload is still
/// unlinked until its clip bank, controller, skeleton and events are resolved.
pub fn convert(bytes: &[u8]) -> Result<(Payload, Value)> {
    lower(bytes, None)
}

/// Convert a clip whose events are carried on the name-matched native
/// counterpart's event block. See `events` for the shape checks that gate it.
pub fn convert_with_events(bytes: &[u8], counterpart: &[u8]) -> Result<(Payload, Value)> {
    lower(bytes, Some(&Payload(counterpart.to_vec())))
}

fn lower(bytes: &[u8], counterpart: Option<&Payload>) -> Result<(Payload, Value)> {
    let source = Payload(bytes.to_vec());
    ensure!(
        bytes.len() >= 0x190 && source.u64(0)? == bytes.len() as u64,
        "clip payload size differs"
    );
    ensure!(
        source.u64(8)? == 0 && source.u64(0x68)? == 0,
        "clip has an unsupported stream extension"
    );
    if counterpart.is_none() {
        ensure!(
            source.u64(0x160)? == 0 && source.u64(0x168)? == 0,
            "clip event records need native event conversion"
        );
    }
    let mut native = source.clone();
    let mut streams = Vec::new();
    for at in (0x10..0x68).step_by(8) {
        if source.u64(at)? != 0 {
            streams.push(
                stream(&source, &mut native, at)
                    .with_context(|| format!("animation stream at {at:X}"))?,
            );
        }
    }
    ensure!(!streams.is_empty(), "clip contains no tracks");
    for at in [0xA8, 0xB8, 0xC8, 0xD8, 0xE8, 0xF8, 0x108] {
        array(&source, &mut native, at, 2, 0x8080000A, 0x8080000A)?;
    }
    for at in [0x90, 0x150] {
        array(&source, &mut native, at, 8, 0x80800070, 0x80800070)?;
    }
    if counterpart.is_none() {
        array(&source, &mut native, 0x170, 8, 0x80808C5F, 0x8080907F)?;
    }
    // The modern fixed header has an additional four-byte build field. The
    // following byte counts and track counts retain their native widths.
    ensure!(
        source.0[0x148..0x150].iter().all(|b| *b == 0),
        "clip fixed-header reserved bytes differ"
    );
    native.0[0x128..0x144].copy_from_slice(&source.0[0x12C..0x148]);
    native.0[0x144..0x150].fill(0);
    let preserved = streams
        .iter()
        .all(|s| s["encoded_tracks_preserved"] == true);
    let size = native.0.len() as u64;
    native.0[..8].copy_from_slice(&size.to_le_bytes());
    let mut report = json!({"source_class":"80808BE0","native_class":"80808F49",
        "name_hash":format!("{:08X}",source.u32(0x120)?),"streams":streams,
        "encoded_tracks_preserved":preserved,"event_count":0,"runtime_ready":false});
    if let Some(counterpart) = counterpart {
        report["events"] = events::carry(&source, &mut native, counterpart)?;
        report["event_count"] = report["events"]["event_count"].clone();
    }
    Ok((native, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d2_mot::rig_convert::write_array;

    fn fixture() -> Payload {
        let mut data = vec![0; 0x220];
        data[0x10..0x18].copy_from_slice(&(0x198i64 - 0x10).to_le_bytes());
        data[0x194..0x198].copy_from_slice(&0x80808B40u32.to_le_bytes());
        data[0x198..0x19A].copy_from_slice(&3u16.to_le_bytes());
        write_array(&mut data, 0x1D0, 0x8080000A, 4, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let header = Payload(data.clone()).pointer(0x1D8).unwrap();
        data[header - 4..header].copy_from_slice(&0x80809FB8u32.to_le_bytes());
        let len = data.len() as u64;
        data[..8].copy_from_slice(&len.to_le_bytes());
        data[0x12C..0x130].copy_from_slice(&16u32.to_le_bytes());
        Payload(data)
    }

    fn modern_array(data: &mut Vec<u8>, at: usize, class: u32, count: usize, bytes: &[u8]) {
        write_array(data, at, class, count, bytes).unwrap();
        let header = Payload(data.clone()).pointer(at + 8).unwrap();
        data[header - 4..header].copy_from_slice(&0x80809FB8u32.to_le_bytes());
    }

    #[test]
    fn identity_position_quantization_relocates_the_index_array() {
        let mut data = vec![0; 0x230];
        data[0x28..0x30].copy_from_slice(&(0x198i64 - 0x28).to_le_bytes());
        data[0x194..0x198].copy_from_slice(&0x80808B48u32.to_le_bytes());
        data[0x198..0x19C].copy_from_slice(&0x0001_0001u32.to_le_bytes());
        modern_array(&mut data, 0x1E8, 0x8080000F, 1, &1f32.to_le_bytes());
        modern_array(&mut data, 0x1F8, 0x8080000F, 1, &0f32.to_le_bytes());
        modern_array(&mut data, 0x208, 0x80800006, 2, &[1, 0, 11, 0]);
        let len = data.len() as u64;
        data[..8].copy_from_slice(&len.to_le_bytes());
        let source = Payload(data);
        let (native, _) = convert(&source.0).unwrap();
        assert_eq!(native.u32(0x194).unwrap(), 0x80808F78);
        let range = native.array_range(0x1E8, 2, Some(0x80800006)).unwrap();
        assert_eq!(&native.0[range], &[1, 0, 11, 0]);
        assert!(native.0[0x1F8..0x218].iter().all(|b| *b == 0));
        let mut invalid = source.clone();
        let at = source.array(0x1E8, 4, None).unwrap()[0];
        word(&mut invalid, at, 2f32.to_bits());
        assert!(convert(&invalid.0).is_err());
        invalid = source;
        word(&mut invalid, 0x198, 0x0002_0001);
        assert!(convert(&invalid.0).is_err());
    }

    #[test]
    fn blend_curves_keep_their_vertices_and_edges() {
        for (source_class, target_class, edge_class, target_edge, stride) in [
            (0x80808B30u32, 0x80808F5F, 0x80808B3A, 0x80808F69, 4),
            (0x80808B32, 0x80808F61, 0x80808B3B, 0x80808F6A, 6),
            (0x80808B34, 0x80808F63, 0x80808B3D, 0x80808F6C, 4),
        ] {
            let mut data = vec![0; 0x1C0];
            data[0x50..0x58].copy_from_slice(&(0x198i64 - 0x50).to_le_bytes());
            data[0x194..0x198].copy_from_slice(&source_class.to_le_bytes());
            modern_array(&mut data, 0x198, 0x80808B3F, 3, &[0; 24]);
            let edges = [0, 1, 2, 0xFF, 0xFF, 0xFF];
            modern_array(&mut data, 0x1A8, edge_class, 1, &edges[..stride]);
            let len = data.len() as u64;
            data[..8].copy_from_slice(&len.to_le_bytes());
            let (native, _) = convert(&data).unwrap();
            assert_eq!(native.u32(0x194).unwrap(), target_class);
            let range = native
                .array_range(0x1A8, stride, Some(target_edge))
                .unwrap();
            assert_eq!(&native.0[range], &edges[..stride]);
        }
    }

    #[test]
    fn constant_track_records_preserve_their_complete_layout() {
        let mut data = vec![0; 0x1B0];
        data[0x30..0x38].copy_from_slice(&(0x198i64 - 0x30).to_le_bytes());
        data[0x194..0x198].copy_from_slice(&0x80808B2Au32.to_le_bytes());
        let mut records = [0u8; 128];
        records[20..24].copy_from_slice(&0x82E952D3u32.to_le_bytes());
        records[64 + 60..128].copy_from_slice(&1f32.to_le_bytes());
        modern_array(&mut data, 0x198, 0x80808B2D, 2, &records);
        let len = data.len() as u64;
        data[..8].copy_from_slice(&len.to_le_bytes());
        let (native, _) = convert(&data).unwrap();
        assert_eq!(native.u32(0x194).unwrap(), 0x80808F59);
        assert_eq!(
            &native.0[native.array_range(0x198, 64, Some(0x80808F5C)).unwrap()],
            &records
        );
    }

    #[test]
    fn clip_lowering_preserves_tracks_and_rejects_unknown_codecs_and_events() {
        let source = fixture();
        let (native, _) = convert(&source.0).unwrap();
        assert_eq!(native.u32(0x194).unwrap(), 0x80808F6F);
        assert_eq!(native.u32(0x128).unwrap(), 16);
        let range = source.array_range(0x1D0, 2, Some(0x8080000A)).unwrap();
        assert_eq!(source.0[range.clone()], native.0[range]);
        assert_eq!(source.u32(0x194).unwrap(), 0x80808B40);
        let mut invalid = source.clone();
        word(&mut invalid, 0x194, 0x80808B48);
        assert!(convert(&invalid.0).is_err());
        invalid = source.clone();
        word(&mut invalid, 0x160, 1);
        assert!(convert(&invalid.0).is_err());
        invalid = source;
        word(&mut invalid, 0x1D0, 100_000);
        assert!(convert(&invalid.0).is_err());
    }
}
