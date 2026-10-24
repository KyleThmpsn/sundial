//! Eight-row native CPU billboards with explicit source atlas and alpha handling.
use super::*;
use crate::presentation::append;

pub(super) fn pending(material: &Material) -> Option<&'static str> {
    let option = |name: &str| material.options.get(name).map(String::as_str);
    if !matches!(
        option("albedo"),
        Some(
            "diffuse_only"
                | "diffuse_plus_billboard_alpha"
                | "diffuse_plus_sprite_alpha"
                | "palettized"
                | "palettized_plus_billboard_alpha"
                | "palettized_plus_sprite_alpha"
        )
    ) {
        return Some("Plasma or modulated particle albedo");
    }
    if !matches!(
        option("blend_mode"),
        Some("alpha_blend" | "pre_multiplied_alpha" | "additive" | "add_src_times_srcalpha")
    ) {
        return Some("Another particle composition path");
    }
    if option("specialized_rendering") != Some("none") || option("warp") != Some("none") {
        return Some("Particle warp or specialized rendering");
    }
    if material.properties.len() != 1 || !material.properties[0].functions.is_empty() {
        return Some("Animated particle material parameters");
    }
    if !matches!(option("self_illumination"), Some("none" | "constant_color")) {
        return Some("Another particle self-illumination path");
    }
    None
}

pub(super) fn validate(material: &Material) -> Result<()> {
    ensure!(
        pending(material).is_none(),
        "Source particle material requires another adapter"
    );
    ensure!(
        material.mapping(&["base_map"]).is_some(),
        "Source particle has no base map"
    );
    Ok(())
}

const VERTEX: &str = r#"
cbuffer Object : register(b11) { float4 objectData[4]; };
cbuffer View : register(b12) { float4 viewData[8]; };
static const float2 corners[4]={float2(-1,1),float2(-1,-1),float2(1,1),float2(1,-1)};
static const float2 coordinates[4]={float2(0,0),float2(0,1),float2(1,0),float2(1,1)};
void main(float4 a0:TEXCOORD0,float4 a1:TEXCOORD1,float4 a2:TEXCOORD2,float4 a3:TEXCOORD3,
float4 a4:TEXCOORD4,float4 a5:TEXCOORD5,float4 a6:TEXCOORD6,float4 a7:TEXCOORD7,uint id:SV_VertexID,
out float4 uv:TEXCOORD0,out float4 color:TEXCOORD1,out float4 property:TEXCOORD2,
out float4 animation:TEXCOORD3,out float4 random:TEXCOORD4,out float4 position:SV_POSITION0) {
float3 center=objectData[0].xyz*a2.x+objectData[1].xyz*a2.y+objectData[2].xyz*a2.z+objectData[3].xyz;
float3 eye=viewData[7].xyz-center;
float3 up=normalize(viewData[4].xyz);
float3 right=cross(eye,up);
float rightLength=dot(right,right);
right=rightLength>0.000001?right*rsqrt(rightLength):float3(1,0,0);
float rotation=a2.w+$RANDOM_ROTATION$;
float sine,cosine;sincos(rotation,sine,cosine);
float2 corner=(corners[id%4]-float2($CENTER_X$,$CENTER_Y$))*a3.xy;
float2 rotated=float2(corner.x*cosine-corner.y*sine,corner.x*sine+corner.y*cosine);
float3 world=center+right*rotated.x+up*rotated.y;
position=viewData[0]*world.x+viewData[1]*world.y+viewData[2]*world.z+viewData[3];
float2 sourceUv=coordinates[id%4];
$MIRROR$
uv=float4(sourceUv,length(eye),a1.w);
color=a0;property=a3;animation=a4;random=a7;
if(a1.w>=1.0 || max(abs(a3.x),abs(a3.y))<0.000001) position=float4(10000,10000,-10000,1);
}
"#;

