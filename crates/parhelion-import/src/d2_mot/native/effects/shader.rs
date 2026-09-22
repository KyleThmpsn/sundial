use crate::d2_mot::native::shader::replace_once;
use anyhow::{Context, Result, bail, ensure};

fn source_uv(rect: [usize; 4], size: [usize; 2]) -> String {
    let [x, y, w, h] = rect;
    let [aw, ah] = size;
    format!("float2 source_uv = (v3.xy * float2({aw},{ah}) - float2({x},{y})) / float2({w},{h});")
}
fn clamp_samples(mut text: String, slots: &[u8], rect: [usize; 4], size: [usize; 2]) -> String {
    let [x, y, w, h] = rect;
    let [aw, ah] = size;
    let coords =
        format!("(saturate(source_uv) * float2({w},{h}) + float2({x},{y})) / float2({aw},{ah})");
    for slot in slots {
        for sampler in 0..32 {
            text = text.replace(
                &format!("t{slot}.Sample(s{sampler}_s, v3.xy)"),
                &format!("t{slot}.SampleGrad(s{sampler}_s, {coords}, ddx(v3.xy), ddy(v3.xy))"),
            );
        }
    }
    text
}

pub fn decal(
    text: &str,
    blend: u8,
    channel: u8,
    rect: [usize; 4],
    size: [usize; 2],
    source: u32,
) -> Result<String> {
    ensure!(channel <= 5, "source dye selector outside native bank");
    let mut text = replace_once(
        text,
        "uint4 bitmask, uiDest;",
        &format!("uint4 bitmask, uiDest;\n  {}", source_uv(rect, size)),
    )?;
    let c = channel % 2;
    text = text.replace(
        &format!("cb0[{}]", if blend == 29 { 5 } else { 4 }),
        &format!("float4({c},{c},{c},{c})"),
    );
    text = text.replace("cb7[2].w", "(0.0)");
    let simple = matches!(source, 0x80CF9AB9 | 0x80CF5EB7);
    if simple && blend == 29 {
        text = replace_once(&text, "float4 cb7[15];", "float4 cb7[23];")?;
    }
    text = replace_once(
        &text,
        &format!("float4 cb0[{}];", if blend == 29 { 6 } else { 5 }),
        &format!("float4 cb0[{}];", if simple { 6 } else { 8 }),
    )?;
    let dye = 18 + 4 * c;
    match blend {
        29 => {
            text = replace_once(
                &text,
                "SamplerState s1_s : register(s1);",
                "SamplerState s3_s : register(s3);",
            )?;
            text = replace_once(&text, "t0.Sample(s1_s, v3.xy)", "t0.Sample(s2_s, v3.xy)")?;
            text = replace_once(&text, "t2.Sample(s2_s, v3.xy)", "t2.Sample(s3_s, v3.xy)")?;
            text = format!(
                "Texture2D<float4> t6 : register(t6);\nSamplerState s4_s : register(s4);\n{text}"
            );
            let replacement = if simple {
                format!(
                    "float mask_value = saturate(cb0[5].x * cb0[4].x);\n  mask_value = saturate(cb7[{dye}].y * mask_value + cb7[{dye}].x);\n  mask_value = cb7[{dye}].w * mask_value + cb7[{dye}].z;\n  r1.x = r2.x * mask_value;\n  r1.xz = saturate(r1.xx);"
                )
            } else {
                "float mask_value = t6.Sample(s4_s, source_uv * cb0[5].xy + cb0[5].zw).x;\n  mask_value = mask_value * cb0[6].x + cb0[6].y;\n  mask_value = (1 - r2.x) * mask_value;\n  mask_value = mask_value * cb0[7].x + cb0[7].y;\n  r1.x = saturate(cb0[4].y * r2.x + cb0[4].x) * mask_value;\n  r1.xz = saturate(r1.xx);".into()
            };
            text = replace_once(
                &text,
                "r1.x = cb0[4].y * r2.x + cb0[4].x;\n  r1.xz = saturate(r1.xx);",
                &replacement,
            )?;
        }
        26 => {
            let start = "  r1.xy = v3.xy * cb0[2].xy + cb0[2].zw;";
            let end = "  r1.xz = saturate(r1.xx);";
            ensure!(
                text.matches(start).count() == 1 && text.matches(end).count() == 1,
                "native rough decal mask block differs"
            );
            let a = text.find(start).unwrap();
            let b = text.find(end).unwrap() + end.len();
            ensure!(a < b, "native decal block order differs");
            let replacement = if simple {
                format!(
                    "  r1.xy = source_uv * cb0[4].xy + cb0[4].zw;\n  r1.x = t6.Sample(s4_s, r1.xy).x;\n  r1.x = saturate(r1.x * cb0[5].x + cb0[5].y);\n  r1.x = saturate(cb7[{dye}].y * r1.x + cb7[{dye}].x);\n  r1.x = cb7[{dye}].w * r1.x + cb7[{dye}].z;\n  r1.x = r1.z * r1.x;\n  r1.xz = saturate(r1.xx);"
                )
            } else {
                "  r1.xy = source_uv * cb0[5].xy + cb0[5].zw;\n  r1.x = t6.Sample(s4_s, r1.xy).x;\n  r1.x = r1.x * cb0[6].x + cb0[6].y;\n  r1.x = (1 - r1.z) * r1.x;\n  r1.x = r1.x * cb0[7].x + cb0[7].y;\n  r1.y = saturate(cb0[4].y * r1.z + cb0[4].x);\n  r1.x = r1.y * r1.x;\n  r1.xz = saturate(r1.xx);".into()
            };
            text.replace_range(a..b, &replacement);
            text = replace_once(
                &text,
                "r1.zw = v3.xy * cb7[0].xy + cb7[0].zw;",
                "r1.zw = source_uv * cb7[0].xy + cb7[0].zw;",
            )?;
        }
        _ => bail!("unaudited decal blend"),
    }
    Ok(clamp_samples(text, &[0, 2], rect, size))
}

