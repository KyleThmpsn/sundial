//! Source particle models on the native CPU model-particle path.
//!
//! A Reach `prt3` whose `+0x1BC` names a particle model (`pmdf`) draws that mesh at every particle
//! in place of a sprite. MCC U13's particle-model vertex programs, the entries of
//! `shaders\particle_shared_vertex_shaders` that read `mesh_vertices`, place each vertex at
//! `position + size * R(axis, 2 pi rotation) * vertex` in world axes for the model orientations
//! (`prt3 +0x14` 8 and above). `axis` is the particle's packed axis and `rotation` its turn
//! fraction. The vertex is a 20-byte particle-model vertex, and each particle draws one mesh
//! variant's vertex range as a strip.
//!
//! A native definition selects model geometry with `+0x14B` 1, and the system's container at
//! `+0x18` names one model whose first mesh and first draw the particle draw instances. The stock
//! CPU model particle `80F2F8BC` of the auto rifle firing flow supplies the system, definition,
//! container, model and buffer templates. Its eight-row records and binding rows match the
//! billboard template, so the source simulation program runs unchanged. The vertex program reads
//! the eight record rows as `TEXCOORD0` to `TEXCOORD7` per instance and the model's Float3, Float2
//! and Float3 vertex as `POSITION0` to `POSITION2`, as the stock model program `80FE2682` does.
use super::*;
use crate::tiger::particle::Program;
use anyhow::bail;

/// The stock CPU model-particle system whose definition, container, model and buffers the
/// converted models copy.
const SYSTEM: u32 = 0x80f2_f8bc;
/// MCC's particle-model vertex type and its stride.
const PARTICLE_MODEL: u8 = 3;
const STRIDE: usize = 20;
/// Native particle-model vertices: Float3 position, Float2 texture coordinate, Float3 normal.
const NATIVE_STRIDE: usize = 32;
/// `prt3 +0x14` orientations the converter draws: `world (particle models)` and
/// `velocity horizontal (particle models)`, the HREK names of 8 and 9.
const WORLD: i16 = 8;
const VELOCITY_HORIZONTAL: i16 = 9;
/// Emitter `+7` model axis `globe`, by its HREK name.
const GLOBE: u8 = 3;
/// Particle attachment trigger `collision`, by its HREK name.
const COLLISION: u8 = 1;
/// The `prt3 +0x10` appearance bit of distortion. Every inspected particle model also sets bit 17,
/// which the tag compiler writes for model particles.
const DISTORTION: u32 = 0x8000;
/// Mesh variants the vertex program can select among, as MCC's `ParticleRenderMeshVS` holds them.
const VARIANTS: usize = 15;

pub(in crate::halo_reach) struct Model {
    pub source: Tag,
    pub name: String,
    pub life: f32,
    definition: Payload,
    material: Material,
    mesh: Mesh,
    /// The source appearance flags: bit 2 adds a random turn and bit 6 scales alpha by intensity.
    appearance: u32,
    near_fade: [f32; 2],
    /// The source coordinate system: 0 world space, 1 local space and 2 the parent's space.
    coordinates: i64,
    framed: bool,
    /// The source system's environment, disposition and camera mode.
    conditions: Value,
    limits: Vec<String>,
}

/// A particle model's vertices in native layout, its triangle list and its mesh variants.
struct Mesh {
    source: Tag,
    vertices: Vec<u8>,
    indices: Vec<u16>,
    /// Each variant's first and last source vertex.
    variants: Vec<[u32; 2]>,
    center: [f32; 3],
    radius: f32,
}

impl Model {
    /// As [`Emitter::bind_world`] does for a billboard: gives a world-space source system's
    /// particles the frame of a second locator binding and returns whether it did.
    pub(in crate::halo_reach) fn bind_world(&mut self) -> Result<bool> {
        if self.coordinates != 0 {
            return Ok(false);
        }
        let bytes = &mut self.definition.0;
        ensure!(
            bytes[0x40] == 0 && bytes[0x41] == 0 && bytes[0x47] == 1,
            "Native model particle definition bindings differ"
        );
        bytes[0x41] = 1;
        bytes[0x47] = 2;
        self.framed = true;
        Ok(true)
    }
}