const PIXEL: &str = r#"
Texture2D<float4> Map:register(t0);
Texture2D<float4> Palette:register(t1);
Texture2D<float4> Alpha:register(t2);
SamplerState SurfaceSampler:register(s1);
cbuffer Effect:register(b13) { float4 effectData[2]; };
cbuffer Frame:register(b0) { float4 frameData; };
static const float4 frames[$FRAME_COUNT$]={$FRAMES$};
float4 sampleFrame(float2 sourceUv,float2 billboardUv,float paletteValue) {
float2 uv=sourceUv*float2($MAP_U$,$MAP_V$)+float2($MAP_X$,$MAP_Y$);
$SAMPLE$
return texel;
}
float4 main(float4 uv:TEXCOORD0,float4 color:TEXCOORD1,float4 property:TEXCOORD2,
float4 animation:TEXCOORD3,float4 random:TEXCOORD4,float4 position:SV_POSITION0):SV_TARGET0 {
float frame=animation.x+frac(animation.w*animation.z);
frame=$FRAME_POLICY$;
float coordinate=frame*$FRAME_SPAN$;
$BACKWARDS$
coordinate-=floor(coordinate/$FRAME_COUNT$)*$FRAME_COUNT$;
uint first=min((uint)floor(coordinate),$LAST_FRAME$);
uint second=$NEXT_FRAME$;
float weight=$FRAME_BLEND$;
float4 a=frames[first],b=frames[second];
float2 scroll=float2($SCROLL_U$,$SCROLL_V$)*frameData.x;
float scaleA=$SCALE_A$,scaleB=$SCALE_B$;
float2 uvA=(uv.xy-0.5)*a.zw*scaleA+0.5*a.zw+a.xy+scroll*abs(a.zw);
float2 uvB=(uv.xy-0.5)*b.zw*scaleB+0.5*b.zw+b.xy+scroll*abs(b.zw);
float4 texel=lerp(sampleFrame(uvA,uv.xy,animation.y),sampleFrame(uvB,uv.xy,animation.y),weight);
$BLACK_POINT$
float opacity=saturate(texel.a*color.a);
float intensity=max(property.w,0);
$INTENSITY_ALPHA$
float visibility=saturate((uv.z-$NEAR_CUTOFF$)*$NEAR_SCALE$);
opacity*=visibility;
float3 rgb=texel.rgb*color.rgb*intensity+$SELF_ILLUM$;
$OUTPUT$
}
"#;

fn vertex(emitter: &Emitter) -> String {
    let mut vertex = VERTEX.to_owned();
    let mut mirror = String::new();
    if emitter.appearance & 1 != 0 {
        mirror.push_str("if(a7.y<0.5)sourceUv.x=1-sourceUv.x;\n");
    }
    if emitter.appearance & 2 != 0 {
        mirror.push_str("if(a7.z<0.5)sourceUv.y=1-sourceUv.y;\n");
    }
    for (key, value) in [
        ("$MIRROR$", mirror),
        (
            "$RANDOM_ROTATION$",
            format!(
                "{}+{}",
                if emitter.appearance & 4 != 0 {
                    "a7.w*6.28318530718"
                } else {
                    "0.0"
                },
                if emitter.appearance & 0x20 != 0 {
                    "1.57079632679"
                } else {
                    "0.0"
                }
            ),
        ),
        ("$CENTER_X$", format!("{:.9}", emitter.center[0])),
        ("$CENTER_Y$", format!("{:.9}", emitter.center[1])),
    ] {
        vertex = vertex.replace(key, &value);
    }
    vertex
}

