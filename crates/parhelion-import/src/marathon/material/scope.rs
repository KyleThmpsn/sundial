//! A native mask or a neutral aiming mark on the source's physical aperture.
use super::*;

/// The highest `TEXCOORD` index a translated shader names, where a bare `TEXCOORD` is index 0.
fn highest_texcoord(code: &str) -> Option<u32> {
    code.match_indices("TEXCOORD")
        .map(|(at, word)| {
            let digits = code[at + word.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            digits.parse().unwrap_or(0)
        })
        .max()
}

/// Source scopes can contain solid display backs and interior geometry. Open their actual
/// aperture in both color and depth passes, keeping all fragments outside its boundary intact.
/// The aperture point takes the varying after the highest one the source pixel program reads,
/// and at least `TEXCOORD8`, so the vertex adapter finds it as the highest pixel varying.
pub(super) fn cutout(mut pixel: String, lens: &geometry::Lens) -> Result<String> {
    let varying = highest_texcoord(&pixel).map_or(8, |index| (index + 1).max(8));
    ensure!(varying < 32, "Source scope leaves no free varying");
    let start = pixel.find("void main(").context("Scope pixel entry")? + "void main(".len();
    let end = pixel[start..]
        .find(") {")
        .context("Scope pixel parameter list")?
        + start;
    let mut coverage =
        String::from("\nfloat2 aperturePoint=apertureVarying.xy;\nfloat apertureCoverage=1e20;\n");
    for edge in &lens.outline {
        coverage.push_str(&format!(
            "apertureCoverage=min(apertureCoverage,dot(float3(aperturePoint,1),float3({},{},{})));\n",
            vertex::float(edge[0]), vertex::float(edge[1]), vertex::float(edge[2])));
    }
    coverage.push_str("clip(-apertureCoverage);\n");
    pixel.insert_str(end + 3, &coverage);
    // Ordinary inputs must precede system values, which a source list can end with. The vertex
    // adapter follows the pixel signature's order, so the registers still pair. A full register
    // keeps the compiler from packing a source varying into its upper lanes.
    let comma = if start == end { "" } else { "," };
    pixel.insert_str(
        start,
        &format!("float4 apertureVarying:TEXCOORD{varying}{comma}"),
    );
    Ok(pixel)
}

/// A weapon whose source eye sits inside its sight box shows only the scene and the reticle once
/// aimed, because the source view never leaves the aperture. Every other surface discards its
/// color, depth and shadow fragments once aim visibility passes 0.99, where Shadowkeep's reticle
/// spline starts to rise. The material expression copies aim visibility to `row`.
pub(super) fn hidden_when_aimed(pixel: String, row: u8) -> Result<String> {
    clip_by_aim(pixel, row, &format!("0.99-cb0[{row}].x"))
}

/// Glass ahead of an enclosed sight's source eye belongs to the aimed view alone, which its vertex
/// stage always projects. It discards until aim visibility passes 0.99, where the rest of the
/// weapon disappears and the framed reticle's window opens.
pub(super) fn shown_when_aimed(pixel: String, row: u8) -> Result<String> {
    clip_by_aim(pixel, row, &format!("cb0[{row}].x-0.99"))
}

fn clip_by_aim(pixel: String, row: u8, kept: &str) -> Result<String> {
    let mut pixel = declare_row(pixel, row)?;
    let start = pixel.find("void main(").context("Pixel entry")?;
    let body = pixel[start..].find(") {").context("Pixel parameter list")? + start + ") {".len();
    pixel.insert_str(body, &format!("\nclip({kept});\n"));
    Ok(pixel)
}

/// Extend or add the material constant declaration so `row` is readable.
pub(super) fn declare_row(mut pixel: String, row: u8) -> Result<String> {
    let rows = usize::from(row) + 1;
    if let Some(line) = pixel.find("register(b0)") {
        let start = pixel[line..]
            .find("cb0[")
            .context("Material constant declaration")?
            + line
            + "cb0[".len();
        let end = pixel[start..]
            .find(']')
            .context("Material constant extent")?
            + start;
        if pixel[start..end].parse::<usize>()? < rows {
            pixel.replace_range(start..end, &rows.to_string());
        }
    } else {
        pixel.insert_str(
            0,
            &format!("cbuffer C0:register(b0) {{ float4 cb0[{rows}]; }};\n"),
        );
    }
    Ok(pixel)
}

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    key: u32,
    surface: &geometry::Surface,
    lens: &geometry::Lens,
    motion: (&str, u32),
) -> Result<Value> {
    let mask = surface.stage == 15 && lens.artwork;
    let template = if mask { 0x80EF50EF } else { 0x815246AA };
    let shell = native.tag(template, Some(0x808071E8))?;
    let vector = |values: &[f32]| {
        values
            .iter()
            .map(|v| vertex::float(*v))
            .collect::<Vec<_>>()
            .join(",")
    };
    let vertex = format!(
        r#"
cbuffer Object:register(b11) {{float4 objectData[{object_rows}];}};
cbuffer View:register(b12) {{float4 viewData[14];}};
{motion}
void main(float4 position:POSITION0,float4 weight:BLENDWEIGHT0,uint4 bone:BLENDINDICES0,out float2 aim:TEXCOORD0,out float3 stored:TEXCOORD4,out float4 projected:SV_POSITION) {{
float4 p=float4(position.xyz*objectData[5].w+objectData[5].xyz,1);
stored=p.xyz;
float3 world,n,t;
deform(p,float3(0,0,1),float3(0,1,0),weight,bone,world,n,t);
float3 relative=world-viewData[7].xyz;
projected=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];
float3 lens=p.xyz-float3({center});
aim=float2(dot(lens,float3({horizontal})),dot(lens,float3({vertical})))/float2({radii});
}}
"#,
        object_rows = 8 + motion.1 * 3,
        motion = motion.0,
        center = vector(&lens.aim),
        horizontal = vector(&lens.axes[0]),
        vertical = vector(&lens.axes[1]),
        radii = vector(&lens.radii)
    );
    let pixel = if mask {
        "void main(out float4 color:SV_TARGET0,out float4 mask:SV_TARGET1){color=0;mask=float4(0,0,0,1);}"
    } else {
        r#"
float4 main(float2 aim:TEXCOORD0):SV_TARGET0 {
float2 edge=max(fwidth(aim),float2(0.001,0.001));
float vertical=(1-smoothstep(0.012,0.012+edge.x,abs(aim.x)))*(1-smoothstep(0.22,0.22+edge.y,abs(aim.y)));
float horizontal=(1-smoothstep(0.012,0.012+edge.y,abs(aim.y)))*(1-smoothstep(0.22,0.22+edge.x,abs(aim.x)));
float mark=max(vertical,horizontal);
return float4(float3(0.15,0.018,0.003)*mark,mark*0.8);
}
"#
    };
    let vertex_name = format!("vertex-{key:08X}");
    let pixel_name = format!("pixel-{key:08X}");
    let vertex_code = program(native, graph, &vertex_name, shell.u32(0x48)?, &vertex, true)?;
    let pixel_code = program(native, graph, &pixel_name, shell.u32(0x2C8)?, pixel, false)?;
    shader::inputs::verify_linkage(&vertex_code, &pixel_code)?;
    let mut bytes = resources::envelope(&shell, if mask { 0 } else { 0x88 })?;
    // These programs consume no source materials, renderer lighting, textures or writable values.
    for offset in [0x50, 0xD8, 0x2D0, 0x2E8, 0x2F8, 0x318] {
        put(&mut bytes, offset, &[0; 16])?;
    }
    bytes[0x33C] = 0;
    put(&mut bytes, 0x338, &0u32.to_le_bytes())?;
    put(&mut bytes, 0x348, &u32::MAX.to_le_bytes())?;
    graph.add(
        &format!("material-{key:08X}"),
        template,
        &bytes,
        None,
        vec![
            json!({"offset":0x48,"symbol":vertex_name}),
            json!({"offset":0x2C8,"symbol":pixel_name}),
        ],
    )?;
    Ok(
        json!({"material":format!("{:08X}",surface.material),"model":format!("{:08X}",surface.model),"source_stage":surface.stage,"native_stage":surface.native_stage(),
        "scope_adapter":if mask {"aperture_mask"} else {"aperture_aiming_mark"},"aperture_center":lens.center,"aim_center":lens.aim,"aperture_radii":lens.radii,"source_eye_enclosed":lens.enclosed,"solid_pass_cutout_supported":!lens.outline.is_empty(),
        "limits":"A native aperture mask or neutral aiming mark. Source glass translates as ordinary transparent work. Source distortion, magnification and live alignment are not established."}),
    )
}
