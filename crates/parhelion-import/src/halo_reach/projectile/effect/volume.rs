//! Source light volumes (`ltvl`) as native straight beams of camera-facing profiles.
//!
//! MCC U13 draws each light volume of a light volume system (0x1B4-byte rows at `+0x0`) as
//! `count` profiles. Profile `i` sits at `origin + direction * (offset + i * distance)` with
//! percentile `i / (count - 1)`, where its thickness, color, alpha and intensity are evaluated.
//! Each profile is a camera-facing quad `lerp(thickness, profile length, sin)` long along the
//! projected direction and `thickness` wide, where `sin` is the sine of the angle between the
//! direction and the view. Its color is the profile color times intensity, and its alpha the
//! profile alpha times intensity, `lerp(brightness ratio, 1, sin)` and the reciprocal of the
//! expected overdraw `min(count, length / (distance * sin))`. The circular albedo shades the quad
//! with `pow(saturate(c - c * |2 uv - 1|^2), falloff)` from the template's `center_offset` and
//! `falloff`, and an additive mode adds color times alpha. These are the contracts of the HREK
//! `light_volume` render and update sources.
//!
//! Native geometry selector 5 expands one particle into a straight line of records, record `i`
//! at `position + direction * i * length / (count - 1)` with its normalized distance in route
//! 35, and draws each record as an independent camera-facing quad. The Fallen bolt streak
//! `80FF021F` is such a beam in the projectile's own frame. Its definition `80C1BEE3` keeps both
//! transform rows on the emitter, so the beam follows the projectile, and starts it along local X,
//! the axis native movement aligns with velocity. A private copy places the source profiles on
//! local +X from the source offset, one record per profile, and a private material evaluates the
//! source functions per record in its vertex program and the circular albedo in its pixel program.
//!
//! A flight beam keeps emitting short particles rather than holding one for the whole flight. Its
//! event ends when the projectile attaches or retires, and a stopped native system keeps drawing
//! its particles until their life ends, so a single long-lived particle stayed on a stuck needle
//! for the rest of its life. The stock streak stores one burst and no rate. Each beam here lives
//! [`BEAM_LIFE`] and is emitted [`BEAM_OVERLAP`] times per life, every particle weighted by
//! `(1 - cos(2 pi age)) / BEAM_OVERLAP` of its normalized age. Evenly spaced weights sum to one, so
//! the beam keeps its brightness while it emits and is gone `BEAM_LIFE` after its event ends.
//! Selector 5 copies the particle's appearance record, whose row 1 W holds that age (route 0),
//! into every beam record.
use super::*;
use crate::{
    halo_reach::{effect::property::Property, native::material::program},
    tiger::particle::Program,
};

const SYSTEM: u32 = 0x80ff_021f;
const SYSTEM_CLASS: u32 = 0x8080_6e28;
const DEFINITION_CLASS: u32 = 0x8080_6e2c;
const MATERIAL_CLASS: u32 = 0x8080_71e8;
const STAGES: [usize; 6] = [0x48, 0xe8, 0x188, 0x228, 0x2c8, 0x368];
/// Material byte `+0x22` selecting native rasterizer row 1, solid with no culling, as Reach
/// effect billboards draw.
const BOTH_FACES: u8 = 0x81;
/// The native beam's record ceiling, `clamp(ceil(route 34 * definition +0x11C), 1, 96)`.
const RECORDS: i32 = 96;
const ROW: usize = 0x1b4;
/// Light volume state input 0, the profile's percentile.
const PERCENTILE: u8 = 0;
/// Seconds each particle of a flight beam lives, which bounds how long a beam outlasts its event.
pub(super) const BEAM_LIFE: f32 = 0.25;
/// Particles of a flight beam alive at once. At 60 frames per second with uneven frame times, six
/// keep the summed brightness within a few percent while a beam emits.
pub(super) const BEAM_OVERLAP: f32 = 6.;