pub fn reflection(text: &str, channel: u8, rect: [usize; 4], size: [usize; 2]) -> Result<String> {
    ensure!(channel <= 5, "source dye selector outside native bank");
    let mut text = replace_once(text, "  r0.xyz = cb0[19].xyz * r0.xyz;\n", "")?;
    text = text.replace("cb0[2]", "float4(0,0,0,0)");
    text = replace_once(&text, "float4 cb0[20];", "float4 cb0[22];")?;
    let mut mapped = String::new();
    let mut tail = text.as_str();
    while let Some(at) = tail.find("cb0[") {
        mapped.push_str(&tail[..at]);
        tail = &tail[at + 4..];
        let end = tail.find(']').context("unterminated reflection constant")?;
        let index = match tail[..end].parse::<usize>()? {
            1 => 4,
            3 => 5,
            7 => 10,
            8 => 11,
            10 => 13,
            11 => 14,
            12 => 15,
            13 => 16,
            14 => 17,
            15 => 18,
            16 => 19,
            17 => 20,
            18 => 21,
            22 => 22,
            _ => bail!("unmapped native reflection constant"),
        };
        mapped.push_str(&format!("cb0[{index}]"));
        tail = &tail[end + 1..];
    }
    mapped.push_str(tail);
    text = replace_once(&mapped, "float4 cb12[8];", "float4 cb12[13];")?;
    text = format!(
        "Texture2D<float4> t16 : register(t16);\nTexture3D<float4> t17 : register(t17);\nSamplerState s7_s : register(s7);\ncbuffer cb8 : register(b8) {{ float4 cb8[8]; }}\n{text}"
    );
    text = replace_once(
        &text,
        "uint4 bitmask, uiDest;",
        &format!("uint4 bitmask, uiDest;\n  {}", source_uv(rect, size)),
    )?;
    let marker = "r1.x = cmp(0.000000 != cb0[21].x);";
    ensure!(
        text.matches(marker).count() == 1,
        "native dither block differs"
    );
    let at = text.find(marker).unwrap();
    text = format!(
        "{}{}",
        &text[..at],
        text[at..].replace("v3.xy", "source_uv")
    );
    text = replace_once(
        &text,
        "  o0.xyz = r0.xyz * r0.www;\n  o0.w = cb0[18].x * r0.w;",
        "  float2 fog_uv = cb12[12].zw * v5.xy;\n  float fog_depth = sqrt(min(1.0, length(cb12[7].xyz - v4.xyz) * 0.015625));\n  float fog_weight = t17.SampleLevel(s7_s, float3(fog_uv, fog_depth), 0).x;\n  float fog_alpha = t16.SampleLevel(s7_s, fog_uv, 0).w;\n  float attenuation = 1.0 + fog_weight * (fog_alpha - 1.0);\n  float lighting = dot(cb8[7].xyz, float3(0.300000012,0.589999974,0.109999999));\n  float opacity = r0.w * attenuation * lighting;\n  o0.xyz = r0.xyz * opacity;\n  o0.w = saturate(cb0[18].x) * opacity;",
    )?;
    let c = channel % 2;
    text = text.replace("float4(0,0,0,0)", &format!("float4({c},{c},{c},{c})"));
    Ok(clamp_samples(text, &[0, 1, 2], rect, size))
}
