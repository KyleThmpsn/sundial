//! Source cheap particle emitters (`cpem`) as native CPU billboards.
//!
//! MCC U13 runs these on the GPU. `cheap_particles_spawn` places each particle at a texel of the
//! emitter's position texture, scaled, flattened along forward and turned into the emitter frame
//! with texel X along up, Y along `forward x up` and Z along forward. Its velocity is a texel of
//! the velocity texture turned the same way, moved toward forward by the directionality and
//! scaled by the speed. Life is drawn as an inverse between the inverse bounds, and the type is
//! chosen by the emitter's weights from the global type library (`cptl`).
//! `cheap_particles_update` applies the type's gravity, drag, texture turbulence and depth-buffer
//! collision. `cheap_particles_render` draws a quad as wide as the type's size, rotated at random,
//! facing the screen or laid along the velocity and stretched by speed, with the color and alpha
//! faded over the end of life. Its pixel program multiplies the type's sprite by them and adds the
//! result over the scene scaled by one minus its alpha, so alpha zero is additive.
use super::*;
use crate::halo_reach::material::{Bitmap, Mapping};

/// Texels taken from each emitter texture, evenly spread through it.
const TABLE: usize = 32;
/// Emitter flags this adapter reads: correlated textures, position and velocity textures in the
/// emitter frame, normalized velocity and a random turn about up.
const FLAGS: u16 = 0x1f;
const LOCAL_TEXTURES: u16 = 6;

pub(in crate::halo_reach) struct Cheap {
    pub source: Tag,
    pub kinds: Vec<String>,
    pub life: f32,
    definition: Payload,
    /// One sprite per distinct type texture, in material texture register order.
    sprites: Vec<Mapping>,
    limits: Vec<String>,
}

struct Kind {
    name: String,
    weight: f32,
    row: usize,
}

fn library(cache: &Cache, at: usize) -> Result<Tag> {
    cache
        .reference(at + 0xfc, Some("cptl"))?
        .context("Cheap particle emitter has no type library")
}

/// The emitter's types with their library rows.
fn kinds(cache: &Cache, at: usize) -> Result<Vec<Kind>> {
    let types = cache.block(library(cache, at)?.address()?, 0x50)?;
    let mut kinds = Vec::new();
    for slot in 0..4 {
        let id = cache.u32(at + 0x2c + slot * 8)?;
        let weight = cache.f32(at + 0x30 + slot * 8)?;
        if id == 0 || weight <= 0. {
            continue;
        }
        ensure!(weight.is_finite(), "Cheap particle type weight is invalid");
        let name = cache.string_id(id)?;
        let row = types
            .iter()
            .copied()
            .find(|&row| cache.u32(row).ok() == Some(id))
            .with_context(|| format!("Cheap particle type {name} is absent from its library"))?;
        kinds.push(Kind { name, weight, row });
    }
    ensure!(!kinds.is_empty(), "Cheap particle emitter has no types");
    Ok(kinds)
}

/// The spawn rate's two ends per second. The function is a constant, ranged by a random draw.
fn rate(cache: &Cache, at: usize) -> Result<Option<[f32; 2]>> {
    let size = cache.u32(at + 0x10)? as usize;
    if size != 32 {
        return Ok(None);
    }
    let function = cache.meta(cache.expand(cache.u32(at + 0x1c)?)?, size)?;
    let input = cache.string_id(cache.u32(at + 8)?)?;
    let range = cache.string_id(cache.u32(at + 12)?)?;
    let ranged = function[1] & 1 != 0;
    if function[0] != 1 || !input.is_empty() || (ranged && range != "random") {
        return Ok(None);
    }
    let value = |at: usize| f32::from_le_bytes(function[at..at + 4].try_into().unwrap());
    let rate = [value(4), if ranged { value(8) } else { value(4) }];
    ensure!(
        rate.iter().all(|v| v.is_finite() && *v >= 0.),
        "Cheap particle spawn rate is invalid"
    );
    Ok(Some(rate))
}