/// Whether the system's particle draws a particle model.
pub(in crate::halo_reach) fn is_model(cache: &Cache, system: &Value) -> Result<bool> {
    let source: Tag = serde_json::from_value(system["tag"].clone())?;
    ensure!(source.group == "prt3", "Source emitter has no particle");
    Ok(cache.reference(source.address()? + 0x1bc, None)?.is_some())
}

fn material_pending(material: &Material) -> Option<&'static str> {
    let option = |name: &str| material.options.get(name).map(String::as_str);
    if option("albedo") != Some("diffuse_only") {
        return Some("Particle model albedo other than diffuse only");
    }
    if !matches!(
        option("blend_mode"),
        Some(
            "opaque"
                | "alpha_blend"
                | "pre_multiplied_alpha"
                | "additive"
                | "add_src_times_srcalpha"
        )
    ) {
        return Some("Another particle composition path");
    }
    if option("specialized_rendering") != Some("none")
        || !matches!(option("warp"), None | Some("none"))
    {
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

/// Whether the particle model at `at` has the one-mesh particle-model layout the converter reads.
fn mesh_pending(cache: &Cache, at: usize) -> Result<Option<&'static str>> {
    let sections = cache.block(at + 4, 92)?;
    let [row] = sections.as_slice() else {
        return Ok(Some("Particle model with several meshes"));
    };
    if cache.u8(row + 47)? != PARTICLE_MODEL
        || !matches!(cache.u8(row + 50)?, 3 | 5)
        || cache.i16(row + 24)? < 0
        || cache.block(at + 16, 52)?.is_empty()
    {
        return Ok(Some("Particle model vertex type, topology or compression"));
    }
    Ok(None)
}

/// Recognized source contracts that still require another native adapter, as
/// [`super::pending`] lists them for billboards.
pub(in crate::halo_reach) fn pending(
    cache: &Cache,
    system: &Value,
    emitter: &Value,
) -> Result<Vec<String>> {
    let source: Tag = serde_json::from_value(system["tag"].clone())?;
    ensure!(source.group == "prt3", "Source emitter has no particle");
    let at = source.address()?;
    let emitter_at = usize::try_from(
        emitter["offset"]
            .as_u64()
            .context("Source emitter address")?,
    )?;
    let mut result = Vec::new();
    // Particle collision is not translated, so a collision attachment never plays.
    for row in cache.block(at + 4, 0x14)? {
        if cache.u8(row + 0x10)? != COLLISION {
            result.push("Particle attachment events other than collision".into());
            break;
        }
    }
    match cache.reference(at + 0x1bc, None)? {
        Some(model) if model.group == "pmdf" => {
            if let Some(reason) = mesh_pending(cache, model.address()?)? {
                result.push(reason.into());
            }
        }
        _ => result.push("Particle model of another group".into()),
    }
    let style = cache.i16(at + 0x14)?;
    if !matches!(style, WORLD | VELOCITY_HORIZONTAL) {
        result.push(format!("Particle model orientation {style}"));
    }
    let axis = cache.u8(emitter_at + 7)?;
    if axis != GLOBE {
        result.push(format!("Particle model axis {axis}"));
    }
    if cache.u8(emitter_at + 4)? > 5
        || cache.reference(emitter_at + 0xc, None)?.is_some()
        || cache.reference(emitter_at + 0x1c, None)?.is_some()
    {
        result.push("Custom emitter geometry".into());
    }
    if cache.floats::<6>(emitter_at + 0xa0)? != [0.; 6] {
        result.push("Emitter direction interpolation".into());
    }
    ensure!(
        matches!(system["coordinates"].as_i64(), Some(0..=2)),
        "Unknown source particle coordinate system"
    );
    result.extend(super::conditions(system)?.reasons);
    if cache.u32(at + 0x10)? & DISTORTION != 0 {
        result.push("Distortion rendering".into());
    }
    let material = crate::halo_reach::material::embedded(cache, source, at + 0x40)?;
    if let Some(reason) = material_pending(&material) {
        result.push(reason.into());
    }
    let kinds = cache
        .block(emitter_at + 0x1ac, 0x18)?
        .into_iter()
        .map(|row| cache.i16(row))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        kinds.iter().all(|kind| (0..=5).contains(kind)),
        "Unknown source particle movement controller"
    );
    Ok(result)
}