/// A source light volume in metres, with its per-profile functions as shader expressions of the
/// percentile `p`.
pub(super) struct Volume {
    pub name: String,
    count: i32,
    start: f32,
    spacing: f32,
    profile_length: f32,
    brightness: f32,
    center: f32,
    falloff: f32,
    thickness: String,
    color: String,
    alpha: String,
    intensity: String,
    limits: Vec<String>,
}

/// A fixed property's value, or why it cannot be one.
fn fixed(cache: &Cache, at: usize, name: &str) -> Result<f32> {
    let value = Property::read(cache, at)?
        .constant()
        .with_context(|| format!("Light volume {name} varies"))?;
    ensure!(value.is_finite(), "Light volume {name} is not finite");
    Ok(value)
}

/// A property's shader expression of the percentile, as MCC's light volume update evaluates it.
/// Other light volume inputs, such as system age or random, have no producer on the native records.
fn curve(cache: &Cache, at: usize, name: &str) -> Result<String> {
    Property::read(cache, at)?
        .evaluated_with(&|index| {
            ensure!(
                index == PERCENTILE,
                "Light volume {name} reads source input {index}, which the native records do not carry"
            );
            Ok("p".into())
        })
        .with_context(|| format!("Light volume {name}"))
}

/// Every light volume of source system `tag`, each read on its own with its name, or the reason
/// it cannot be drawn.
pub(super) fn read(
    cache: &Cache,
    tag: &Tag,
    metres: f32,
) -> Result<Vec<(String, std::result::Result<Volume, String>)>> {
    ensure!(tag.group == "ltvl", "Expected a source light volume system");
    cache
        .block(tag.address()?, ROW)?
        .into_iter()
        .map(|at| {
            Ok((
                cache.string_id(cache.u32(at)?)?,
                volume(cache, tag, at, metres).map_err(|error| format!("{error:#}")),
            ))
        })
        .collect()
}

fn volume(cache: &Cache, tag: &Tag, at: usize, metres: f32) -> Result<Volume> {
    let material = crate::halo_reach::material::embedded(cache, tag.clone(), at + 4)?;
    let option = |name: &str| material.options.get(name).map(String::as_str);
    ensure!(
        option("albedo") == Some("circular"),
        "A light volume whose albedo samples its base map needs a texture binding"
    );
    ensure!(
        matches!(
            option("blend_mode"),
            Some("additive" | "add_src_times_srcalpha")
        ),
        "Light volume blend mode {} has no additive counterpart",
        option("blend_mode").unwrap_or("none")
    );
    ensure!(
        option("depth_fade").is_none_or(|mode| mode == "off"),
        "A depth-faded light volume needs the scene depth"
    );
    let constant = |name: &str| {
        material
            .constant(name)
            .map(|value| value[0])
            .filter(|value| value.is_finite())
            .with_context(|| format!("Light volume material has no {name}"))
    };
    let (center, falloff) = (constant("center_offset")?, constant("falloff")?);
    ensure!(falloff > 0., "Light volume falloff is not positive");
    ensure!(
        cache.u16(at + 0x68)? == 0,
        "Light volume appearance flags need translation"
    );
    let brightness = cache.f32(at + 0x6c)?;
    ensure!(
        brightness.is_finite() && brightness >= 0.,
        "Light volume brightness ratio is invalid"
    );
    let offset = fixed(cache, at + 0x8c + 0x20, "offset")?;
    let density = fixed(cache, at + 0x8c + 0x40, "profile density")?;
    let profile_length = fixed(cache, at + 0x8c + 0x60, "profile length")?;
    // The source's own profile count, runtime field `+0x194`, which is the whole part of length
    // times density in the inspected volumes.
    let count = cache.i32(at + 0x194)?;
    ensure!(
        (2..=RECORDS).contains(&count),
        "Light volume profile count {count} is outside the native beam's 2 to {RECORDS} records"
    );
    ensure!(
        density > 0. && profile_length > 0.,
        "Light volume profile spacing is not positive"
    );
    let mut limits = vec![
        "Profiles are spaced at the reciprocal of the source profile density, from the source offset along the projectile's local +X axis.".into(),
        "Light volume inputs other than the profile percentile are not carried, and the source LOD fade is not evaluated.".into(),
    ];
    if option("fog") == Some("on") {
        limits.push("Atmospheric extinction of the source profiles is not applied.".into());
    }
    let flags = cache.u32(at + 0x70)?;
    if flags != 0 {
        limits.push(format!(
            "Light volume flags {flags:#x} are recorded and not translated."
        ));
    }
    Ok(Volume {
        name: cache.string_id(cache.u32(at)?)?,
        count,
        start: offset * metres,
        spacing: metres / density,
        profile_length: profile_length * metres,
        brightness,
        center,
        falloff,
        thickness: curve(cache, at + 0x8c + 0x80, "thickness")?,
        color: curve(cache, at + 0x8c + 0xa0, "color")?,
        alpha: curve(cache, at + 0x8c + 0xc0, "alpha")?,
        intensity: curve(cache, at + 0x8c + 0xe0, "intensity")?,
        limits,
    })
}

