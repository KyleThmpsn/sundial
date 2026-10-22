//! Source pixel programs, authored inputs and explicit native render passes.
use super::*;
use crate::tiger::shader::{self, dxil, identity::Stage, inputs::Inspection};
use std::collections::{BTreeMap, BTreeSet};
mod channel;
mod expression;
mod lighting;
mod resources;
mod texture;
mod vertex;
use resources::{program, vectors};

fn pixel(
    listing: Option<&str>,
    surface: &geometry::Surface,
    model: &Payload,
    values: &[[u8; 16]],
    written: &BTreeSet<usize>,
    viewports: &BTreeSet<u32>,
) -> Result<String> {
    let Some(listing) = listing else {
        ensure!(
            matches!(surface.stage, 4 | 13),
            "Source surface has no pixel program"
        );
        return Ok("void main() {}".into());
    };
    let parsed = dxil::Program::parse(listing)?;
    let mut options = dxil::Options {
        perspective_depth: surface.stage == 8,
        targets: parsed
            .outputs
            .iter()
            .filter(|f| {
                if matches!(surface.stage, 4 | 13) {
                    false
                } else if surface.stage == 8 {
                    f.index == 0
                } else {
                    f.index < 3
                }
            })
            .map(|f| f.index)
            .collect(),
        ..Default::default()
    };
    options.constants.insert((12, 16), "cb12[12]".into());
    options.constants.insert((12, 17), "cb12[13]".into());
    options.constants.insert((12, 18), "cb12[7]".into());
    if surface.stage == 0 {
        // The inspected source GBuffer uses 0.67/1 for the standard surface
        // flag, where native material writers use 0/0.33. Validate the literal
        // or select producer rather than assuming every material has this form.
        ensure!(
            parsed
                .outputs
                .iter()
                .any(|field| field.index == 1 && field.width == 4),
            "Source GBuffer has no surface flag"
        );
        options.output_values.insert(
            (1, 3),
            BTreeMap::from([
                (0.67f32.to_bits(), 0f32.to_bits()),
                (1f32.to_bits(), 0.33f32.to_bits()),
            ]),
        );
    }
    let offset = surface
        .offset
        .iter()
        .copied()
        .chain([model.f32(0xbc)?])
        .map(vertex::float)
        .collect::<Vec<_>>()
        .join(",");
    options
        .constants
        .insert((1, 5), format!("float4({offset})"));
    if surface.stage == 2 {
        // Source viewport remapping is a renderer-specific material constant.
        // The native decal pass addresses the current full-size GBuffer.
        options
            .constants
            .insert((0, 0), "float4(cb12[12].xy,0,0)".into());
    }
    if surface.stage == 8 {
        for &row in viewports {
            // Source depth coordinates are already normalized screen UVs.
            // Native full-size depth therefore needs dimensions, not unit scale.
            options
                .constants
                .insert((0, row), "float4(cb12[12].xy,0,0)".into());
        }
        // Retain the source shader's unfogged surface branch and apply native
        // inverse exposure. Source atmospheric volumes and velocity are absent.
        options.constants.insert((2, 7), "float4(1,1,1,1)".into());
        options.texture_registers.insert(23, 18);
        options
            .constants
            .insert((13, 1), "float4(cb13[0].y,0,0,1)".into());
        let bindings = parsed.resources()?;
        if (16..=18).all(|slot| bindings.get(&(0, slot)).is_some_and(|&(kind, _)| kind == 4)) {
            lighting::adapt(&mut options, surface);
        }
    }
    for (row, value) in values.iter().enumerate() {
        if !written.contains(&row) {
            let lanes = value
                .chunks_exact(4)
                .map(|lane| format!("{}u", u32::from_le_bytes(lane.try_into().unwrap())))
                .collect::<Vec<_>>()
                .join(",");
            // Preserve exact source bits. Only the stage expression can mutate
            // material rows, and explicit renderer adapters own their overrides.
            options
                .constants
                .entry((0, row as u32))
                .or_insert_with(|| format!("asfloat(uint4({lanes}))"));
        }
    }
    parsed.hlsl(&options)
}

