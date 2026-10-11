//! Checked surface roles adapted to the native deferred surface contract.
use super::{
    cache::Cache,
    material::{self, Material},
    resource::Pages,
};
use crate::{
    presentation::{Graph, append, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};
mod surface;

pub(super) struct Binding<'a> {
    pub palette: &'a [u8],
    pub rigid: bool,
    pub objects: &'a BTreeMap<String, u8>,
    pub overrides: &'a BTreeMap<String, BTreeMap<String, [f32; 4]>>,
    pub lenses: &'a BTreeMap<u32, Lens>,
}

#[derive(Clone)]
pub(super) struct Lens {
    pub center: [f32; 3],
    pub rear: f32,
    pub radius: f32,
    pub reticle: bool,
}

const VERTEX: &str = r#"
$SOURCE_PALETTE$
cbuffer Object : register(b11) {float4 objectData[776];};
cbuffer View : register(b12) {float4 viewData[14];};
void main(float4 position:POSITION0,float4 weight:BLENDWEIGHT0,uint4 bone:BLENDINDICES0,float2 uv:TEXCOORD0,float4 normal:NORMAL0,float4 tangent:TANGENT0,
out float4 o0:TEXCOORD0,out float4 o1:TEXCOORD1,out float4 o2:TEXCOORD2,out float4 o3:TEXCOORD3,out float3 o4:TEXCOORD4,out float4 projected:SV_POSITION0) {
float4 p=float4(position.xyz*objectData[5].www+objectData[5].xyz,1);
$MOTION$
float3 world=float3(dot(r0,p),dot(r1,p),dot(r2,p));
float3 n=normalize(float3(dot(r0.xyz,normal.xyz),dot(r1.xyz,normal.xyz),dot(r2.xyz,normal.xyz)));
float3 t=normalize(float3(dot(r0.xyz,tangent.xyz),dot(r1.xyz,tangent.xyz),dot(r2.xyz,tangent.xyz)));
float3 relative=world-viewData[7].xyz;
projected=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];
o0=float4(n,saturate(objectData[7].w+saturate(objectData[7].z*n.z)));
o1=float4(t,p.z);o2=float4(cross(n,t)*tangent.w,0);
o3=(uv*objectData[6].xy+objectData[6].zw).xyxy;o4=p.xyz;
}
"#;

fn program(
    native: &mut Reader,
    g: &mut Graph,
    symbol: &str,
    template: u32,
    text: &str,
    vertex: bool,
) -> Result<()> {
    let (bytes, warnings) = crate::tiger::shader::compile(text, vertex)?;
    fs::write(g.root.join(format!("{symbol}.hlsl")), text)?;
    fs::write(g.root.join(format!("{symbol}.log")), warnings)?;
    let mut header = native.tag(template, None)?.0.clone();
    put(&mut header, 8, &u32::try_from(bytes.len())?.to_le_bytes())?;
    let data = format!("{symbol}-data");
    g.add(symbol, template, &header, Some(&data), vec![])?;
    g.add(
        &data,
        native.reference(template)?,
        &bytes,
        Some(symbol),
        vec![],
    )
}