const VERTEX: &str = r#"
cbuffer Object : register(b11) { float4 objectData[4]; };
cbuffer View : register(b12) { float4 viewData[8]; };
static const float2 coordinates[4]={float2(0,0),float2(0,1),float2(1,0),float2(1,1)};
void main(float4 a0:TEXCOORD0,float4 a1:TEXCOORD1,float4 a2:TEXCOORD2,uint id:SV_VertexID,
out float4 uv:TEXCOORD0,out float4 color:TEXCOORD1,out float4 position:SV_POSITION0) {
float p=saturate(a0.w);
float3 center=objectData[0].xyz*a1.x+objectData[1].xyz*a1.y+objectData[2].xyz*a1.z+objectData[3].xyz;
float3 axis=objectData[0].xyz*a0.x+objectData[1].xyz*a0.y+objectData[2].xyz*a0.z;
float axisLength=length(axis);
axis=axisLength>0.000001?axis/axisLength:float3(1,0,0);
float3 view=center-viewData[7].xyz;
float viewLength=length(view);
view=viewLength>0.000001?view/viewLength:axis;
float3 side=cross(view,axis);
float sine=length(side);
if(sine>0.000001) side/=sine;
else side=normalize(cross(view,abs(view.z)<0.9?float3(0,0,1):float3(1,0,0)));
float3 along=cross(view,side);
float thickness=max(($THICKNESS$)*$METRES$,0.000001);
float profileLength=lerp(thickness,$PROFILE_LENGTH$,sine);
float2 coordinate=coordinates[id%4];
float2 offset=(coordinate-0.5)*float2(profileLength,thickness);
float3 world=center+along*offset.x+side*offset.y;
position=viewData[0]*world.x+viewData[1]*world.y+viewData[2]*world.z+viewData[3];
float overdraw=max(min($COUNT$,profileLength/max($SPACING$*sine,0.000001)),0.000001);
float intensity=$INTENSITY$;
float alpha=($ALPHA$)*intensity*lerp($BRIGHTNESS$,1.0,sine)/overdraw;
float weight=(1.0-cos(6.28318531*saturate(a1.w)))/$OVERLAP$;
color=float4(($COLOR$)*intensity*alpha*weight,0);
uv=float4(coordinate,0,0);
if(a1.w>=1.0) position=float4(10000,10000,-10000,1);
}
"#;

const PIXEL: &str = r#"
cbuffer Effect : register(b13) { float4 effectData[2]; };
float4 main(float4 uv:TEXCOORD0,float4 color:TEXCOORD1,float4 position:SV_POSITION0):SV_TARGET0 {
float2 delta=2.0*uv.xy-1.0;
float glow=pow(saturate($CENTER$-$CENTER$*dot(delta,delta)),$FALLOFF$);
return float4(color.rgb*glow*effectData[1].z*effectData[1].w,0.0);
}
"#;