fn inspect(code: &[u8], stage: u8) -> Result<shader::inputs::Inputs> {
    let inspection = Inspection::read(Stage::Pixel, code)?.inputs;
    ensure!(
        inspection.unordered_access.is_empty(),
        "Marathon pixel program writes an unsupported resource"
    );
    for (slot, reads) in &inspection.constant_reads {
        ensure!(
            reads.iter().all(|r| r.index.is_some()),
            "Dynamic source renderer constant indexing"
        );
        match slot {
            0 => {}
            2 => ensure!(
                stage == 8
                    && reads.iter().all(|r| r.index == Some(0)
                        && r.lanes.bytes().all(|lane| matches!(lane, b'x' | b'y'))),
                "Unmapped Marathon depth constants"
            ),
            12 => ensure!(
                reads
                    .iter()
                    .all(|r| matches!(r.index, Some(6 | 7 | 12 | 13))),
                "Unmapped Marathon view constant"
            ),
            13 => ensure!(
                stage == 8
                    && reads.iter().all(|r| r.index == Some(0)
                        && !r.lanes.is_empty()
                        && r.lanes.bytes().all(|lane| lane == b'y')),
                "Unmapped Marathon frame constant"
            ),
            8 => ensure!(
                stage == 8
                    && reads.iter().all(|r| r.index == Some(4)
                        && r.lanes.bytes().all(|lane| matches!(lane, b'z' | b'w'))),
                "Unmapped Marathon lighting constant"
            ),
            11 => ensure!(
                stage == 8 && reads.iter().all(|r| matches!(r.index, Some(0..=2))),
                "Unmapped Marathon rigid constant"
            ),
            _ => anyhow::bail!("Unmapped Marathon constant buffer {slot}: {reads:?}"),
        }
    }
    Ok(inspection)
}