fn pixel(emitter: &Emitter, sample: String) -> Result<String> {
    let material = &emitter.material;
    let map = material
        .mapping(&["base_map"])
        .context("Particle base map")?;
    let count = emitter.frames.len();
    let frames = emitter
        .frames
        .iter()
        .map(|v| format!("float4({:.9},{:.9},{:.9},{:.9})", v[0], v[1], v[2], v[3]))
        .collect::<Vec<_>>()
        .join(",");
    let blend = material.options["blend_mode"].as_str();
    let output = match blend {
        "additive" => "return float4(rgb*color.a*visibility*effectData[1].z*effectData[1].w,0);",
        "add_src_times_srcalpha" => "return float4(rgb*opacity*effectData[1].z*effectData[1].w,0);",
        "pre_multiplied_alpha" => "return float4(rgb*color.a*visibility,opacity);",
        "alpha_blend" => "return float4(rgb*opacity,opacity);",
        _ => anyhow::bail!("Unsupported particle blend mode"),
    };
    let self_illum = material.constant("self_illum_color").unwrap_or([0.; 4]);
    let frame_blend = material
        .options
        .get("frame_blend")
        .is_some_and(|v| v == "on");
    let start_scale = material.constant("starting_uv_scale").map_or(1., |v| v[0]);
    let end_scale = material.constant("ending_uv_scale").map_or(1., |v| v[0]);
    ensure!(
        start_scale.is_finite() && end_scale.is_finite() && start_scale > 0. && end_scale > 0.,
        "Particle animation UV scale is invalid"
    );
    ensure!(
        emitter
            .center
            .iter()
            .chain(&emitter.scroll)
            .all(|v| v.is_finite()),
        "Nonfinite particle coordinates"
    );
    let mut pixel = PIXEL.to_owned();
    for (key,value) in [
        ("$FRAME_COUNT$",count.to_string()),("$LAST_FRAME$",(count-1).to_string()),("$FRAMES$",frames),
        ("$FRAME_SPAN$",if emitter.animation&1!=0 {count-1}else{count}.to_string()),
        ("$FRAME_POLICY$","frame".into()),
        ("$NEXT_FRAME$",if emitter.animation&1!=0 {format!("min(first+1,{})",count-1)}else{format!("(first+1)%{count}")}),
        ("$FRAME_BLEND$",if frame_blend {"frac(coordinate)"}else{"0.0"}.into()),
        ("$BACKWARDS$",if emitter.animation&2!=0 {
            format!("if(((uint)(random.z*256.0)&1)!=0)coordinate={}-coordinate;",if emitter.animation&1!=0 {count-1}else{count})
        }else{String::new()}),
        ("$SCROLL_U$",format!("{:.9}",emitter.scroll[0])),("$SCROLL_V$",format!("{:.9}",emitter.scroll[1])),
        ("$SCALE_A$",if frame_blend {format!("rcp(lerp({start_scale:.9},{end_scale:.9},(1.0+weight)*0.5))")}else{"1.0".into()}),
        ("$SCALE_B$",if frame_blend {format!("rcp(lerp({start_scale:.9},{end_scale:.9},weight*0.5))")}else{"1.0".into()}),
        ("$MAP_U$",format!("{:.9}",map.transform[0])),("$MAP_V$",format!("{:.9}",map.transform[1])),
        ("$MAP_X$",format!("{:.9}",map.transform[2])),("$MAP_Y$",format!("{:.9}",map.transform[3])),
        ("$NEAR_SCALE$",format!("{:.9}",emitter.near_fade[0])),("$NEAR_CUTOFF$",format!("{:.9}",emitter.near_fade[1])),
        ("$INTENSITY_ALPHA$",if emitter.appearance&0x40!=0 {"opacity*=intensity;"}else{""}.into()),
        ("$BLACK_POINT$",if material.options.get("black_point").is_some_and(|v|v=="on") {
            "float black=saturate(property.z);float middle=(black+1)*0.5;texel.a=middle*saturate((texel.a-black)/max(middle-black,0.000001))+saturate(texel.a-middle);"
        }else{""}.into()),
        ("$SELF_ILLUM$",format!("float3({:.9},{:.9},{:.9})",self_illum[0],self_illum[1],self_illum[2])),
        ("$SAMPLE$",sample),("$OUTPUT$",output.into()),
    ] {pixel=pixel.replace(key,&value);}
    Ok(pixel)
}

