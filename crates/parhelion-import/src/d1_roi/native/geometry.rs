use super::material::{Draw, Equipment};
use super::*;

pub(super) struct Model {
    pub symbol: String,
    pub vertices: usize,
}

fn primitives(indices: &[u32], primitive: u16, restart: u32) -> Result<u32> {
    let mut count = 0usize;
    match primitive {
        3 => {
            ensure!(
                indices.len().is_multiple_of(3),
                "Source triangle draw has partial primitives"
            );
            for face in indices.chunks_exact(3) {
                ensure!(
                    !face.contains(&restart),
                    "Source triangle list has a restart index"
                );
                if face[0] != face[1] && face[1] != face[2] && face[0] != face[2] {
                    count += 1;
                }
            }
        }
        5 => {
            for strip in indices.split(|index| *index == restart) {
                count += strip
                    .windows(3)
                    .filter(|face| face[0] != face[1] && face[1] != face[2] && face[0] != face[2])
                    .count();
            }
        }
        _ => anyhow::bail!("Untranslated source primitive type {primitive}"),
    }
    Ok(u32::try_from(count)?)
}

impl Assembly<'_> {
    fn buffer(&mut self, tag: u32, indices: bool) -> Result<(String, Payload, usize)> {
        let symbol = format!("buffer-{tag:08X}");
        let source = self.source.read(tag)?;
        let raw = self.source.read(self.source.reference(tag)?)?;
        let subtype = if indices { 6 } else { 4 };
        let template = self.template(32, subtype)?;
        let data_template = self.template(40, subtype)?;
        let mut header = self.native.tag(template, None)?.0.clone();
        let stride = if indices {
            ensure!(
                source.0.len() == 24
                    && header.len() == 24
                    && source.u8(1)? <= 1
                    && source.u32(8)? as usize == raw.0.len(),
                "Source index storage differs"
            );
            header[1] = source.u8(1)?;
            put(&mut header, 8, &u32::try_from(raw.0.len())?.to_le_bytes())?;
            if source.u8(1)? == 0 { 2 } else { 4 }
        } else {
            ensure!(
                source.0.len() == 12
                    && header.len() == 12
                    && source.u32(0)? as usize == raw.0.len(),
                "Source vertex storage differs"
            );
            put(&mut header, 0, &u32::try_from(raw.0.len())?.to_le_bytes())?;
            put(&mut header, 4, &source.bytes::<2>(4)?)?;
            usize::from(source.u16(4)?)
        };
        ensure!(
            stride > 0 && raw.0.len().is_multiple_of(stride),
            "Source buffer stride differs"
        );
        if !self.symbols.contains(&symbol) {
            let data = format!("{symbol}-data");
            self.node(&data, data_template, &raw.0, Some(&symbol), &[])?;
            self.node(&symbol, template, &header, Some(&data), &[])?;
        }
        Ok((symbol, raw, stride))
    }

    #[allow(clippy::cognitive_complexity)]
    pub(super) fn model(
        &mut self,
        source_tag: u32,
        ordinals: &[usize],
        symbol: &str,
        equipment: &Equipment,
        weapon: bool,
    ) -> Result<Model> {
        let source = self.source.read(source_tag)?;
        let meshes = source.array(0x10, 0xA0, Some(0x80801BBF))?;
        let first = *ordinals
            .first()
            .context("Source model has no selected meshes")?;
        let mesh = *meshes.get(first).context("Source mesh is absent")?;
        let transform = source.bytes::<48>(mesh)?;
        let owner = self.native.tag(0x80BC359E, Some(0x80809C36))?;
        let native_tag = owner.u32(owner.pointer(24)? + 0x1DC)?;
        let template = self.native.tag(native_tag, None)?;
        let native_mesh = *template
            .array(16, 136, Some(0x80807378))?
            .first()
            .context("Native model template has no mesh")?;
        let mut out = template
            .0
            .get(..0xA0)
            .context("Native model prefix")?
            .to_vec();
        let native_mesh_bytes = template.bytes::<136>(native_mesh)?;
        let mesh_start = array(
            &mut out,
            16,
            0x80807378,
            &native_mesh_bytes.repeat(ordinals.len()),
            136,
        )?;
        let bones = source.u32(0x40)?;
        ensure!(
            bones > 0 && bones <= self.palette,
            "Source model exceeds the validated runtime palette"
        );
        put(&mut out, 0x40, &bones.to_le_bytes())?;
        put(&mut out, 0x50, &transform)?;
        let mut patches = Vec::new();
        let mut total_vertices = 0usize;
        let mut total_parts = 0usize;
        for (mesh_index, &ordinal) in ordinals.iter().enumerate() {
            let mesh = *meshes.get(ordinal).context("Source mesh is absent")?;
            ensure!(
                source.bytes::<48>(mesh)? == transform,
                "Source meshes have different model transforms"
            );
            let to = mesh_start + mesh_index * 136;
            let mut strides = [0; 4];
            let mut vertices = None;
            for (index, stride) in strides.iter_mut().enumerate() {
                put(&mut out, to + index * 4, &u32::MAX.to_le_bytes())?;
                let tag = source.u32(mesh + 0x30 + index * 4)?;
                if matches!(tag, 0 | u32::MAX) {
                    continue;
                }
                let (symbol, data, size) = self.buffer(tag, false)?;
                *stride = size;
                let count = data.0.len() / size;
                if let Some(previous) = vertices {
                    ensure!(
                        count == previous,
                        "Source vertex streams have different counts"
                    );
                }
                vertices = Some(count);
                patches.push((to + index * 4, symbol));
            }
            let vertices = vertices.context("Source model has no vertex stream")?;
            total_vertices = total_vertices
                .checked_add(vertices)
                .context("Model vertex count overflow")?;
            let (index_symbol, raw, index_size) = self.buffer(source.u32(mesh + 0x40)?, true)?;
            put(&mut out, to + 16, &u32::MAX.to_le_bytes())?;
            patches.push((to + 16, index_symbol));
            let indices = raw
                .0
                .chunks_exact(index_size)
                .map(|b| {
                    if index_size == 2 {
                        u32::from(u16::from_le_bytes(b.try_into().unwrap()))
                    } else {
                        u32::from_le_bytes(b.try_into().unwrap())
                    }
                })
                .collect::<Vec<_>>();
            let restart = if index_size == 2 {
                u32::from(u16::MAX)
            } else {
                u32::MAX
            };
            ensure!(
                indices
                    .iter()
                    .all(|index| *index == restart || (*index as usize) < vertices),
                "Source indices exceed the retained vertex streams"
            );
            let parts = source.array(mesh + 0x48, 36, Some(0x80801AEF))?;
            let mut rows = Vec::new();
            let mut materials = Vec::new();
            let mut covered = 0;
            for stage in 0..23 {
                let (start, end) = if stage < 19 {
                    (
                        source.u16(mesh + 0x58 + stage * 2)? as usize,
                        source.u16(mesh + 0x5A + stage * 2)? as usize,
                    )
                } else {
                    (parts.len(), parts.len())
                };
                ensure!(
                    start == covered && start <= end && end <= parts.len(),
                    "Source stages do not partition their draw records"
                );
                put(
                    &mut out,
                    to + 0x28 + stage * 2,
                    &u16::try_from(start)?.to_le_bytes(),
                )?;
                let layout = if start == end {
                    u16::MAX
                } else {
                    self.layouts
                        .matching(source.u8(mesh + 0x80 + stage)?, strides)?
                };
                put(&mut out, to + 0x58 + stage * 2, &layout.to_le_bytes())?;
                for (index, &part) in parts.iter().enumerate().take(end).skip(start) {
                    let flags = source.u16(part + 0x1C)?;
                    ensure!(
                        flags & !0x777 == 0,
                        "Source draw flags need a native contract: {flags:04X}"
                    );
                    let simulated = flags & 16 != 0;
                    let material = source.u32(part)?;
                    ensure!(
                        !matches!(material, 0 | u32::MAX) && source.i16(part + 4)? == -1,
                        "Source draw uses an unresolved variant material"
                    );
                    let dye = source.u8(part + 0x1E)?;
                    let mat_symbol = format!(
                        "{symbol}-material-{material:08X}-pass-{stage}-dye-{dye}-layout-{layout}-sim-{}",
                        u8::from(simulated)
                    );
                    let mat = self
                    .material(
                        material,
                        &mat_symbol,
                        Draw { stage, dye, simulated, layout },
                        equipment,
                    )
                    .with_context(|| {
                        format!("Source model {source_tag:08X}, mesh {ordinal}, draw {index}, material {material:08X}")
                    })?;
                    let mut row = vec![0; 32];
                    put(&mut row, 0, &u32::MAX.to_le_bytes())?;
                    put(&mut row, 4, &source.bytes::<12>(part + 4)?)?;
                    let start = source.u32(part + 8)? as usize;
                    let count = source.u32(part + 12)? as usize;
                    let draw = indices
                        .get(start..start.checked_add(count).context("Source draw overflow")?)
                        .context("Source draw exceeds the index buffer")?;
                    put(
                        &mut row,
                        16,
                        &primitives(draw, source.u16(part + 6)?, restart)?.to_le_bytes(),
                    )?;
                    put(&mut row, 20, &source.bytes::<2>(part + 0x18)?)?;
                    put(
                        &mut row,
                        22,
                        &u16::try_from(total_parts + index)?.to_le_bytes(),
                    )?;
                    // Source 0087E6F0 and native 0119F900 use exclusion masks
                    // 0x200/0x100 for view 8 and 0x100/0x80 for view 7. A weapon keeps no
                    // exclusion: stock native weapon parts carry 0x0005, 0x0001, 0x0015 or
                    // 0x0025 and never 0x80 or 0x100, while a source weapon's detail parts
                    // carry 0x0301 (and 0x0325, 0x0305, 0x0335) and still draw on the
                    // source's inspect screen, so its views 7 and 8 are not the native
                    // ones, and mapped exclusions hid the Gjallarhorn's detail parts on the
                    // native inspect screen and in the world.
                    let mut native_flags = (flags & 7) | ((flags & !15) >> 1);
                    if weapon {
                        native_flags &= !0x180;
                    }
                    put(&mut row, 24, &native_flags.to_le_bytes())?;
                    row[26] = dye;
                    // Native 011873D0 accepts category 1 across both detailed LOD
                    // ranges. Mapping it to 0 loses the second range. Category 8
                    // covers the two coarser ranges in both qualified formats.
                    row[27] = match source.u8(part + 0x1F)? {
                        category @ (1 | 8) => category,
                        other => {
                            anyhow::bail!("Source draw LOD byte {other} has no native category")
                        }
                    };
                    // Native 01188AE0 also compares this cutoff against the view.
                    // Stock gear uses 127, including its most detailed category.
                    row[28] = 127;
                    row[29] = u8::try_from(source.u32(part + 0x20)?)
                        .context("Source LOD run exceeds native capacity")?;
                    ensure!(
                        row[29] > 0 && usize::from(row[29]) <= end - index,
                        "Source LOD run exceeds its draw stage"
                    );
                    materials.push(mat);
                    rows.extend(row);
                }
                covered = end;
            }
            put(
                &mut out,
                to + 0x56,
                &u16::try_from(parts.len())?.to_le_bytes(),
            )?;
            let start = array(&mut out, to + 0x18, 0x8080737E, &rows, 32)?;
            for (index, material) in materials.into_iter().enumerate() {
                patches.push((start + index * 32, material));
            }
            total_parts = total_parts
                .checked_add(parts.len())
                .context("Model draw count overflow")?;
        }
        // A conservative sphere encloses the source packed-coordinate box.
        let mut squared = 0.;
        for axis in 0..3 {
            let scale = source.f32(mesh + axis * 4)?.abs();
            squared += scale * scale;
            put(
                &mut out,
                0x80 + axis * 4,
                &source.bytes::<4>(mesh + 0x10 + axis * 4)?,
            )?;
        }
        for at in [0x20, 0x24, 0x8C] {
            put(&mut out, at, &squared.sqrt().to_le_bytes())?;
        }
        crate::tiger::draws::declare_draw_indices(&mut out, total_parts)?;
        size(&mut out)?;
        crate::tiger::draws::validate_model(&Payload(out.clone()))?;
        self.node(symbol, native_tag, &out, None, &patches)?;
        self.nodes.last_mut().context("Converted model node")?["model"] = json!(true);
        Ok(Model {
            symbol: symbol.to_owned(),
            vertices: total_vertices,
        })
    }
}