fn texture(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    mapping: &material::Mapping,
    normal: bool,
    srgb: bool,
) -> Result<String> {
    let mut image = material::image(
        c,
        pages,
        mapping.bitmap.as_ref().context("bitmap")?,
        mapping.static_frame(c)?,
        normal,
    )?;
    let animated = mapping.ammunition_place().is_some();
    if animated {
        let tag = mapping.bitmap.as_ref().context("Animated bitmap")?;
        ensure!(
            c.block(tag.address()? + 124, 56)?.len() == 10 && mapping.frame == 0,
            "Decimal frame function needs ten bitmap frames beginning at zero"
        );
        let frames = (0..10)
            .map(|frame| material::image(c, pages, tag, frame, normal))
            .collect::<Result<Vec<_>>>()?;
        let width = image.width;
        let height = image.height;
        ensure!(
            frames
                .iter()
                .all(|f| f.width == width && f.height == height && f.format == image.format),
            "Decimal bitmap frame dimensions or formats differ"
        );
        image.width *= 10;
        image.rgba.resize(image.width * height * 4, 0);
        for (index, frame) in frames.iter().enumerate() {
            for y in 0..height {
                let start = (y * image.width + index * width) * 4;
                image.rgba[start..start + width * 4]
                    .copy_from_slice(&frame.rgba[y * width * 4..(y + 1) * width * 4]);
            }
        }
    }
    let symbol = format!(
        "{}-{}{}",
        image.name(),
        if srgb { "srgb" } else { "linear" },
        if animated { "-decimal" } else { "" }
    );
    if g.nodes.iter().any(|n| n["symbol"] == symbol) {
        return Ok(symbol);
    }
    let template = if srgb { 0x80ec31b9 } else { 0x80ec31bc };
    let mut header = native.tag(template, None)?.0.clone();
    put(
        &mut header,
        0,
        &u32::try_from(image.rgba.len())?.to_le_bytes(),
    )?;
    put(
        &mut header,
        4,
        &(if srgb { 29u32 } else { 28u32 }).to_le_bytes(),
    )?;
    put(&mut header, 14, &u16::try_from(image.width)?.to_le_bytes())?;
    put(&mut header, 16, &u16::try_from(image.height)?.to_le_bytes())?;
    put(&mut header, 18, &1u16.to_le_bytes())?;
    put(&mut header, 20, &1u16.to_le_bytes())?;
    header[22] = 0;
    header[23] = 1;
    put(&mut header, 36, &u32::MAX.to_le_bytes())?;
    crate::tiger::texture::resident(&mut header, image.rgba.len())?;
    let data = format!("{symbol}-data");
    g.add(&symbol, template, &header, Some(&data), vec![])?;
    g.add(
        &data,
        native.reference(template)?,
        &image.rgba,
        Some(&symbol),
        vec![],
    )?;
    Ok(symbol)
}

fn sample(slot: usize, mapping: &material::Mapping) -> String {
    let transform = mapping.transform;
    let uv = format!(
        "v3.xy*float2({:.9},{:.9})+float2({:.9},{:.9})",
        transform[0], transform[1], transform[2], transform[3]
    );
    if let Some(place) = mapping.ammunition_place() {
        format!("Map{slot}.Sample(SurfaceSampler,decimalUv({uv},{place}.0))")
    } else {
        format!("Map{slot}.Sample(SurfaceSampler,{uv})")
    }
}

fn decimal_display(c: &Cache, pages: &mut Pages, material: &Material) -> Result<Option<String>> {
    let Some(mapping) = material
        .properties
        .iter()
        .flat_map(|p| &p.textures)
        .find(|mapping| mapping.ammunition_place().is_some())
    else {
        return Ok(None);
    };
    let frame = material::image(
        c,
        pages,
        mapping.bitmap.as_ref().context("Decimal bitmap")?,
        0,
        false,
    )?;
    Ok(Some(format!(
        "cbuffer Magazine : register(b0) {{ float4 magazine; }};\nfloat2 decimalUv(float2 uv,float place) {{ float digit=fmod(floor(max(magazine.x,0)/place),10); return float2((digit+clamp(uv.x,{:.9},{:.9}))/10,clamp(uv.y,{:.9},{:.9})); }}",
        0.5 / frame.width as f32,
        1. - 0.5 / frame.width as f32,
        0.5 / frame.height as f32,
        1. - 0.5 / frame.height as f32
    )))
}