/// The beam definition: the stock streak's routes with its defaults, apart from life (route 5) and
/// rate (route 10) for a re-emitted beam, beam length (route 33) and the record count factor
/// (route 34, times definition `+0x11C`, set to one), and a spawn section placing the beam on
/// local +X from `start`. The stock burst (route 16) starts the first particle with the flight.
fn definition(native: &mut Reader, volume: &Volume) -> Result<(u32, Payload)> {
    let tag = native.tag(SYSTEM, Some(SYSTEM_CLASS))?.u32(0)?;
    let stock = (*native.tag(tag, Some(DEFINITION_CLASS))?).clone();
    let route = |index: usize| -> Result<(u8, u8)> {
        Ok((stock.u8(0x80 + index * 2)?, stock.u8(0x81 + index * 2)?))
    };
    ensure!(
        stock.u8(0x14b)? == 5
            && stock.u8(0x141)? == 1
            && stock.u8(0x142)? == 3
            && stock.u8(0x40)? == 0
            && stock.u8(0x41)? == 0
            && route(0)? == (1, 7)
            && route(6)? == (1, 4)
            && route(7)? == (1, 0)
            && route(35)? == (1, 3)
            && route(33)?.0 == 6
            && route(34)?.0 == 6
            && route(5)?.0 == 6
            && route(10)?.0 == 6
            && route(16)?.0 == 6,
        "Fallen bolt streak beam definition differs"
    );
    let defaults = stock
        .array(8, 16, Some(0x8080_0090))?
        .into_iter()
        .map(|row| {
            Ok([
                stock.f32(row)?,
                stock.f32(row + 4)?,
                stock.f32(row + 8)?,
                stock.f32(row + 12)?,
            ])
        })
        .collect::<Result<Vec<_>>>()?;
    let length = volume.spacing * (volume.count - 1) as f32;
    let mut program = Program::default();
    for index in 0..56 {
        let (bank, scalar) = route(index)?;
        match (index, bank) {
            (_, 0xff) => {}
            (5, _) => program.default_value(index, [BEAM_LIFE; 4])?,
            (10, _) => program.default_value(index, [BEAM_OVERLAP / BEAM_LIFE; 4])?,
            (33, _) => program.default_value(index, [length; 4])?,
            (34, _) => program.default_value(index, [volume.count as f32; 4])?,
            (_, 6) => {
                let row = defaults
                    .get(usize::from(scalar) / 4)
                    .context("Fallen bolt streak default")?;
                program.default_value(index, [row[usize::from(scalar) % 4]; 4])?;
            }
            _ => program.route(index, bank, scalar)?,
        }
    }
    // Direction into record row 0 and the beam's start into row 1, XYZ.
    program.constant(3, [1., 0., 0., 0.])?;
    program.code(3, &[0x3f, 1, 0, 1])?;
    program.constant(3, [volume.start, 0., 0., 0.])?;
    program.code(3, &[0x3f, 1, 1, 1])?;
    let mut payload = program.write(&stock, 1, 3)?;
    put(&mut payload.0, 0x11c, &1f32.to_le_bytes())?;
    put(&mut payload.0, 0x120, &(BEAM_LIFE + 0.05).to_le_bytes())?;
    Ok((tag, payload))
}

