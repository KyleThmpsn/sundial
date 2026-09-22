//! Lossless BC plate composition and native gear atlas construction.
use crate::d2_mot::{
    payload::Payload,
    reader::{outside, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

const MAX_SIDE: usize = 16384;
// The resident gear-plate path stalls on a 4096 canvas in the native client.
// Keep source BC mip blocks intact while fitting that path's 2048 canvas.
const RESIDENT_SIDE: usize = 2048;

pub(crate) struct Plate {
    pub side: usize,
    pub format: u32,
    pub mips: usize,
    pub data: Vec<u8>,
}

fn block_size(format: u32) -> Result<usize> {
    match format {
        71 | 72 => Ok(8),
        98 | 99 => Ok(16),
        _ => anyhow::bail!("plate format {format} is not BC1 or BC7"),
    }
}

pub(crate) fn compose(header: &Payload, data: &[u8], placement: [usize; 4]) -> Result<Plate> {
    let [x, y, width, height] = placement;
    ensure!(
        width > 0 && height > 0 && x <= MAX_SIDE && y <= MAX_SIDE,
        "invalid plate placement"
    );
    ensure!(
        header.u16(34)? as usize == width && header.u16(36)? as usize == height,
        "plate placement requires resampling"
    );
    ensure!(
        header.u16(38)? == 1 && header.u16(40)? == 1,
        "plate is not a 2D texture"
    );
    ensure!(
        header.u32(60)? == u32::MAX && header.u32(0)? as usize == data.len(),
        "incomplete plate data"
    );
    let side = (x.checked_add(width).context("plate width overflow")?)
        .max(y.checked_add(height).context("plate height overflow")?)
        .next_power_of_two();
    ensure!(side <= MAX_SIDE, "plate exceeds D3D11 texture size");
    let format = header.u32(4)?;
    let block = block_size(format)?;
    let mips = header.u8(45)? as usize;
    ensure!((1..=15).contains(&mips), "invalid plate mip count");
    let expected: usize = (0..mips)
        .map(|mip| (width >> mip).max(1).div_ceil(4) * (height >> mip).max(1).div_ceil(4) * block)
        .sum();
    ensure!(expected == data.len(), "plate mip byte count differs");
    let mut output = vec![];
    let mut offset = 0;
    let mut retained = 0;
    for mip in 0..mips {
        let (w, h, level_side) = (
            (width >> mip).max(1),
            (height >> mip).max(1),
            (side >> mip).max(1),
        );
        if !((x >> mip) % 4 == 0 && (y >> mip) % 4 == 0 && w >= 4 && h >= 4) {
            // A native texture may expose a partial mip chain. Keep the lossless
            // aligned prefix instead of copying a partial BC block across another
            // tile. Never accept an unaligned highest-resolution placement.
            if retained > 0 {
                break;
            }
            return Err(crate::d2_mot::source_limit(anyhow::anyhow!(
                "plate placement loses BC block alignment"
            )));
        }
        let pitch = level_side.div_ceil(4) * block;
        let row_bytes = w.div_ceil(4) * block;
        let rows = h.div_ceil(4);
        let mut level = vec![0; pitch * level_side.div_ceil(4)];
        for row in 0..rows {
            let src = offset + row * row_bytes;
            let dst = ((y >> mip) / 4 + row) * pitch + (x >> mip) / 4 * block;
            let bytes = data
                .get(src..src + row_bytes)
                .context("truncated plate mip")?;
            level
                .get_mut(dst..dst + row_bytes)
                .context("plate placement exceeds canvas")?
                .copy_from_slice(bytes);
        }
        output.extend(level);
        offset += row_bytes * rows;
        retained += 1;
    }
    Ok(Plate {
        side,
        format,
        mips: retained,
        data: output,
    })
}

fn layout(sides: &[usize], mips: usize) -> Result<([usize; 2], Vec<[usize; 4]>, usize)> {
    ensure!(!sides.is_empty(), "no source plates");
    if sides.len() == 1 {
        return Ok(([sides[0]; 2], vec![[0, 0, sides[0], sides[0]]], 0));
    }
    let pad = 4usize
        .checked_shl((mips - 1) as u32)
        .context("atlas padding overflow")?;
    let mut rectangles = vec![];
    let mut x = 0usize;
    for &side in sides {
        ensure!(
            side % pad == 0,
            "plate is not aligned at retained mip levels"
        );
        rectangles.push([x + pad, pad, side, side]);
        x = x
            .checked_add(side + 2 * pad)
            .context("atlas width overflow")?;
    }
    let mut width = x
        .checked_next_power_of_two()
        .context("atlas width overflow")?;
    let mut height = (sides.iter().max().context("empty atlas")? + 2 * pad).next_power_of_two();
    if width > MAX_SIDE && height <= MAX_SIDE {
        // Keep source order and BC-aligned margins while placing overflow on
        // another shelf. Every interior block remains lossless at every mip.
        rectangles.clear();
        let (mut x, mut y, mut shelf, mut extent) = (0usize, 0usize, 0usize, 0usize);
        for &side in sides {
            let span = side + 2 * pad;
            ensure!(span <= MAX_SIDE, "atlas exceeds D3D11 texture size");
            if x + span > MAX_SIDE {
                y += shelf;
                x = 0;
                shelf = 0;
            }
            rectangles.push([x + pad, y + pad, side, side]);
            x += span;
            extent = extent.max(x);
            shelf = shelf.max(span);
        }
        width = extent.next_power_of_two();
        height = (y + shelf).next_power_of_two();
    }
    ensure!(
        width <= MAX_SIDE && height <= MAX_SIDE,
        "atlas exceeds D3D11 texture size"
    );
    Ok(([width, height], rectangles, pad))
}

fn merge_tiles(tiles: Vec<(Plate, [usize; 4])>) -> Result<Plate> {
    ensure!(!tiles.is_empty(), "empty source plate");
    let side = tiles.iter().map(|(p, _)| p.side).max().unwrap();
    let mips = tiles.iter().map(|(p, _)| p.mips).min().unwrap();
    let format = tiles[0].0.format;
    ensure!(
        tiles.iter().all(|(p, _)| p.format == format),
        "source plate pieces have different formats"
    );
    let block = block_size(format)?;
    let mut offsets = vec![0; tiles.len()];
    let mut data = Vec::new();
    for mip in 0..mips {
        let pitch = (side >> mip).div_ceil(4) * block;
        let mut level = vec![0; pitch * (side >> mip).div_ceil(4)];
        for (i, (plate, [x, y, w, h])) in tiles.iter().enumerate() {
            let source_pitch = (plate.side >> mip).div_ceil(4) * block;
            let row_bytes = (w >> mip).div_ceil(4) * block;
            for row in 0..(h >> mip).div_ceil(4) {
                let sy = (y >> mip) / 4 + row;
                let sx = (x >> mip) / 4 * block;
                let src = offsets[i] + sy * source_pitch + sx;
                let dst = sy * pitch + sx;
                level
                    .get_mut(dst..dst + row_bytes)
                    .context("composed tile exceeds plate")?
                    .copy_from_slice(
                        plate
                            .data
                            .get(src..src + row_bytes)
                            .context("truncated composed tile")?,
                    );
            }
            offsets[i] += source_pitch * (plate.side >> mip).div_ceil(4);
        }
        data.extend(level);
    }
    Ok(Plate {
        side,
        format,
        mips,
        data,
    })
}

fn atlas(
    plates: &[Plate],
    size: [usize; 2],
    rectangles: &[[usize; 4]],
    mips: usize,
    pad: usize,
) -> Result<Vec<u8>> {
    let block = block_size(plates[0].format)?;
    let mut offsets = vec![0; plates.len()];
    let mut output = vec![];
    for mip in 0..mips {
        let pitch = (size[0] >> mip) / 4 * block;
        let mut level = vec![0; pitch * ((size[1] >> mip) / 4)];
        for (i, (plate, rect)) in plates.iter().zip(rectangles).enumerate() {
            let count = (plate.side >> mip) / 4;
            let bytes = plate
                .data
                .get(offsets[i]..offsets[i] + count * count * block)
                .context("truncated atlas source")?;
            offsets[i] += count * count * block;
            let margin = ((pad >> mip) / 4) as isize;
            let bx = ((rect[0] >> mip) / 4) as isize;
            let by = ((rect[1] >> mip) / 4) as isize;
            for row in -margin..count as isize + margin {
                for col in -margin..count as isize + margin {
                    let sy = row.clamp(0, count as isize - 1) as usize;
                    let sx = col.clamp(0, count as isize - 1) as usize;
                    let src = (sy * count + sx) * block;
                    let dst = (by + row) as usize * pitch + (bx + col) as usize * block;
                    level
                        .get_mut(dst..dst + block)
                        .context("atlas rectangle exceeds output")?
                        .copy_from_slice(&bytes[src..src + block]);
                }
            }
            // Read back every interior row independently of the border loop.
            for row in 0..count {
                let dst = (by as usize + row) * pitch + bx as usize * block;
                ensure!(
                    level[dst..dst + count * block]
                        == bytes[row * count * block..(row + 1) * count * block],
                    "atlas source mip readback differs"
                );
            }
        }
        output.extend(level);
    }
    Ok(output)
}

fn resident_layout(size: [usize; 2], mips: usize) -> Result<([usize; 2], usize)> {
    ensure!((1..=15).contains(&mips), "invalid resident mip count");
    ensure!(
        size.iter().all(|s| s.is_power_of_two() && *s <= MAX_SIDE),
        "invalid resident canvas"
    );
    let mut reduced = size;
    let mut first = 0;
    while reduced.iter().any(|s| *s > RESIDENT_SIDE) {
        first += 1;
        ensure!(
            first < mips,
            "source has no mip small enough for the native gear canvas"
        );
        reduced = reduced.map(|s| (s / 2).max(1));
    }
    Ok((reduced, first))
}

fn resident_data(
    data: &[u8],
    size: [usize; 2],
    mips: usize,
    first: usize,
    format: u32,
) -> Result<&[u8]> {
    ensure!(first < mips && mips <= 15, "invalid retained mip range");
    let block = block_size(format)?;
    let mut start = 0;
    let mut end = 0;
    for mip in 0..mips {
        if mip == first {
            start = end;
        }
        end += (size[0] >> mip).max(1).div_ceil(4) * (size[1] >> mip).max(1).div_ceil(4) * block;
    }
    data.get(start..end).context("truncated resident mip chain")
}

/// Read and compose all pieces of one source plate for both graph and atlas conversion.
pub(crate) fn source_plate(source: &Path, provenance: &Value, entries: &[Value]) -> Result<Plate> {
    ensure!(
        !entries.is_empty() && entries.len() <= 64,
        "expected 1..64 source plate pieces"
    );
    let mut tiles = Vec::new();
    for entry in entries {
        let tag = entry["texture"].as_str().context("source texture")?;
        let header = Payload(fs::read(source.join("raw").join(format!("{tag}.bin")))?);
        let reference = provenance["tags"][tag]["reference"]
            .as_u64()
            .context("texture buffer reference")?;
        let data = fs::read(source.join("raw").join(format!("{reference:08X}.bin")))?;
        let mut placement = [0; 4];
        for (i, value) in placement.iter_mut().enumerate() {
            *value = usize::try_from(
                entry["placement"][i]
                    .as_u64()
                    .context("invalid plate placement")?,
            )?;
        }
        tiles.push((compose(&header, &data, placement)?, placement));
    }
    if tiles.len() == 1 {
        Ok(tiles.remove(0).0)
    } else {
        merge_tiles(tiles)
    }
}

pub fn build(source: &Path, output: &Path) -> Result<Value> {
    let output = outside(output, source)?;
    let provenance: Value =
        serde_json::from_slice(&fs::read(source.join("source-manifest.json"))?)?;
    let packages = Path::new(
        provenance["packages"]
            .as_str()
            .context("source package path")?,
    );
    outside(&output, packages.parent().context("package parent")?)?;
    ensure!(!output.exists(), "output directory already exists");
    let report: Value = serde_json::from_slice(&fs::read(source.join("report.json"))?)?;
    let models = report["models"].as_array().context("source models")?;
    ensure!(
        !models.is_empty() && models.len() <= 64,
        "expected 1..64 source models"
    );
    let mut channels = vec![];
    for name in ["albedo", "normal", "gstack"] {
        let mut plates = vec![];
        for model in models {
            let entries = model["texture_plates"][name]
                .as_array()
                .context("source plate entries")?;
            plates.push(source_plate(source, &provenance, entries)?);
        }
        channels.push((name, plates));
    }
    let sides = channels[0].1.iter().map(|p| p.side).collect::<Vec<_>>();
    let mips = channels
        .iter()
        .flat_map(|(_, p)| p.iter().map(|p| p.mips))
        .min()
        .context("missing mips")?;
    let (size, rectangles, pad) = layout(&sides, mips)?;
    let (resident_size, first_mip) = resident_layout(size, mips)?;
    let scale = 1usize << first_mip;
    let resident_rectangles = rectangles
        .iter()
        .map(|r| -> Result<[usize; 4]> {
            ensure!(
                r.iter().all(|v| v % scale == 0),
                "plate placement cannot be scaled exactly"
            );
            Ok(r.map(|v| v / scale))
        })
        .collect::<Result<Vec<_>>>()?;
    fs::create_dir_all(&output)?;
    let mut textures = vec![];
    for (name, plates) in channels {
        ensure!(
            plates.iter().map(|p| p.side).eq(sides.iter().copied()),
            "plate channels have different canvas sizes"
        );
        ensure!(
            plates.iter().all(|p| p.format == plates[0].format),
            "plate formats differ across models"
        );
        let data = if plates.len() == 1 {
            plates[0].data.clone()
        } else {
            atlas(&plates, size, &rectangles, mips, pad)?
        };
        let data = resident_data(&data, size, mips, first_mip, plates[0].format)?;
        let file = format!("{name}.bin");
        fs::write(output.join(&file), data)?;
        textures
            .push(json!({"name":name,"file":file,"format":plates[0].format,"bytes":data.len()}));
    }
    let result = json!({"atlas_size":resident_size,"source_rectangles":resident_rectangles,"retained_mips":mips-first_mip,
        "source_atlas_size":size,"source_atlas_rectangles":rectangles,"dropped_top_mips":first_mip,
        "atlas_required":models.len()>1,"source_plate_placements_composed":true,
        "mip_policy":"lossless block-aligned prefix, native resident size limit",
        "edge_mode":"native clamp","textures":textures,"gameplay_verified":false});
    write_json(&output.join("plates.json"), &result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(width: u16, height: u16, bytes: u32, mips: u8) -> Payload {
        let mut data = vec![0; 64];
        data[..4].copy_from_slice(&bytes.to_le_bytes());
        data[4..8].copy_from_slice(&72u32.to_le_bytes());
        data[34..36].copy_from_slice(&width.to_le_bytes());
        data[36..38].copy_from_slice(&height.to_le_bytes());
        data[38..40].copy_from_slice(&1u16.to_le_bytes());
        data[40..42].copy_from_slice(&1u16.to_le_bytes());
        data[45] = mips;
        data[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        Payload(data)
    }

    #[test]
    fn offset_plate_preserves_blocks_on_square_canvas() {
        let data = (0..160).map(|v| v as u8).collect::<Vec<_>>();
        let plate = compose(&header(8, 32, 160, 2), &data, [16, 0, 8, 32]).unwrap();
        assert_eq!(plate.side, 32);
        assert_eq!(&plate.data[32..48], &data[..16]);
        assert!(plate.data[..32].iter().all(|b| *b == 0));
        assert_eq!(&plate.data[512 + 16..512 + 24], &data[128..136]);
        assert!(compose(&header(8, 32, 159, 2), &data, [16, 0, 8, 32]).is_err());
        assert!(compose(&header(8, 32, 160, 2), &data, [2, 0, 8, 32]).is_err());
    }

    #[test]
    fn unaligned_tail_mips_are_omitted_only_after_validating_the_complete_source() {
        let source = vec![7; 56];
        let plate = compose(&header(4, 16, 56, 3), &source, [0, 0, 4, 16]).unwrap();
        assert_eq!((plate.side, plate.mips, plate.data.len()), (16, 1, 128));
        for row in 0..4 {
            assert_eq!(&plate.data[row * 32..row * 32 + 8], &[7; 8]);
        }
        assert!(compose(&header(4, 16, 55, 3), &source[..55], [0, 0, 4, 16]).is_err());
        assert!(compose(&header(4, 16, 56, 3), &source, [2, 0, 4, 16]).is_err());
    }

    #[test]
    fn multiple_pieces_preserve_source_blocks_at_each_mip() {
        let a = vec![1; 40];
        let b = vec![2; 40];
        let h = header(8, 8, 40, 2);
        let result = merge_tiles(vec![
            (compose(&h, &a, [0, 0, 8, 8]).unwrap(), [0, 0, 8, 8]),
            (compose(&h, &b, [8, 0, 8, 8]).unwrap(), [8, 0, 8, 8]),
        ])
        .unwrap();
        assert_eq!((result.side, result.mips, result.data.len()), (16, 2, 160));
        assert_eq!(&result.data[..16], &[1; 16]);
        assert_eq!(&result.data[16..32], &[2; 16]);
        assert_eq!(&result.data[128..136], &[1; 8]);
        assert_eq!(&result.data[136..144], &[2; 8]);
        assert!(result.data[64..128].iter().all(|v| *v == 0));
    }

    #[test]
    fn source_plate_composes_manifest_pieces_and_rejects_missing_data() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("raw")).unwrap();
        for (tag, buffer, fill) in [("00000001", 3u32, 1u8), ("00000002", 4, 2)] {
            fs::write(
                root.path().join(format!("raw/{tag}.bin")),
                header(8, 8, 40, 2).0,
            )
            .unwrap();
            fs::write(
                root.path().join(format!("raw/{buffer:08X}.bin")),
                [fill; 40],
            )
            .unwrap();
        }
        let provenance = json!({"tags":{"00000001":{"reference":3},"00000002":{"reference":4}}});
        let entries = vec![
            json!({"texture":"00000001","placement":[0,0,8,8]}),
            json!({"texture":"00000002","placement":[8,0,8,8]}),
        ];
        let plate = source_plate(root.path(), &provenance, &entries).unwrap();
        assert_eq!((plate.side, plate.mips, plate.data.len()), (16, 2, 160));
        assert_eq!(&plate.data[..32], [&[1u8; 16][..], &[2u8; 16][..]].concat());
        assert_eq!(
            &plate.data[128..144],
            [&[1u8; 8][..], &[2u8; 8][..]].concat()
        );
        fs::remove_file(root.path().join("raw/00000004.bin")).unwrap();
        assert!(source_plate(root.path(), &provenance, &entries).is_err());
        assert!(source_plate(root.path(), &provenance, &[]).is_err());
    }

    #[test]
    fn atlas_padding_uses_clamped_edge_blocks() {
        let data = (0..4).flat_map(|v| [v; 8]).collect::<Vec<_>>();
        let plate = Plate {
            side: 8,
            format: 72,
            mips: 1,
            data,
        };
        let other = Plate {
            side: 8,
            format: 72,
            mips: 1,
            data: vec![9; 32],
        };
        let (size, rectangles, pad) = layout(&[8, 8], 1).unwrap();
        let bytes = atlas(&[plate, other], size, &rectangles, 1, pad).unwrap();
        let pitch = size[0] / 4 * 8;
        assert_eq!(&bytes[..8], &[0; 8]);
        assert_eq!(&bytes[3 * pitch + 3 * 8..3 * pitch + 4 * 8], &[3; 8]);
        assert_eq!(&bytes[4 * 8..5 * 8], &[9; 8]);
    }

    #[test]
    fn resident_canvas_preserves_existing_bc_mips_and_normalized_placement() {
        let size = [4096, 2048];
        let (reduced, first) = resident_layout(size, 5).unwrap();
        assert_eq!((reduced, first), ([2048, 1024], 1));
        let rect = [64, 64, 2048, 1024];
        let scaled = rect.map(|v| v >> first);
        for i in 0..4 {
            assert_eq!(
                rect[i] as f64 / size[i % 2] as f64,
                scaled[i] as f64 / reduced[i % 2] as f64
            );
        }
        for format in [72, 98] {
            let block = block_size(format).unwrap();
            let mut levels = Vec::new();
            for mip in 0..5 {
                levels.push(vec![
                    mip as u8;
                    (size[0] >> mip) / 4 * (size[1] >> mip) / 4 * block
                ]);
            }
            let data = levels.concat();
            assert_eq!(
                resident_data(&data, size, 5, first, format).unwrap(),
                levels[1..].concat()
            );
            assert!(resident_data(&data[..data.len() - 1], size, 5, first, format).is_err());
        }
        assert_eq!(resident_layout([2048, 2048], 4).unwrap(), ([2048, 2048], 0));
        assert_eq!(
            resident_layout([16384, 8192], 5).unwrap(),
            ([2048, 1024], 3)
        );
        assert!(resident_layout([4096, 4096], 1).is_err());
        assert!(resident_layout([0, 2048], 5).is_err());
    }

    #[test]
    fn single_plate_does_not_expand_and_oversized_atlas_is_rejected() {
        assert_eq!(
            layout(&[2048], 5).unwrap(),
            ([2048, 2048], vec![[0, 0, 2048, 2048]], 0)
        );
        assert!(layout(&[16384, 16384], 5).is_err());
    }

    #[test]
    fn atlas_wraps_without_overlap_and_preserves_mip_alignment() {
        let (size, rects, pad) = layout(&[4096; 4], 5).unwrap();
        assert_eq!(size, [16384, 16384]);
        for (i, a) in rects.iter().enumerate() {
            assert_eq!(a[0] % 64, 0);
            assert_eq!(a[1] % 64, 0);
            assert!(a[0] + a[2] + pad <= size[0]);
            assert!(a[1] + a[3] + pad <= size[1]);
            for b in &rects[i + 1..] {
                assert!(
                    a[0] + a[2] + 2 * pad <= b[0]
                        || b[0] + b[2] + 2 * pad <= a[0]
                        || a[1] + a[3] + 2 * pad <= b[1]
                        || b[1] + b[3] + 2 * pad <= a[1]
                );
            }
        }
    }
}