pub(super) fn build(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    materials: &BTreeMap<u32, Material>,
    binding: Binding<'_>,
) -> Result<Vec<Value>> {
    let shell = native.tag(0x80ec270d, Some(0x808071e8))?;
    let transparent_shell = native.tag(0x815246aa, Some(0x808071e8))?;
    ensure!(
        transparent_shell.0[32] == 0x88,
        "Native premultiplied surface state differs"
    );
    let palette = binding.palette;
    ensure!(
        !palette.is_empty() && palette.len() <= 256,
        "Invalid source motion palette"
    );
    let lookup = format!(
        "static const uint motionPalette[{}] = {{{}}};",
        palette.len(),
        palette
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    let motion = if binding.rigid {
        "float4 r0=float4(objectData[0].x,objectData[1].x,objectData[2].x,objectData[3].x);float4 r1=float4(objectData[0].y,objectData[1].y,objectData[2].y,objectData[3].y);float4 r2=float4(objectData[0].z,objectData[1].z,objectData[2].z,objectData[3].z);"
    } else {
        "float4 r0=0,r1=0,r2=0;[unroll] for(uint i=0;i<4;i++){uint row=8+motionPalette[bone[i]]*3;r0+=objectData[row]*weight[i];r1+=objectData[row+1]*weight[i];r2+=objectData[row+2]*weight[i];}"
    };
    let vertex = VERTEX
        .replace("$SOURCE_PALETTE$", &lookup)
        .replace("$MOTION$", motion);
    program(native, g, "vertex", shell.u32(0x48)?, &vertex, true)?;
    let mut report = Vec::new();
    for (&key, material) in materials {
        let surface = surface::Surface::new(material, &binding, key);
        let display = decimal_display(c, pages, material)?;
        let animated = display.is_some();
        let display = display.unwrap_or_default();
        let mut bindings = Vec::new();
        let mut samples = BTreeMap::new();
        for (slot, roles, normal, srgb) in [
            (0, &["base_map", "base_map_m_0"][..], false, true),
            (1, &["bump_map", "bump_map_m_0"][..], true, false),
            (2, &["material_texture"][..], false, false),
            (
                3,
                &["alpha_mask_map", "alpha_test_map", "alpha_map"][..],
                false,
                false,
            ),
            (
                4,
                &["self_illum_map", "meter_map"][..],
                false,
                material
                    .options
                    .get("self_illumination")
                    .is_none_or(|v| v != "3_channel_self_illum"),
            ),
            (5, &["detail_map"][..], false, true),
            (6, &["noise_map_a"][..], false, false),
            (7, &["noise_map_b"][..], false, false),
            (8, &["palette"][..], false, true),
        ] {
            if let Some(mapping) = material.mapping(roles) {
                bindings.push((slot, texture(c, pages, native, g, mapping, normal, srgb)?));
                samples.insert(slot, sample(slot, mapping));
            }
        }
        let base = samples
            .get(&0)
            .cloned()
            .unwrap_or_else(|| "float4(1,1,1,1)".into());
        let tint = surface.constant("albedo_color").unwrap_or([1.; 4]);
        let mut color = format!(
            "({base})*float4({:.9},{:.9},{:.9},{:.9})",
            tint[0], tint[1], tint[2], tint[3]
        );
        if material
            .options
            .get("albedo")
            .is_some_and(|m| m.contains("detail"))
            && let Some(detail) = samples.get(&5)
        {
            color = format!("({color})*float4(2*({detail}).rgb,1)");
        }
        let emission_tint = surface
            .constant("self_illum_color")
            .or_else(|| surface.constant("self_illum_tint_color"))
            .unwrap_or([1.; 4]);
        let emission_scale = surface
            .constant("self_illum_intensity")
            .map_or(1., |v| v[0])
            .max(0.);
        let emission = surface.emission(&samples, &color);
        let normal = samples
            .get(&1)
            .map(|s| format!("({s}).xyz*2-1"))
            .unwrap_or_else(|| "float3(0,0,1)".into());
        let rough = samples
            .get(&2)
            .map(|s| format!("saturate(1-({s}).r)"))
            .unwrap_or_else(|| "0.48".into());
        let clip = if material.alpha_mode() == "MASK" {
            format!(
                "clip({}-0.5);",
                samples
                    .get(&3)
                    .map(|s| format!("({s}).a"))
                    .unwrap_or_else(|| "albedo.a".into())
            )
        } else {
            String::new()
        };
        let output = surface.output();
        let finish = surface.finish();
        let text = format!(
            r#"
Texture2D<float4> Map0:register(t0);Texture2D<float4> Map1:register(t1);Texture2D<float4> Map2:register(t2);Texture2D<float4> Map3:register(t3);Texture2D<float4> Map4:register(t4);Texture2D<float4> Map5:register(t5);
Texture2D<float4> Map6:register(t6);Texture2D<float4> Map7:register(t7);Texture2D<float4> Map8:register(t8);
{display}
SamplerState SurfaceSampler:register(s3);
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,uint front:SV_isFrontFace0,
{output}) {{
float4 albedo={color};{clip}float3 n={normal};n=normalize(n.x*v1.xyz+n.y*v2.xyz+n.z*v0.xyz);
float3 emission={emission};float roughness={rough};{finish}
}}
"#
        );
        let shader = format!("pixel-{key:08X}");
        program(native, g, &shader, shell.u32(0x2c8)?, &text, false)?;
        let template = if surface.transparent() {
            0x815246aa
        } else {
            0x80ec270d
        };
        let envelope = if surface.transparent() {
            &transparent_shell
        } else {
            &shell
        };
        let mut bytes = fresh_surface(envelope)?;
        let mut fixed = Vec::new();
        for (slot, _) in &bindings {
            fixed.extend((*slot as u32).to_le_bytes());
            fixed.extend(u32::MAX.to_le_bytes());
        }
        append(&mut bytes, 0x2d0, 0x80807211, &fixed, 8)?;
        ensure!(
            shell.array(0x308, 16, None)?.len() > 2,
            "Native sampler table is missing"
        );
        let samplers = shell
            .array(0x308, 16, None)?
            .into_iter()
            .map(|at| shell.bytes::<16>(at))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let sampler_class = shell.u32(shell.array(0x308, 16, None)?[0] - 8)?;
        append(&mut bytes, 0x308, u64::from(sampler_class), &samplers, 16)?;
        let mut expression = if bindings.is_empty() {
            vec![]
        } else {
            vec![0x4c, 2, 0x49, 0x23]
        };
        if animated {
            let input = *binding
                .objects
                .get("CAF915D8")
                .context("Native current-magazine model input")?;
            expression.extend([0x4d, input, 0x43, 0]);
            bytes[0x33c] = 0x10;
            put(
                &mut bytes,
                0x338,
                &u32::try_from(binding.objects.len())?.to_le_bytes(),
            )?;
            put(&mut bytes, 0x348, &0u32.to_le_bytes())?;
        }
        append(
            &mut bytes,
            0x318,
            0x80800090,
            if animated { &[0; 16] } else { &[] },
            16,
        )?;
        put(&mut bytes, 0x34c, &u32::MAX.to_le_bytes())?;
        append(&mut bytes, 0x2e8, 0x80800009, &expression, 1)?;
        let mut patches = vec![
            json!({"offset":0x48,"symbol":"vertex"}),
            json!({"offset":0x2c8,"symbol":shader}),
        ];
        for (row, (_, symbol)) in Payload(bytes.clone())
            .array(0x2d0, 8, None)?
            .into_iter()
            .zip(&bindings)
        {
            patches.push(json!({"offset":row+4,"symbol":symbol}));
        }
        g.add(
            &format!("material-{key:08X}"),
            template,
            &bytes,
            None,
            patches,
        )?;
        report.push(json!({"source":material.tag,"source_properties":material.properties,"effective_constants":surface.constants,"options":material.options,"bound_textures":bindings,"albedo_color":tint,"emission_color":emission_tint,"emission_scale":emission_scale,"alpha":material.alpha_mode(),"native_stage":if surface.transparent() {7} else {0},"limits":["Opaque surfaces use the native deferred approximation. Transparent surfaces use premultiplied output. Decimal ammunition frame functions are bound to the live magazine input. Other source functions, activation events, plasma depth fading and layered overlays are not translated. Explicit overrides select a static appearance state."]}));
    }
    shadow(native, g)?;
    Ok(report)
}