/// The particle model `model`'s vertices in native layout, in its source units, and its variants'
/// strips as one triangle list. A variant row holds its first and last vertex after a header row
/// of the variant count and the longest variant, as MCC's `ParticleRenderMeshVS` reads them.
fn mesh(cache: &Cache, pages: &mut Pages, model: &Tag) -> Result<Mesh> {
    let at = model.address()?;
    let geometry = crate::halo_reach::model::Geometry::read(cache, model, at)?;
    ensure!(
        geometry.sections.len() == 1,
        "Source particle model needs one mesh"
    );
    let section = geometry
        .raw_section(cache, pages, 0)?
        .context("Source particle model has no vertices")?;
    ensure!(
        section.format == PARTICLE_MODEL
            && section.vertices.len() == section.count * STRIDE
            && section.count <= usize::from(u16::MAX),
        "Source particle model vertex layout differs"
    );
    let bounds = cache.floats::<10>(
        *cache
            .block(at + 16, 52)?
            .first()
            .context("Source particle model has no compression bounds")?
            + 4,
    )?;
    let mut vertices = Vec::with_capacity(section.count * NATIVE_STRIDE);
    let mut low = [f32::MAX; 3];
    let mut high = [f32::MIN; 3];
    for bytes in section.vertices.chunks_exact(STRIDE) {
        let word = |p: usize| u16::from_le_bytes([bytes[p], bytes[p + 1]]);
        let unorm = |p: usize, bound: usize| {
            bounds[bound] + f32::from(word(p)) / 65535. * (bounds[bound + 1] - bounds[bound])
        };
        // MCC's program reads positions and texture coordinates as UNorm16 across the
        // compression bounds and the normal as three half floats.
        let position = [unorm(0, 0), unorm(2, 2), unorm(4, 4)];
        let uv = [unorm(8, 6), unorm(10, 8)];
        let normal = [12, 14, 16].map(|p| half::f16::from_bits(word(p)).to_f32());
        ensure!(
            position
                .iter()
                .chain(&uv)
                .chain(&normal)
                .all(|v| v.is_finite()),
            "Nonfinite source particle model vertex"
        );
        for axis in 0..3 {
            low[axis] = low[axis].min(position[axis]);
            high[axis] = high[axis].max(position[axis]);
        }
        for value in position.into_iter().chain(uv).chain(normal) {
            vertices.extend(value.to_le_bytes());
        }
    }
    let count = u32::try_from(section.count)?;
    let rows = cache.block(at + 0xa8, 16)?;
    let mut variants = if let Some((&header, rows)) = rows.split_first() {
        let header = cache.floats::<2>(header)?;
        ensure!(
            header[0] == rows.len() as f32 && (1..=VARIANTS).contains(&rows.len()),
            "Source particle model variant table differs"
        );
        rows.iter()
            .map(|&row| {
                let [first, last] = cache.floats::<2>(row)?;
                ensure!(
                    first >= 0. && first <= last && last < count as f32 && first.fract() == 0.,
                    "Source particle model variant is outside its mesh"
                );
                Ok([first as u32, last as u32])
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![[0, count - 1]]
    };
    variants.sort_unstable();
    ensure!(
        variants.windows(2).all(|pair| pair[0][1] < pair[1][0]),
        "Source particle model variants overlap"
    );
    // The vertex program tells variants apart by vertex index, so several need the unindexed
    // order the inspected particle models store.
    ensure!(
        variants.len() == 1
            || section
                .indices
                .iter()
                .enumerate()
                .all(|(index, &vertex)| vertex as usize == index),
        "Source particle model variants need unindexed vertices"
    );
    let mut indices = Vec::new();
    for &[first, last] in &variants {
        let range = section
            .indices
            .get(first as usize..=last as usize)
            .context("Source particle model variant is outside its indices")?;
        let mut triangles = Vec::new();
        if section.topology == 3 {
            ensure!(
                range.len().is_multiple_of(3),
                "Incomplete source particle model triangle list"
            );
            triangles.extend(range.chunks_exact(3).map(|f| [f[0], f[1], f[2]]));
        } else {
            // Each variant is its own strip, whose winding alternates from its first vertex.
            let mut previous: Vec<u32> = Vec::new();
            let mut parity = 0;
            for &vertex in range {
                if vertex == section.restart {
                    previous.clear();
                    parity = 0;
                    continue;
                }
                if let [.., a, b] = previous.as_slice() {
                    triangles.push(if parity % 2 == 0 {
                        [*a, *b, vertex]
                    } else {
                        [*b, *a, vertex]
                    });
                    parity += 1;
                }
                previous.push(vertex);
            }
        }
        for triangle in triangles {
            if triangle[0] == triangle[1]
                || triangle[0] == triangle[2]
                || triangle[1] == triangle[2]
            {
                continue;
            }
            for vertex in triangle {
                ensure!(
                    vertex < count,
                    "Source particle model triangle is outside its mesh"
                );
                indices.push(u16::try_from(vertex)?);
            }
        }
    }
    // The native draw reads its index count as UInt16.
    ensure!(
        !indices.is_empty() && indices.len() <= usize::from(u16::MAX),
        "Source particle model has no drawable triangles or too many indices"
    );
    let center: [f32; 3] = std::array::from_fn(|axis| (low[axis] + high[axis]) * 0.5);
    let mut radius = 0f32;
    for row in vertices.chunks_exact(NATIVE_STRIDE) {
        let squared = (0..3)
            .map(|axis| {
                let value = f32::from_le_bytes(row[axis * 4..axis * 4 + 4].try_into().unwrap());
                (value - center[axis]).powi(2)
            })
            .sum::<f32>();
        radius = radius.max(squared.sqrt());
    }
    ensure!(radius.is_finite(), "Source particle model bounds overflow");
    Ok(Mesh {
        source: model.clone(),
        vertices,
        indices,
        variants,
        center,
        radius: radius.next_up(),
    })
}

pub(in crate::halo_reach) fn read(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    system: &Value,
    emitter: &Value,
    metres: f32,
    frame: &[f32; 16],
) -> Result<Model> {
    let reasons = pending(cache, system, emitter)?;
    ensure!(
        reasons.is_empty(),
        "Source particle model needs another adapter: {}",
        reasons.join(", ")
    );
    let source: Tag = serde_json::from_value(system["tag"].clone())?;
    let at = source.address()?;
    let emitter_at = usize::try_from(
        emitter["offset"]
            .as_u64()
            .context("Source emitter address")?,
    )?;
    let model = cache
        .reference(at + 0x1bc, Some("pmdf"))?
        .context("Source particle model")?;
    let mesh = mesh(cache, pages, &model)?;
    let material = crate::halo_reach::material::embedded(cache, source.clone(), at + 0x40)?;
    ensure!(
        material.mapping(&["base_map"]).is_some(),
        "Source particle model has no base map"
    );
    let appearance = cache.u32(at + 0x10)?;
    let coordinates = system["coordinates"]
        .as_i64()
        .filter(|value| (0..=2).contains(value))
        .context("Unknown source particle coordinate system")?;
    let properties = super::properties(cache, at, emitter_at)?;
    let life = super::life(&properties)?;
    let mut limits = vec![
        "Native random streams differ from Reach. Source LOD is evaluated at full detail.".into(),
        "The model draws its base map times the particle color, as MCC's unlit particle-model program does. Source lightmap tint, fog and motion blur are not applied.".into(),
        "Each particle turns about a uniformly random axis by its rotation. The source axis angle and reference axis do not shape that axis.".into(),
        "Both faces draw.".into(),
    ];
    let conditions = super::conditions(system)?;
    limits.extend(conditions.limits);
    if cache.i16(at + 0x14)? == VELOCITY_HORIZONTAL {
        limits.push("The source velocity-horizontal model orientation draws in world axes.".into());
    }
    if !cache.block(at + 4, 0x14)?.is_empty() {
        limits.push(
            "Source collision attachments do not play, since particle collision is not translated."
                .into(),
        );
    }
    if mesh.variants.len() > 1 {
        limits.push("Each particle draws the mesh variant its own random value selects, not MCC's selection.".into());
    }
    if material
        .options
        .get("depth_fade")
        .is_some_and(|v| v != "off")
    {
        limits.push("Depth fade is not applied to particle models.".into());
    }
    if material
        .options
        .get("lighting")
        .is_some_and(|v| v != "none")
    {
        limits.push("Source particle lighting is not applied.".into());
    }
    if coordinates == 2 {
        limits.push("Source parent space is drawn in the emitter locator's frame, so particles follow the emitter rather than its parent object.".into());
    }
    let program =
        super::simulation::build(cache, emitter_at, &properties, metres, frame, &mut limits)?;
    let definition = definition(native, &program, life)?;
    let near = super::values::<2>(system, "near_fade_source")?;
    Ok(Model {
        source,
        name: emitter["name"]
            .as_str()
            .context("Source emitter name")?
            .into(),
        life,
        definition,
        material,
        mesh,
        appearance,
        near_fade: [near[0] / metres, near[1] * metres],
        coordinates,
        framed: false,
        conditions: conditions.report,
        limits,
    })
}

/// The stock system, checked to be a CPU model particle.
fn stock(native: &mut Reader) -> Result<Payload> {
    let system = (*native.tag(SYSTEM, Some(0x80806e28))?).clone();
    ensure!(
        [4, 8, 0x10]
            .into_iter()
            .all(|at| system.u32(at).ok() == Some(u32::MAX)),
        "Native model particle system needs another computational stage"
    );
    Ok(system)
}

/// The native model-particle definition running `program`, whose particles live at most `life`
/// seconds. The template differs from the billboard template `80C0D9C7` in its geometry selector
/// `+0x14B` 1 and the matching upper nibble of layout keys `+0x48` and `+0x4C`.
fn definition(native: &mut Reader, program: &Program, life: f32) -> Result<Payload> {
    let template = stock(native)?.u32(0)?;
    let original = native.tag(template, Some(0x80806e2c))?;
    ensure!(
        original.u8(0x14b)? == 1
            && original.u8(0x142)? == 8
            && original.u8(0x143)? == 3
            && original.u32(0x48)? == 0x1000_0008
            && original.u32(0x4c)? == 0x1000_0003
            && original.bytes::<8>(0x40)? == [0, 0, 0xff, 0xff, 4, 0, 0, 1],
        "Native model particle definition envelope differs"
    );
    let mut definition = program.write(&original, 8, 8)?;
    // As on the billboard definition, the source program authors positions and velocities.
    definition.0[0x115] = 0;
    definition.0[0x116] = 0;
    definition.0[0x149] = 0;
    definition.0[0x14a] = 1;
    put(&mut definition.0, 0x120, &(life + 0.05).to_le_bytes())?;
    Ok(definition)
}

const VERTEX: &str = r#"
cbuffer Object : register(b11) { float4 objectData[4]; };
cbuffer View : register(b12) { float4 viewData[8]; };
$VARIANTS$
void main(float4 a0:TEXCOORD0,float4 a1:TEXCOORD1,float4 a2:TEXCOORD2,float4 a3:TEXCOORD3,
float4 a4:TEXCOORD4,float4 a5:TEXCOORD5,float4 a6:TEXCOORD6,float4 a7:TEXCOORD7,
float3 vertexPosition:POSITION0,float2 vertexUv:POSITION1,float3 vertexNormal:POSITION2,uint id:SV_VertexID,
out float4 uv:TEXCOORD0,out float4 color:TEXCOORD1,out float4 property:TEXCOORD2,
out float4 animation:TEXCOORD3,out float4 random:TEXCOORD4,out float4 position:SV_POSITION0) {
float3 center=objectData[0].xyz*a2.x+objectData[1].xyz*a2.y+objectData[2].xyz*a2.z+objectData[3].xyz;
float height=a7.x*2.0-1.0;
float ring=sqrt(saturate(1.0-height*height));
float azimuth=a7.y*6.28318530718;
float3 axis=float3(ring*cos(azimuth),ring*sin(azimuth),height);
float angle=a2.w+$RANDOM_ROTATION$;
float sine,cosine;sincos(angle,sine,cosine);
float3 p=vertexPosition*a3.y;
float3 world=center+p*cosine+cross(axis,p)*sine+axis*dot(axis,p)*(1.0-cosine);
position=viewData[0]*world.x+viewData[1]*world.y+viewData[2]*world.z+viewData[3];
uv=float4(vertexUv,length(viewData[7].xyz-world),a1.w);
color=a0;property=a3;animation=a4;random=a7;
$VARIANT_TEST$
if(a1.w>=1.0||abs(a3.y)<0.000001) position=float4(10000,10000,-10000,1);
}
"#;

const PIXEL: &str = r#"
Texture2D<float4> Map:register(t0);
SamplerState SurfaceSampler:register(s1);
cbuffer Effect:register(b13) { float4 effectData[2]; };
float4 main(float4 uv:TEXCOORD0,float4 color:TEXCOORD1,float4 property:TEXCOORD2,
float4 animation:TEXCOORD3,float4 random:TEXCOORD4,float4 position:SV_POSITION0):SV_TARGET0 {
float4 texel=Map.Sample(SurfaceSampler,uv.xy*float2($MAP_U$,$MAP_V$)+float2($MAP_X$,$MAP_Y$));
float opacity=saturate(texel.a*color.a);
float intensity=max(property.w,0);
$INTENSITY_ALPHA$
float visibility=saturate((uv.z-$NEAR_CUTOFF$)*$NEAR_SCALE$);
opacity*=visibility;
float3 rgb=texel.rgb*color.rgb*intensity+$SELF_ILLUM$;
$OUTPUT$
}
"#;

/// The model vertex program. The draw record's row 2 holds the position and rotation, row 3 Y the
/// size, row 1 W the normalized age and row 7 the particle's stable random values. Random X and Y
/// draw the turning axis uniformly over the sphere, Z the mesh variant and W the random turn the
/// billboards also take. The model's vertices keep their source units and the size carries the
/// conversion to meters. A vertex outside the particle's variant collapses with its triangle.
fn vertex(model: &Model) -> String {
    let variants = &model.mesh.variants;
    let (table, test) = if variants.len() > 1 {
        (
            format!(
                "static const uint2 variants[{}]={{{}}};",
                variants.len(),
                variants
                    .iter()
                    .map(|[first, last]| format!("uint2({first},{last})"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            format!(
                "uint variant=min((uint)(a7.z*{count}.0),{last}u);\
if(id<variants[variant].x||id>variants[variant].y) position=float4(10000,10000,-10000,1);",
                count = variants.len(),
                last = variants.len() - 1
            ),
        )
    } else {
        (String::new(), String::new())
    };
    let mut vertex = VERTEX.to_owned();
    for (key, value) in [
        ("$VARIANTS$", table),
        ("$VARIANT_TEST$", test),
        (
            "$RANDOM_ROTATION$",
            if model.appearance & 4 != 0 {
                "a7.w*6.28318530718".to_owned()
            } else {
                "0.0".to_owned()
            },
        ),
    ] {
        vertex = vertex.replace(key, &value);
    }
    vertex
}

/// The model pixel program. MCC's particle-model pixel program returns the base map times the
/// vertex color plus the self-illumination color, its alpha times the vertex alpha and exposure.
/// An opaque source writes full coverage.
fn pixel(model: &Model) -> Result<String> {
    let material = &model.material;
    let map = material
        .mapping(&["base_map"])
        .context("Particle model base map")?;
    let output = match material.options["blend_mode"].as_str() {
        "opaque" => "return float4(rgb,1);",
        "additive" => "return float4(rgb*color.a*visibility*effectData[1].z*effectData[1].w,0);",
        "add_src_times_srcalpha" => "return float4(rgb*opacity*effectData[1].z*effectData[1].w,0);",
        "pre_multiplied_alpha" => "return float4(rgb*color.a*visibility,opacity);",
        "alpha_blend" => "return float4(rgb*opacity,opacity);",
        _ => bail!("Unsupported particle model blend mode"),
    };
    let self_illum = if material
        .options
        .get("self_illumination")
        .map(String::as_str)
        == Some("constant_color")
    {
        material.constant("self_illum_color").unwrap_or([0.; 4])
    } else {
        [0.; 4]
    };
    ensure!(
        map.transform
            .iter()
            .chain(&self_illum)
            .chain(&model.near_fade)
            .all(|v| v.is_finite()),
        "Nonfinite particle model material values"
    );
    let mut pixel = PIXEL.to_owned();
    for (key, value) in [
        ("$MAP_U$", format!("{:.9}", map.transform[0])),
        ("$MAP_V$", format!("{:.9}", map.transform[1])),
        ("$MAP_X$", format!("{:.9}", map.transform[2])),
        ("$MAP_Y$", format!("{:.9}", map.transform[3])),
        (
            "$INTENSITY_ALPHA$",
            if model.appearance & 0x40 != 0 {
                "opacity*=intensity;"
            } else {
                ""
            }
            .into(),
        ),
        ("$NEAR_SCALE$", format!("{:.9}", model.near_fade[0])),
        ("$NEAR_CUTOFF$", format!("{:.9}", model.near_fade[1])),
        (
            "$SELF_ILLUM$",
            format!(
                "float3({:.9},{:.9},{:.9})",
                self_illum[0], self_illum[1], self_illum[2]
            ),
        ),
        ("$OUTPUT$", output.into()),
    ] {
        pixel = pixel.replace(key, &value);
    }
    Ok(pixel)
}

/// Adds the model's material `{prefix}-material` on the billboard shell, which draws both faces.
fn material(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    model: &Model,
    prefix: &str,
) -> Result<String> {
    let map = model
        .material
        .mapping(&["base_map"])
        .context("Particle model base map")?;
    let template = super::material::sampler_template(native)?;
    let samplers = vec![native::material::texture_sampler(
        native, graph, template, map,
    )?];
    let textures = vec![(
        0u32,
        native::material::texture(cache, pages, native, graph, map, false, true)?,
    )];
    let vertex = vertex(model);
    let pixel = pixel(model)?;
    super::material::shell(
        native,
        graph,
        prefix,
        (&vertex, &pixel),
        (textures, &samplers),
        None,
    )
}

/// Adds the model's buffers, its native model `{prefix}-model` drawing `material` and the
/// container `{prefix}-container` naming it, copies of the stock system's own, and returns the
/// container's symbol.
fn geometry(
    native: &mut Reader,
    graph: &mut Graph,
    stock: &Payload,
    mesh: &Mesh,
    material: &str,
    prefix: &str,
) -> Result<String> {
    let container_tag = stock.u32(0x18)?;
    let mut container = (*native.tag(container_tag, Some(0x80806e2e))?).clone();
    let rows = container.array(16, 4, Some(0x808073a4))?;
    let [row] = rows.as_slice() else {
        bail!("Native model particle container needs one model");
    };
    let row = *row;
    let model_tag = container.u32(row)?;
    let mut model = (*native.tag(model_tag, Some(0x808073a5))?).clone();
    let meshes = model.array(16, 136, Some(0x80807378))?;
    let [at] = meshes.as_slice() else {
        bail!("Native particle model needs one mesh");
    };
    let at = *at;
    let draws = model.array(at + 24, 32, Some(0x8080737e))?;
    ensure!(
        !draws.is_empty()
            && model.u64(0x30)? == NATIVE_STRIDE as u64
            && [4, 8, 12]
                .into_iter()
                .all(|offset| model.u32(at + offset).ok() == Some(u32::MAX)),
        "Native particle model needs another vertex layout"
    );
    let mut patches = Vec::new();
    for (offset, kind, data) in [
        (0, "vertices", mesh.vertices.clone()),
        (
            16,
            "indices",
            mesh.indices.iter().flat_map(|v| v.to_le_bytes()).collect(),
        ),
    ] {
        let template = model.u32(at + offset)?;
        let mut header = native.tag(template, None)?.0.clone();
        if offset == 0 {
            ensure!(
                header.len() == 12 && Payload(header.clone()).u16(4)? == NATIVE_STRIDE as u16,
                "Native particle model vertex header differs"
            );
            put(&mut header, 0, &u32::try_from(data.len())?.to_le_bytes())?;
        } else {
            ensure!(
                header.len() == 24 && header[0] == 1,
                "Native particle model index header differs"
            );
            // Byte 1 selects 32-bit indices, and these are 16 bits.
            header[1] = 0;
            put(&mut header, 8, &u64::try_from(data.len())?.to_le_bytes())?;
        }
        let symbol = format!("{prefix}-{kind}");
        let data_symbol = format!("{symbol}-data");
        graph.add(&symbol, template, &header, Some(&data_symbol), vec![])?;
        graph.add(
            &data_symbol,
            native.reference(template)?,
            &data,
            Some(&symbol),
            vec![],
        )?;
        put(&mut model.0, at + offset, &u32::MAX.to_le_bytes())?;
        patches.push(json!({"offset":at + offset,"symbol":symbol}));
    }
    // The stock model stores normalized vertices and its scale, offset and texture transform at
    // `+0x50`, `+0x60` and `+0x70`, which the draw publishes to the object constants. These
    // vertices are final, so the transforms are identities, and the vertex program does not read
    // them. The bounds hold the conservative sphere at `+0x20` and `+0x80`, as the stock rows do.
    let mut header = Vec::new();
    for value in [mesh.radius; 4]
        .into_iter()
        .chain([1., 1., 1., 0.])
        .chain([0., 0., 0., 1.])
        .chain([1., 1., 0., 0.])
    {
        header.extend(value.to_le_bytes());
    }
    put(&mut model.0, 0x20, &header[..16])?;
    put(&mut model.0, 0x50, &header[16..])?;
    for (axis, value) in mesh.center.iter().chain([&mesh.radius]).enumerate() {
        put(&mut model.0, 0x80 + axis * 4, &value.to_le_bytes())?;
    }
    let count = u32::try_from(mesh.indices.len())?;
    for draw in draws {
        ensure!(
            model.u16(draw + 6)? == 3,
            "Native particle model draw topology differs"
        );
        put(&mut model.0, draw, &u32::MAX.to_le_bytes())?;
        put(&mut model.0, draw + 8, &0u32.to_le_bytes())?;
        put(&mut model.0, draw + 12, &count.to_le_bytes())?;
        put(&mut model.0, draw + 16, &(count / 3).to_le_bytes())?;
        patches.push(json!({"offset":draw,"symbol":material}));
    }
    let model_symbol = format!("{prefix}-model");
    graph.add(&model_symbol, model_tag, &model.0, None, patches)?;
    put(&mut container.0, row, &u32::MAX.to_le_bytes())?;
    let symbol = format!("{prefix}-container");
    graph.add(
        &symbol,
        container_tag,
        &container.0,
        None,
        vec![json!({"offset":row,"symbol":model_symbol})],
    )?;
    Ok(symbol)
}

pub(in crate::halo_reach) fn emit(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    model: &Model,
    prefix: &str,
) -> Result<(String, Value)> {
    let stock = stock(native)?;
    let material = material(cache, pages, native, graph, model, prefix)?;
    let container = geometry(native, graph, &stock, &model.mesh, &material, prefix)?;
    let definition = format!("{prefix}-definition");
    graph.add(
        &definition,
        stock.u32(0)?,
        &model.definition.0,
        None,
        Vec::new(),
    )?;
    let mut system = stock.0.clone();
    for field in [0, 0x14, 0x18] {
        put(&mut system, field, &u32::MAX.to_le_bytes())?;
    }
    let symbol = format!("{prefix}-system");
    graph.add(
        &symbol,
        SYSTEM,
        &system,
        None,
        vec![
            json!({"offset":0,"symbol":definition}),
            json!({"offset":0x14,"symbol":material}),
            json!({"offset":0x18,"symbol":container}),
        ],
    )?;
    let mut limits = model.limits.clone();
    let space = match (model.coordinates, model.framed) {
        (1, _) => "local",
        (2, _) => "parent",
        (_, true) => "world",
        (_, false) => {
            limits.push("The source system is world space, but its host binds only the emitter's locator, so its particles follow the emitter.".into());
            "emitter"
        }
    };
    Ok((
        symbol,
        json!({"source":model.source,"emitter":model.name,"kind":"model","model":model.mesh.source,
        "vertices":model.mesh.vertices.len() / NATIVE_STRIDE,"triangles":model.mesh.indices.len() / 3,
        "variants":model.mesh.variants,"maximum_life":model.life,"space":space,
        "conditions":model.conditions,"limits":limits,"gameplay_verified":false}),
    ))
}