/// The signed vectors of an emitter texture.
fn texels(cache: &Cache, pages: &mut Pages, at: usize, role: &str) -> Result<Vec<[f32; 3]>> {
    let tag = cache
        .reference(at, Some("bitm"))?
        .with_context(|| format!("Cheap particle emitter has no {role} texture"))?;
    let texels = Bitmap::read(cache, &tag, 0, false)?
        .stored(cache, pages)?
        .signed()
        .with_context(|| format!("Cheap particle {role} texture"))?;
    ensure!(
        texels.len() >= TABLE,
        "Cheap particle {role} texture is too small"
    );
    let stride = texels.len() / TABLE;
    Ok((0..TABLE)
        .map(|k| {
            let [x, y, z, _] = texels[k * stride + stride / 2];
            // Texel X lies along up, Y along forward x up, which is right, and Z along forward.
            [z, -y, x]
        })
        .collect())
}

/// What a source cheap particle emitter still needs from another adapter.
pub(in crate::halo_reach) fn pending(cache: &Cache, tag: &Tag) -> Result<Vec<String>> {
    ensure!(
        tag.group == "cpem",
        "Source part is not a cheap particle emitter"
    );
    let at = tag.address()?;
    let mut reasons = Vec::new();
    let flags = cache.u16(at)?;
    if flags & !FLAGS != 0 {
        reasons.push("Unknown cheap particle emitter flags".into());
    }
    if flags & LOCAL_TEXTURES != LOCAL_TEXTURES {
        reasons.push("World-space cheap particle emitter textures".into());
    }
    if rate(cache, at)?.is_none() {
        reasons.push("Animated or input-driven cheap particle spawn rate".into());
    }
    for kind in kinds(cache, at)? {
        if !matches!(cache.i32(kind.row + 0x20)?, 0 | 1) {
            reasons.push(format!(
                "Cheap particle type {} has another orientation",
                kind.name
            ));
        }
    }
    Ok(reasons)
}