pub(super) fn emit(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    emitter: &Emitter,
    prefix: &str,
) -> Result<String> {
    let material = &emitter.material;
    let albedo = material.options["albedo"].as_str();
    let map = material
        .mapping(&["base_map"])
        .context("Particle base map")?;
    let mut textures = vec![(
        0u32,
        native::material::texture(
            cache,
            pages,
            native,
            graph,
            map,
            false,
            !albedo.starts_with("palettized"),
        )?,
    )];
    let mut sample: String = if albedo.starts_with("palettized") {
        let palette = material.mapping(&["palette"]).context("Particle palette")?;
        textures.push((
            1,
            native::material::texture(cache, pages, native, graph, palette, false, true)?,
        ));
        "float index=Map.Sample(SurfaceSampler,uv).r;\nfloat4 texel=Palette.Sample(SurfaceSampler,float2(index,paletteValue));".into()
    } else {
        "float4 texel=Map.Sample(SurfaceSampler,uv);".into()
    };
    if albedo.contains("alpha") {
        let alpha = material
            .mapping(&["alpha_map"])
            .context("Particle alpha map")?;
        textures.push((
            2,
            native::material::texture(cache, pages, native, graph, alpha, false, false)?,
        ));
        let [u, v, x, y] = alpha.transform;
        let coordinates = if albedo.contains("billboard") {
            "billboardUv"
        } else {
            "sourceUv"
        };
        sample.push_str(&format!("\ntexel.a=Alpha.Sample(SurfaceSampler,{coordinates}*float2({u:.9},{v:.9})+float2({x:.9},{y:.9})).a;"));
    }
    let vertex = vertex(emitter);
    let pixel = pixel(emitter, sample)?;
    let template = 0x80fc7e03;
    let shell = native.tag(template, Some(0x808071e8))?;
    ensure!(
        shell.u8(0x20)? == 0x88,
        "Particle material composition contract differs"
    );
    let vertex_name = format!("{prefix}-vertex");
    let pixel_name = format!("{prefix}-pixel");
    native::material::program(native, graph, &vertex_name, shell.u32(0x48)?, &vertex, true)?;
    native::material::program(native, graph, &pixel_name, shell.u32(0x2c8)?, &pixel, false)?;
    let mut bytes = shell.0.clone();
    for stage in [0x48, 0xe8, 0x188, 0x228, 0x2c8, 0x368] {
        bytes[stage..stage + 0xa0].fill(0);
        for at in [stage, stage + 0x80, stage + 0x84] {
            put(&mut bytes, at, &u32::MAX.to_le_bytes())?;
        }
    }
    let samplers = shell.array(0x308, 16, None)?;
    ensure!(
        !samplers.is_empty(),
        "Native particle sampler table is absent"
    );
    let sampler = shell.bytes::<16>(samplers[0])?;
    append(
        &mut bytes,
        0x308,
        u64::from(shell.u32(samplers[0] - 8)?),
        &sampler,
        16,
    )?;
    append(
        &mut bytes,
        0x2e8,
        0x80800009,
        &[0x4c, 0, 0x49, 0x21, 0x3c, 1, 1, 0x43, 0],
        1,
    )?;
    append(&mut bytes, 0x318, 0x80800090, &[0; 16], 16)?;
    bytes[0x33c] = 0x10;
    put(&mut bytes, 0x348, &0u32.to_le_bytes())?;
    let bindings = textures
        .iter()
        .flat_map(|(slot, _)| [slot.to_le_bytes(), u32::MAX.to_le_bytes()].concat())
        .collect::<Vec<_>>();
    append(&mut bytes, 0x2d0, 0x80807211, &bindings, 8)?;
    let mut patches = vec![
        json!({"offset":0x48,"symbol":vertex_name}),
        json!({"offset":0x2c8,"symbol":pixel_name}),
    ];
    for (row, (_, texture)) in Payload(bytes.clone())
        .array(0x2d0, 8, Some(0x80807211))?
        .into_iter()
        .zip(textures)
    {
        patches.push(json!({"offset":row+4,"symbol":texture}));
    }
    let symbol = format!("{prefix}-material");
    graph.add(&symbol, template, &bytes, None, patches)?;
    Ok(symbol)
}
