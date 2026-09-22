use super::*;
use std::collections::BTreeSet;

fn pixel(textured: bool, normal: bool, cutout: bool) -> String {
    format!(
        r#"
Texture2D<float4> Color : register(t0);
Texture2D<float4> Normal : register(t1);
SamplerState ColorSampler : register(s3);
SamplerState NormalSampler : register(s4);
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,uint front:SV_isFrontFace0,
out float4 o0:SV_TARGET0,out float4 o1:SV_TARGET1,out float4 o2:SV_TARGET2) {{
float4 albedo = {color};
{clip}
float3 n = {normal};
n=normalize(n.x*v1.xyz+n.y*v2.xyz+n.z*v0.xyz);
float roughness=0.48;
o0=float4(saturate(albedo.rgb),0.04);
o1=float4(saturate(n*(roughness*0.125+0.375)+0.5),0);
o2=float4(roughness,0.5,0,v0.w);
}}
"#,
        color = if textured {
            "Color.Sample(ColorSampler,v3.xy)"
        } else {
            "float4(0.18,0.20,0.22,1)"
        },
        clip = if cutout { "clip(albedo.a-0.25);" } else { "" },
        normal = if normal {
            "Normal.Sample(NormalSampler,v3.xy).xyz*2-1"
        } else {
            "float3(0,0,1)"
        }
    )
}
fn texture(source: &mut Reader, native: &mut Reader, g: &mut Graph, tag: u32) -> Result<String> {
    let name = format!("texture-{tag:08X}");
    if g.nodes.iter().any(|n| n["symbol"] == name) {
        return Ok(name);
    }
    let h = source.tag(tag, None)?;
    let w = h.u16(34)? as usize;
    let height = h.u16(36)? as usize;
    let fmt = h.u32(4)?;
    ensure!(
        h.u16(38)? == 1 && h.u16(40)? == 1 && w > 0 && height > 0,
        "only 2D source textures supported"
    );
    let block = match fmt {
        71 | 72 => 8,
        98 | 99 => 16,
        _ => anyhow::bail!("Marathon texture format {fmt} not supported"),
    };
    let large = h.u32(60)?;
    let t = if [0, u32::MAX, 0x811c9dc5].contains(&large) {
        source.reference(tag)?
    } else {
        large
    };
    let raw = source.tag(t, None)?;
    let size = w.div_ceil(4) * height.div_ceil(4) * block;
    ensure!(raw.0.len() >= size, "truncated Marathon texture mip");
    // Fixed shader resources require the detail texture's package storage
    // metadata as well as its resident header. Gear-plate entries have the
    // same decoded type but remain unbound when used as fixed resources.
    let header_tag = if matches!(fmt, 72 | 99) {
        0x80ec31b9
    } else {
        0x80ec31bc
    };
    let data_tag = native.reference(header_tag)?;
    let mut header = native.tag(header_tag, None)?.0.clone();
    put(&mut header, 0, &(size as u32).to_le_bytes())?;
    put(&mut header, 4, &fmt.to_le_bytes())?;
    put(&mut header, 14, &h.bytes::<8>(34)?)?;
    header[23] = 1;
    put(&mut header, 36, &u32::MAX.to_le_bytes())?;
    crate::d2_mot::texture::resident(&mut header, size)?;
    g.add(
        &format!("{name}-data"),
        data_tag,
        &raw.0[..size],
        Some(&name),
        vec![],
    )?;
    g.add(
        &name,
        header_tag,
        &header,
        Some(&format!("{name}-data")),
        vec![],
    )?;
    Ok(name)
}
pub fn build(
    source: &mut Reader,
    native: &mut Reader,
    g: &mut Graph,
    materials: BTreeSet<u32>,
) -> Result<()> {
    let shell = native.tag(0x80ec270d, Some(0x808071e8))?;
    for tag in materials {
        let source_mat = source.tag(tag, Some(0x808031d8))?;
        let rows = source_mat.array(0x280, 24, None)?;
        let mut bindings = vec![];
        for row in rows {
            let slot = source_mat.u32(row)?;
            let tex = source.ref64(&source_mat, row + 8)?;
            bindings.push((slot, tex));
        }
        let mut textures = vec![];
        // Surface programs bind albedo at zero. Decal-only programs use slot
        // three. A BC4 texture at slot one is a mask, never a normal map.
        let color = bindings.iter().find(|x| x.0 == 0).or_else(|| {
            if bindings.len() == 1 && bindings[0].0 == 3 {
                bindings.first()
            } else {
                None
            }
        });
        if let Some((_, tex)) = color {
            textures.push((
                0u32,
                texture(source, native, g, *tex)
                    .with_context(|| format!("albedo for material {tag:08X}"))?,
            ));
        }
        if let Some((_, tex)) = bindings.iter().find(|x| x.0 == 1) {
            if matches!(source.tag(*tex, None)?.u32(4)?, 98 | 99) {
                textures.push((
                    1u32,
                    texture(source, native, g, *tex)
                        .with_context(|| format!("normal for material {tag:08X}"))?,
                ));
            }
        }
        let name = format!("material-{tag:08X}");
        let shader = format!("shader-{tag:08X}");
        let text = pixel(
            textures.iter().any(|x| x.0 == 0),
            textures.iter().any(|x| x.0 == 1),
            textures.len() == 1,
        );
        let (code, warnings) = crate::d2_mot::native::shader::compile(&text, false)?;
        fs::write(g.root.join(format!("{shader}.hlsl")), text)?;
        fs::write(g.root.join(format!("{shader}.log")), warnings)?;
        let shader_tag = shell.u32(0x2c8)?;
        let mut h = native.tag(shader_tag, None)?.0.clone();
        put(&mut h, 8, &(code.len() as u32).to_le_bytes())?;
        let bytes = format!("{shader}-data");
        g.add(&shader, shader_tag, &h, Some(&bytes), vec![])?;
        g.add(
            &bytes,
            native.reference(shader_tag)?,
            &code,
            Some(&shader),
            vec![],
        )?;
        let mut mat = shell.0.clone();
        for at in [24, 28] {
            put(&mut mat, at, &(shell.u32(at)? & !0x1e000000).to_le_bytes())?;
        }
        put(&mut mat, 0x2c8, &u32::MAX.to_le_bytes())?;
        let mut fixed = vec![];
        for (slot, _) in &textures {
            fixed.extend(slot.to_le_bytes());
            fixed.extend(u32::MAX.to_le_bytes());
        }
        append(&mut mat, 0x2d0, 0x80807211, &fixed, 8)?;
        // The replacement has no dynamic constants, but texture sampling still
        // needs the native expression that binds the donor's sampler table.
        let mut sampler_code = vec![];
        for (texture_slot, _) in &textures {
            let index = 2 + *texture_slot as u8;
            let register = 3 + *texture_slot as u8;
            ensure!(
                (index as usize) < shell.array(0x308, 16, None)?.len(),
                "native texture sampler is missing"
            );
            sampler_code.extend([0x4c, index, 0x49, 0x20 | register]);
        }
        append(&mut mat, 0x2e8, 0x80800009, &sampler_code, 1)?;
        let mut patches = vec![json!({"offset":0x2c8,"symbol":shader})];
        for (row, (_, symbol)) in Payload(mat.clone())
            .array(0x2d0, 8, None)?
            .iter()
            .zip(&textures)
        {
            patches.push(json!({"offset":row+4,"symbol":symbol}));
        }
        g.add(&name, 0x80ec270d, &mat, None, patches)?;
    }
    let shadow = native.tag(0x80ec271d, Some(0x808071e8))?;
    let mut bytes = shadow.0.clone();
    let mut patches = vec![];
    for offset in [0x48, 0x2c8] {
        let tag = shadow.u32(offset)?;
        let name = format!("shadow-program-{tag:08X}");
        let raw_name = format!("{name}-data");
        let header = native.tag(tag, None)?;
        let raw_tag = native.reference(tag)?;
        let raw = native.tag(raw_tag, None)?;
        g.add(&name, tag, &header.0, Some(&raw_name), vec![])?;
        g.add(&raw_name, raw_tag, &raw.0, Some(&name), vec![])?;
        put(&mut bytes, offset, &u32::MAX.to_le_bytes())?;
        patches.push(json!({"offset":offset,"symbol":name}));
    }
    g.add("material-shadow", 0x80ec271d, &bytes, None, patches)?;
    Ok(())
}