/// The native system of source cheap particle emitter `tag` for an event part that lasts
/// `duration` seconds in `frame`. Its particles draw their types, so one system holds them all.
pub(in crate::halo_reach) fn read(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    tag: &Tag,
    duration: f32,
    metres: f32,
    frame: &[f32; 16],
) -> Result<Cheap> {
    ensure!(
        pending(cache, tag)?.is_empty(),
        "Source cheap particle emitter requires another adapter"
    );
    let at = tag.address()?;
    let flags = cache.u16(at)?;
    let rate = rate(cache, at)?.context("Cheap particle spawn rate")?;
    let life = cache.floats::<2>(at + 0x4c)?;
    let [scale, flatten] = cache.floats::<2>(at + 0x64)?;
    let positions = texels(cache, pages, at + 0x54, "position")?
        .into_iter()
        .map(|[x, y, z]| {
            [
                x * scale * metres * (1. - flatten),
                y * scale * metres,
                z * scale * metres,
            ]
        })
        .collect::<Vec<_>>();
    let velocities = texels(cache, pages, at + 0x6c, "velocity")?;
    let directionality = cache.floats::<2>(at + 0x7c)?;
    let speed = cache.floats::<2>(at + 0x84)?.map(|v| v * metres);
    let [rotation, modifier] = cache.floats::<2>(at + 0x94)?;
    let [_, offset, darkest, brightest] = cache.floats::<4>(at + 0x9c)?;
    ensure!(
        [
            scale, flatten, rotation, modifier, offset, darkest, brightest
        ]
        .into_iter()
        .chain(life)
        .chain(directionality)
        .chain(speed)
        .all(f32::is_finite),
        "Cheap particle emitter values are invalid"
    );
    // The spawn program decodes each emitter's brightness as a power of two from its exposure,
    // which the engine lights per emitter between the lighting bounds. The scene's light is
    // taken as zero stops, so a self-lit type keeps its authored color.
    let brightness = offset
        .clamp(darkest.min(brightest), darkest.max(brightest))
        .exp2();
    // Particles are spawned at the rate over the part's event, which lasts at least one frame.
    let emitted = rate.map(|v| v * duration.max(1. / 60.));
    let kinds = kinds(cache, at)?;
    let total = kinds.iter().map(|kind| kind.weight).sum::<f32>();
    let bitmaps = cache.block(library(cache, at)?.address()? + 0xc, 0x10)?;
    let (mut names, mut types, mut textures, mut sprites) =
        (Vec::new(), Vec::new(), Vec::<i32>::new(), Vec::new());
    let (mut turbulent, mut colliding) = (false, false);
    for kind in kinds {
        let row = kind.row;
        let [drag, gravity, turbulence, collision] = cache.floats::<4>(row + 4)?;
        let orientation = cache.i32(row + 0x20)?;
        let [alpha, r, g, b] = cache.floats::<4>(row + 0x24)?;
        let [intensity, fade_start, small, large, stretch] = cache.floats::<5>(row + 0x34)?;
        let texture = cache.i32(row + 0x48)?;
        let texture_scale = cache.f32(row + 0x4c)?;
        ensure!(
            [
                drag, gravity, alpha, r, g, b, intensity, fade_start, small, large, stretch
            ]
            .into_iter()
            .chain([texture_scale])
            .all(f32::is_finite),
            "Cheap particle type {} values are invalid",
            kind.name
        );
        // Types that share a library texture share its texture register.
        let sprite = match textures.iter().position(|&known| known == texture) {
            Some(register) => register,
            None => {
                let bitmap = cache
                    .reference(
                        *bitmaps
                            .get(usize::try_from(texture)?)
                            .context("Cheap particle texture index outside its library")?,
                        Some("bitm"),
                    )?
                    .context("Cheap particle type has no sprite")?;
                textures.push(texture);
                sprites.push(Mapping {
                    role: "base_map".into(),
                    bitmap: Some(bitmap),
                    transform: [1., 1., 0., 0.],
                    frame: 0,
                    address: 0,
                    filter: 0,
                    functions: Vec::new(),
                });
                sprites.len() - 1
            }
        };
        turbulent |= turbulence != 0.;
        colliding |= collision > 0.;
        names.push(kind.name);
        types.push(simulation::CheapKind {
            share: kind.weight / total,
            drag,
            gravity: gravity * metres,
            color: [
                r * intensity * brightness,
                g * intensity * brightness,
                b * intensity * brightness,
                alpha,
            ],
            fade_start,
            size: [small, large].map(|v| v * modifier * metres),
            stretch: stretch / metres,
            texture_scale: if orientation == 0 {
                texture_scale
            } else {
                -texture_scale
            },
            sprite: u32::try_from(sprite)?,
        });
    }
    let source = simulation::Cheap {
        count: emitted,
        life,
        positions: &positions,
        velocities: &velocities,
        correlated: flags & 1 != 0,
        directionality,
        speed,
        normalize: flags & 8 != 0,
        turn: flags & 0x10 != 0,
        kinds: &types,
        rotation,
        pull: 0.01 * metres,
    };
    let program = simulation::cheap(&source, frame)?;
    let longest = life[0].max(life[1]);
    let mut limits = vec![
        format!("Native random streams differ from Reach. Each emitter texture is sampled at {TABLE} evenly spaced texels. Distance LOD and subframe spawn offsets are not translated."),
        "The engine's per-emitter lighting is taken as zero stops. Fog is not applied.".into(),
        "Each particle draws a size between its type's bounds. MCC's render program reads one size per type.".into(),
    ];
    if turbulent {
        limits.push("Type texture turbulence is not translated.".into());
    }
    if colliding {
        limits.push("Depth-buffer collisions, bounces and type changes are not translated.".into());
    }
    Ok(Cheap {
        source: tag.clone(),
        kinds: names,
        life: longest,
        definition: definition(native, &program, longest)?,
        sprites,
        limits,
    })
}