fn fresh_surface(envelope: &Payload) -> Result<Vec<u8>> {
    let mut bytes = envelope.0.clone();
    for at in [24, 28] {
        put(
            &mut bytes,
            at,
            &(envelope.u32(at)? & !0x1e000000).to_le_bytes(),
        )?;
    }
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    // Fresh programs use only the native object/view buffers and explicit bindings.
    for stage in [0x48, 0xe8, 0x188, 0x228, 0x2c8, 0x368] {
        bytes[stage..stage + 0xa0].fill(0);
        put(&mut bytes, stage, &u32::MAX.to_le_bytes())?;
        if [0x48, 0x2c8].contains(&stage) {
            put(&mut bytes, stage + 0x80, &u32::MAX.to_le_bytes())?;
        }
        put(&mut bytes, stage + 0x84, &u32::MAX.to_le_bytes())?;
    }
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x2c8, &u32::MAX.to_le_bytes())?;
    Ok(bytes)
}

fn shadow(native: &mut Reader, g: &mut Graph) -> Result<()> {
    let template = native.tag(0x80ec271d, Some(0x808071e8))?;
    // The donor's dissolve program reads weapon object channels. A projectile
    // has no such channel array, so retaining that program faults on its first
    // shadow draw. These imported surfaces have no translated dissolve state.
    program(
        native,
        g,
        "shadow-pixel",
        template.u32(0x2c8)?,
        "void main() {}",
        false,
    )?;
    let mut bytes = template.0.clone();
    // Keep the native pixel-bearing shadow envelope, with a fresh stage that
    // writes depth and needs no material expressions or constant resources.
    bytes[0x2c8..0x368].fill(0);
    put(&mut bytes, 0x348, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x34c, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x2c8, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    g.add(
        "material-shadow",
        0x80ec271d,
        &bytes,
        None,
        vec![
            json!({"offset":0x48,"symbol":"vertex"}),
            json!({"offset":0x2c8,"symbol":"shadow-pixel"}),
        ],
    )
}