/// A private copy of the stock streak's material whose only programs are the light volume's.
fn material(
    native: &mut Reader,
    graph: &mut Graph,
    volume: &Volume,
    prefix: &str,
    metres: f32,
) -> Result<String> {
    let tag = native.tag(SYSTEM, Some(SYSTEM_CLASS))?.u32(0x14)?;
    let shell = (*native.tag(tag, Some(MATERIAL_CLASS))?).clone();
    ensure!(
        shell.u8(0x20)? == 0x88 && shell.u32(0x48)? != u32::MAX && shell.u32(0x2c8)? != u32::MAX,
        "Fallen bolt streak material composition differs"
    );
    let vertex = VERTEX
        .replace("$THICKNESS$", &volume.thickness)
        .replace("$COLOR$", &volume.color)
        .replace("$ALPHA$", &volume.alpha)
        .replace("$INTENSITY$", &volume.intensity)
        .replace("$METRES$", &format!("{metres:.9}"))
        .replace("$PROFILE_LENGTH$", &format!("{:.9}", volume.profile_length))
        .replace("$SPACING$", &format!("{:.9}", volume.spacing))
        .replace("$COUNT$", &format!("{:.1}", volume.count as f32))
        .replace("$BRIGHTNESS$", &format!("{:.9}", volume.brightness))
        .replace("$OVERLAP$", &format!("{BEAM_OVERLAP:.1}"));
    let pixel = PIXEL
        .replace("$CENTER$", &format!("{:.9}", volume.center))
        .replace("$FALLOFF$", &format!("{:.9}", volume.falloff));
    let (vertex_symbol, pixel_symbol) = (format!("{prefix}-vertex"), format!("{prefix}-pixel"));
    program(
        native,
        graph,
        &vertex_symbol,
        shell.u32(0x48)?,
        &vertex,
        true,
    )?;
    program(
        native,
        graph,
        &pixel_symbol,
        shell.u32(0x2c8)?,
        &pixel,
        false,
    )?;
    let mut bytes = shell.0.clone();
    bytes[0x22] = BOTH_FACES;
    // The programs read the object, view and effect scopes directly, so no stage keeps the
    // shell's expressions, constants, textures or samplers.
    for stage in STAGES {
        bytes[stage..stage + 0xa0].fill(0);
        for at in [stage, stage + 0x80, stage + 0x84] {
            put(&mut bytes, at, &u32::MAX.to_le_bytes())?;
        }
    }
    let symbol = format!("{prefix}-material");
    graph.add(
        &symbol,
        tag,
        &bytes,
        None,
        vec![
            json!({"offset":0x48,"symbol":vertex_symbol}),
            json!({"offset":0x2c8,"symbol":pixel_symbol}),
        ],
    )?;
    Ok(symbol)
}

/// The native beam system drawing `volume`, its graph nodes named by `prefix`, and its report.
pub(super) fn emit(
    native: &mut Reader,
    graph: &mut Graph,
    volume: &Volume,
    prefix: &str,
    metres: f32,
) -> Result<(String, Value)> {
    let (definition_tag, definition) = self::definition(native, volume)?;
    let material = self::material(native, graph, volume, prefix, metres)?;
    let definition_symbol = format!("{prefix}-definition");
    graph.add(
        &definition_symbol,
        definition_tag,
        &definition.0,
        None,
        vec![],
    )?;
    let mut bytes = (*native.tag(SYSTEM, Some(SYSTEM_CLASS))?).clone();
    for at in [4, 8, 0x10] {
        ensure!(
            bytes.u32(at)? == u32::MAX,
            "Fallen bolt streak system has a computational stage"
        );
    }
    put(&mut bytes.0, 0, &u32::MAX.to_le_bytes())?;
    put(&mut bytes.0, 0x14, &u32::MAX.to_le_bytes())?;
    let symbol = format!("{prefix}-system");
    graph.add(
        &symbol,
        SYSTEM,
        &bytes.0,
        None,
        vec![
            json!({"offset":0,"symbol":definition_symbol}),
            json!({"offset":0x14,"symbol":material}),
        ],
    )?;
    Ok((
        symbol,
        json!({"name":volume.name,"donor_system":format!("{SYSTEM:08X}"),
            "profiles":volume.count,"start_metres":volume.start,"spacing_metres":volume.spacing,
            "profile_length_metres":volume.profile_length,"brightness_ratio":volume.brightness,
            "center_offset":volume.center,"falloff":volume.falloff,
            "particle_life_seconds":BEAM_LIFE,"overlapping_particles":BEAM_OVERLAP,
            "limits":volume.limits,"gameplay_verified":false}),
    ))
}