#[allow(clippy::cognitive_complexity)]
pub(super) fn build(
    source: &mut Reader,
    native: &mut Reader,
    graph: &mut Graph,
    mesh: &geometry::Mesh,
) -> Result<Vec<Value>> {
    let globals = channel::Globals::read(source, native)?;
    let objects = crate::tiger::channel::object_channel_map(&graph.read("owner")?.0)?;
    let root = mesh
        .surfaces
        .values()
        .next()
        .context("Missing Marathon surfaces")?
        .entity;
    let parent = channel::bank(source, root)?;
    let mut banks = BTreeMap::new();
    let mut listings = BTreeMap::new();
    let mut reports = Vec::new();
    for (&key, surface) in &mesh.surfaces {
        crate::cancellation::check()?;
        let material = source.tag(surface.material, Some(0x808031d8))?;
        let model = source.tag(surface.model, Some(0x8080881c))?;
        let mut shader_tags = Vec::new();
        for offset in [0x58, 0x278] {
            let tag = material.u32(offset)?;
            if [0, u32::MAX, 0x811c9dc5].contains(&tag) {
                shader_tags.push(None);
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = listings.entry(tag) {
                let code = source.tag(source.reference(tag)?, None)?;
                let text = dxil::disassemble(&code.0)?;
                fs::write(graph.root.join(format!("source-{tag:08X}.ll")), &text)?;
                entry.insert(text);
            }
            shader_tags.push(Some(tag));
        }
        let template = match surface.stage {
            2 => 0x80ba7713,
            8 => 0x815246aa,
            4 | 13 => 0x80ec271d,
            _ => 0x80ec270d,
        };
        let shell = native.tag(template, Some(0x808071e8))?;
        let values = resources::values(source, &material, 0x278)?;
        let code = material
            .array(0x298, 1, None)?
            .into_iter()
            .map(|row| material.u8(row))
            .collect::<Result<Vec<_>>>()?;
        let mut written = expression::written(&code)?;
        let (metadata, samplers) = resources::inputs(source, &material)?;
        if let std::collections::btree_map::Entry::Vacant(entry) = banks.entry(surface.entity) {
            entry.insert(channel::bank(source, surface.entity)?);
        }
        let mut context = expression::Context {
            constants: vectors(&material, 0x2a8)?,
            local: &banks[&surface.entity],
            parent: &parent,
            objects: &objects,
            globals: &globals,
            metadata,
            samplers,
            authored: BTreeMap::new(),
            full_viewport: surface.stage == 8,
        };
        let all_outputs = written.iter().map(|&row| row as u32).collect();
        let planned = context.clone().lower(&code, &values, &all_outputs, false)?;
        let mut specialized = values.clone();
        for (&row, &value) in &planned.constants {
            specialized[row] = value;
            written.remove(&row);
        }
        let pixel_text = pixel(
            shader_tags[1].map(|tag| listings[&tag].as_str()),
            surface,
            &model,
            &specialized,
            &written,
            &planned.viewports,
        )
        .with_context(|| {
            format!(
                "Marathon material {:08X}, model {:08X}, stage {}: translating pixel shader {}",
                surface.material,
                surface.model,
                surface.stage,
                shader_tags[1]
                    .map(|tag| format!("{tag:08X}"))
                    .unwrap_or_else(|| "absent".into())
            )
        })?;
        let pixel_name = format!("pixel-{key:08X}");
        let pixel_code = program(
            native,
            graph,
            &pixel_name,
            shell.u32(0x2c8)?,
            &pixel_text,
            false,
        )?;
        let inspection = inspect(&pixel_code, surface.stage).with_context(|| {
            format!(
                "Marathon material {:08X}, model {:08X}, stage {}: inspecting translated pixel shader {}",
                surface.material, surface.model, surface.stage,
                shader_tags[1].map(|tag| format!("{tag:08X}")).unwrap_or_else(|| "absent".into())
            )
        })?;
        let scene_depth = surface.stage == 8 && inspection.resources.contains_key(&10);
        if scene_depth {
            ensure!(
                pixel_text.contains("/* Native reciprocal depth */")
                    && inspection.resources[&10]
                        == "dcl_resource_texture2d (float,float,float,float) @"
                    && inspection.constant_reads.get(&12).is_none_or(|reads| reads
                        .iter()
                        .all(|r| r.index != Some(13) || !r.lanes.contains('y'))),
                "Source depth projection differs from the native reciprocal-depth contract"
            );
            ensure!(
                material
                    .array(0x280, 24, None)?
                    .into_iter()
                    .all(|row| material.u32(row).is_ok_and(|slot| slot != 10)),
                "Scene depth conflicts with a fixed texture"
            );
            resources::scene_depth(source, native)?;
        }
        let scene_color = surface.stage == 8 && inspection.resources.contains_key(&18);
        let forward_lighting = surface.stage == 8
            && inspection
                .resources
                .keys()
                .any(|slot| (12..=15).contains(slot));
        if forward_lighting {
            lighting::validate(source, native, &material, &inspection)?;
        }
        if scene_color {
            ensure!(
                pixel_text.contains("t23:register(t18)")
                    && inspection.resources[&18]
                        == "dcl_resource_texture2d (float,float,float,float) @",
                "Source scene-color texture type differs"
            );
            ensure!(
                material
                    .array(0x280, 24, None)?
                    .into_iter()
                    .all(|row| { material.u32(row).is_ok_and(|slot| !matches!(slot, 18 | 23)) }),
                "Source scene color conflicts with a fixed texture"
            );
            resources::scene_color(source, native)?;
        }
        let vertex_tag = shader_tags[0].context("Missing source vertex program")?;
        let source_vertex = &listings[&vertex_tag];
        let vertex_values = resources::values(source, &material, 0x58)?;
        let vertex_expression = material
            .array(0x78, 1, None)?
            .into_iter()
            .map(|at| material.u8(at))
            .collect::<Result<Vec<_>>>()?;
        let vertex_written = expression::written(&vertex_expression)?;
        let (vertex_text, uses_colors) = vertex::hlsl(source_vertex, &pixel_code, surface, &vertex_values, &vertex_written)
            .with_context(|| {
                format!(
                    "Marathon material {:08X}, model {:08X}, stage {}: translating vertex shader {:08X}",
                    surface.material, surface.model, surface.stage, vertex_tag
                )
            })?;
        let vertex_name = format!("vertex-{key:08X}");
        let vertex_code = program(
            native,
            graph,
            &vertex_name,
            shell.u32(0x48)?,
            &vertex_text,
            true,
        )?;
        shader::inputs::verify_linkage(&vertex_code, &pixel_code)?;
        let mut bytes = resources::fresh(&shell, &material, surface.stage)?;
        let mut patches = vec![
            json!({"offset":0x48,"symbol":vertex_name}),
            json!({"offset":0x2c8,"symbol":pixel_name}),
        ];
        if uses_colors {
            let symbol = resources::colors(native, graph, mesh)?;
            let mut row = 0u32.to_le_bytes().to_vec();
            row.extend(u32::MAX.to_le_bytes());
            append(&mut bytes, 0x50, 0x80807211, &row, 8)?;
            let at = Payload(bytes.clone()).array(0x50, 8, None)?[0];
            patches.push(json!({"offset":at+4,"symbol":symbol}));
        }
        let bindings = resources::bindings(source, native, graph, &material, &inspection)?;
        bindings.append(native, &mut bytes, &mut patches)?;
        let live = inspection
            .constant_reads
            .get(&0)
            .into_iter()
            .flatten()
            .map(|r| r.index.context("Dynamic material constant read"))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            live.iter().all(|i| (*i as usize) < values.len()),
            "Source material constant buffer is truncated"
        );
        let lowered = context.lower(&code, &values, &live, true)?;
        for slot in &lowered.resources {
            ensure!(
                inspection.resources.get(slot).is_none_or(|kind|
                    kind == "dcl_resource_texture2d (float,float,float,float) @"),
                "Translated scene texture has an incompatible resource type"
            );
        }
        let unbound: Vec<_> = inspection
            .resources
            .keys()
            .filter(|&&slot| {
                !bindings.textures.contains_key(&slot)
                    && !lowered.resources.contains(&slot)
                    && !(scene_color && slot == 18)
                    && !(scene_depth && slot == 10)
                    && !(forward_lighting && (12..=15).contains(&slot))
            })
            .copied()
            .collect();
        ensure!(
            unbound.is_empty(),
            "Marathon material {:08X}, model {:08X}, stage {} requires source renderer texture registers {:?} with no validated native binding",
            surface.material,
            surface.model,
            surface.stage,
            unbound
        );
        ensure!(
            inspection
                .samplers
                .keys()
                .all(|slot| lowered.samplers.contains(slot)),
            "Translated shader has an unbound sampler"
        );
        append(&mut bytes, 0x2e8, 0x80800009, &lowered.code, 1)?;
        let authored: Vec<_> = context
            .authored
            .iter()
            .map(|(&input, &constant)| super::inputs::Binding { constant, input })
            .collect();
        append(
            &mut bytes,
            0x2f8,
            0x80800090,
            &context.constants.into_iter().flatten().collect::<Vec<_>>(),
            16,
        )?;
        let has_values = !values.is_empty();
        append(
            &mut bytes,
            0x318,
            0x80800090,
            &values.into_iter().flatten().collect::<Vec<_>>(),
            16,
        )?;
        put(
            &mut bytes,
            0x338,
            &u32::try_from(objects.len())?.to_le_bytes(),
        )?;
        // Native writable storage requires a nonempty initial output table.
        // The compiled reads were checked against this table before emission.
        bytes[0x33c] = if has_values { 0x10 } else { 0 };
        put(
            &mut bytes,
            0x348,
            &(if has_values { 0u32 } else { u32::MAX }).to_le_bytes(),
        )?;
        graph.add(
            &format!("material-{key:08X}"),
            template,
            &bytes,
            None,
            patches,
        )?;
        if !authored.is_empty() {
            graph.nodes.last_mut().context("Authored material node")?["material_inputs"] =
                json!(authored);
        }
        reports.push(json!({"material":format!("{:08X}",surface.material),"model":format!("{:08X}",surface.model),"source_stage":surface.stage,"native_stage":surface.native_stage(),"pixel":shader_tags[1].map(|t|format!("{t:08X}")),"textures":bindings.textures,"vertex_colors":uses_colors,"inputs":lowered.evidence,"specialized_inputs":planned.evidence.iter().filter(|row| row["output"].as_u64().is_some_and(|i| planned.constants.contains_key(&(i as usize)))).collect::<Vec<_>>(),"pixel_bindings":inspection,"surface_flags":if surface.stage==0{json!("Source standard surface flags adapted to native 0/0.33 encoding")}else{Value::Null},"atmosphere":if surface.stage==8{json!("Source unfogged surface with native exposure")}else{Value::Null}}));
    }
    write_json(&graph.root.join("material-report.json"), &json!(reports))?;
    Ok(reports)
}