const VERTEX: &str = r#"
cbuffer Object : register(b11) { float4 objectData[4]; };
cbuffer View : register(b12) { float4 viewData[8]; };
static const float2 coordinates[4]={float2(0,0),float2(0,1),float2(1,0),float2(1,1)};
void main(float4 a0:TEXCOORD0,float4 a1:TEXCOORD1,float4 a2:TEXCOORD2,float4 a3:TEXCOORD3,
float4 a4:TEXCOORD4,float4 a5:TEXCOORD5,float4 a6:TEXCOORD6,float4 a7:TEXCOORD7,uint id:SV_VertexID,
out float4 uv:TEXCOORD0,out float4 color:TEXCOORD1,out float4 position:SV_POSITION0) {
float3 center=objectData[0].xyz*a2.x+objectData[1].xyz*a2.y+objectData[2].xyz*a2.z+objectData[3].xyz;
float3 eye=viewData[7].xyz-center;
center+=eye*(a3.w/max(length(eye),0.000001));
float3 up=normalize(viewData[4].xyz);
float3 right=cross(eye,up);
float rightLength=dot(right,right);
right=rightLength>0.000001?right*rsqrt(rightLength):float3(1,0,0);
float3 velocity=objectData[0].xyz*a1.x+objectData[1].xyz*a1.y+objectData[2].xyz*a1.z;
float speed=length(velocity);
if(a3.z>0&&speed>0.000001){right=velocity*(max(speed*a3.y,1)/speed);float3 side=cross(eye,right);float sideLength=dot(side,side);if(sideLength>0.000001)up=side*rsqrt(sideLength);}
float sine,cosine;sincos(a2.w,sine,cosine);
float2 coordinate=coordinates[id%4];
float2 corner=coordinate-0.5;
float2 rotated=float2(corner.x*cosine-corner.y*sine,corner.x*sine+corner.y*cosine)*a3.x;
float3 world=center+right*rotated.x+up*rotated.y;
position=viewData[0]*world.x+viewData[1]*world.y+viewData[2]*world.z+viewData[3];
uv=float4(coordinate.x,corner.y*a3.z+0.5,a4.x,0);
color=a0;
if(a1.w>=1.0||abs(a3.x)<0.000001) position=float4(10000,10000,-10000,1);
}
"#;

/// The pixel program for `sprites` textures. Each particle carries its sprite's register in
/// `uv.z`. Every sprite is sampled outside flow control and the particle's own is selected.
fn pixel(sprites: usize) -> String {
    let mut source = String::new();
    for register in 0..sprites {
        source += &format!("Texture2D<float4> Map{register}:register(t{register});\n");
    }
    source += "SamplerState SurfaceSampler:register(s1);\n\
float4 main(float4 uv:TEXCOORD0,float4 color:TEXCOORD1,float4 position:SV_POSITION0):SV_TARGET0 {\n\
float4 texel=Map0.SampleBias(SurfaceSampler,uv.xy,-0.5);\n";
    for register in 1..sprites {
        source += &format!(
            "float4 texel{register}=Map{register}.SampleBias(SurfaceSampler,uv.xy,-0.5);\
if(uv.z>{register}-0.5) texel=texel{register};\n"
        );
    }
    source + "return texel*color;\n}\n"
}

pub(in crate::halo_reach) fn emit(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    cheap: &Cheap,
    prefix: &str,
) -> Result<(String, Value)> {
    let mut textures = Vec::new();
    for (register, sprite) in cheap.sprites.iter().enumerate() {
        textures.push((
            u32::try_from(register)?,
            native::material::texture(cache, pages, native, graph, sprite, false, true)?,
        ));
    }
    let material = material::shell(
        native,
        graph,
        prefix,
        (VERTEX, &pixel(cheap.sprites.len())),
        (textures, &[]),
        None,
    )?;
    let symbol = system(native, graph, prefix, &cheap.definition, &material)?;
    let sprites = cheap
        .sprites
        .iter()
        .map(|sprite| &sprite.bitmap)
        .collect::<Vec<_>>();
    Ok((
        symbol,
        json!({"source":cheap.source,"cheap_particle_types":cheap.kinds,"sprites":sprites,
            "maximum_life":cheap.life,"limits":cheap.limits,"gameplay_verified":false}),
    ))
}
