//! The source HUD scope drawn in the reticle pass of the weapon's own model.
use super::*;
use crate::presentation::{append, geometry::Mesh};
use crate::tiger::shader::{identity::Stage, inputs::Inspection};

/// The body mesh group that carries the overlay. Source material keys are cache datums, whose
/// salt never reaches `FFFF` in the examined caches. [`quad`] refuses a collision.
pub(in crate::halo_reach::native) const KEY: u32 = 0xFFFF_5C0F;

/// Adds the overlay quad to the weapon's own mesh, drawn in the reticle pass. Aimed Marathon
/// glass and framed reticles show in game from the weapon's body model, while a borrowed optic
/// part's model never showed this overlay. The quad's corners straddle the mesh center in X and
/// Y inside its bounds, so encoding leaves the bounds unchanged and each corner's quantized
/// position keeps its sign. The vertex program reads only those signs.
pub(in crate::halo_reach::native) fn quad(
    mesh: &mut Mesh,
    stages: &mut BTreeMap<u32, u8>,
) -> Result<()> {
    ensure!(
        !stages.contains_key(&KEY) && mesh.groups.iter().all(|(key, _)| *key != KEY),
        "A source material uses the scope overlay key"
    );
    let low = std::array::from_fn::<_, 3, _>(|a| {
        mesh.positions
            .iter()
            .map(|p| p[a])
            .fold(f32::INFINITY, f32::min)
    });
    let high = std::array::from_fn::<_, 3, _>(|a| {
        mesh.positions
            .iter()
            .map(|p| p[a])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    let center: [f32; 3] = std::array::from_fn(|a| (low[a] + high[a]) / 2.);
    let half = (high[0] - low[0]).min(high[1] - low[1]) / 4.;
    ensure!(
        half.is_finite() && half > 0.,
        "Scope overlay needs a mesh with extent"
    );
    let attributes = *mesh.attributes.first().context("Scope overlay mesh")?;
    let weights = mesh.weights.first().copied();
    let first = u32::try_from(mesh.positions.len())?;
    for [x, y] in [[-1., -1.], [1., -1.], [-1., 1.], [1., 1.]] {
        mesh.positions
            .push([center[0] + x * half, center[1] + y * half, center[2]]);
        mesh.attributes.push(attributes);
        mesh.weights.extend(weights);
    }
    mesh.groups.push((
        KEY,
        vec![
            [first, first + 1, first + 2],
            [first + 2, first + 1, first + 3],
        ],
    ));
    stages.insert(KEY, 16);
    Ok(())
}

/// The full-screen quad keeps the native reticle's depth and clip W. The template's own
/// producer fills the reticle rows, and a point on the sight axis at its focal plane gives the
/// depth, as an aimed Marathon overlay does. Neither this overlay at a fixed depth without the
/// producer nor Marathon glass projected without it appeared in game. Which difference matters
/// is not isolated.
fn overlay_vertex(rows: usize, matrix: u8, focal: u8) -> String {
    format!(
        r#"
cbuffer Reticle:register(b0){{float4 reticleData[{rows}];}};
void main(float4 p:POSITION0,
out float2 uv:TEXCOORD0,out float4 position:SV_POSITION0) {{
float2 corner=sign(p.xy);
uv=corner;
float4 focal=reticleData[{z}]*-reticleData[{focal}].x+reticleData[{w}];
position=float4(corner,focal.z/focal.w,1);
}}
"#,
        z = matrix + 2,
        w = matrix + 3,
    )
}

/// The native reticle's projection rows: its matrix, from the view extern, and its focal plane,
/// the row after the screen offset. The vertex program must read that plane.
fn projection(native: &mut Reader, material: &Payload) -> Result<(usize, u8, u8)> {
    let code = material
        .array(0x68, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| material.u8(at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        code.len() == 10
            && code[..4] == [0x3e, 2, 6, 0x44]
            && code[5..9] == [0x3d, 0x49, 4, 0x43]
            && code[4].checked_add(4) == Some(code[9]),
        "Native reticle projection producer differs"
    );
    let focal = code[9].checked_add(1).context("Native reticle focal row")?;
    let values = material.array(0x98, 16, Some(0x80800090))?;
    let plane = material.f32(
        *values
            .get(usize::from(focal))
            .context("Native reticle focal plane is absent")?,
    )?;
    ensure!(
        plane.is_finite() && plane > 0.,
        "Native reticle focal plane is invalid"
    );
    let shader = native.tag(native.reference(material.u32(0x48)?)?, None)?;
    ensure!(
        Inspection::read(Stage::Vertex, &shader.0)?
            .inputs
            .constant_reads
            .get(&0)
            .is_some_and(|reads| reads
                .iter()
                .any(|r| r.index == Some(u32::from(focal)) && r.lanes.contains('x'))),
        "Native reticle shader has no focal-plane consumer"
    );
    Ok((values.len(), code[4], focal))
}

mod layers;

pub(super) struct Source<'a> {
    pub cache: &'a cache::Cache,
    pub pages: &'a mut resource::Pages,
    pub overlay: &'a crate::halo_reach::hud::scope::Scope,
}

/// Builds the overlay material of the [`KEY`] group. `inputs` are the weapon body's model
/// inputs, whose aim visibility also hides the body's surfaces.
pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    inputs: &BTreeMap<String, u8>,
    source: Source<'_>,
) -> Result<Value> {
    let name = format!("{:08X}", crate::tiger::channel::inherited::AIM_VISIBILITY);
    let input = *inputs
        .get(&name)
        .context("Scoped weapon body lacks aim visibility")?;
    let material = native.tag(0x8152c441, Some(0x808071e8))?;
    ensure!(
        material.u32(24)? == 0x04002083 && material.u32(32)? == 0x88,
        "Native reticle rendering contract differs"
    );
    let mut bytes = material.0.clone();
    let code = material.array(0x2e8, 1, Some(0x80800009))?;
    let expected = hex::decode(
        "4c0049213f490c47264d022200232200370023220043033c5c0043063c5c0143073c5c0243083c5c0343093c5c04430a3c5c05430b3d49054312",
    )?;
    ensure!(
        code.len() == expected.len()
            && code
                .iter()
                .zip(&expected)
                .all(|(&at, &value)| material.0[at] == value),
        "Native reticle visibility program differs"
    );
    // Keep the native linear clamp sampler. The selected weapon supplies all
    // texture pixels and its body supplies the live aim input. The native program's
    // display rows 6 to 11 follow it unchanged.
    for at in [0x2d0, 0x2f8, 0x328] {
        put(&mut bytes, at, &[0; 16])?;
    }
    // The expression binds that sampler, row 0 of the table at `+0x308`, to its register with its
    // first four bytes. Without them the overlay samples whatever sampler the pass left bound.
    let binding = &expected[..4];
    let mut program = binding.to_vec();
    program.extend([0x4d, input, 0x43, 3]);
    program.extend_from_slice(&expected[expected.len() - 35..expected.len() - 5]);
    append(&mut bytes, 0x2e8, 0x80800009, &program, 1)?;
    bytes[0x33c] = 0x10;
    put(&mut bytes, 0x348, &0u32.to_le_bytes())?;
    put(&mut bytes, 0x34c, &u32::MAX.to_le_bytes())?;
    // The overlay keeps the reticle pass's own depth and stencil state. Rasterizer row one
    // draws both faces of the quad.
    bytes[0x22] = 0x81;
    put(
        &mut bytes,
        0x338,
        &u32::try_from(inputs.len())?.to_le_bytes(),
    )?;
    // The vertex stage keeps the template's producer, constants and buffer binding.
    let (rows, matrix, focal) = projection(native, &material)?;
    let mut textures = Vec::new();
    let mut slots = Vec::new();
    for layer in &source.overlay.layers {
        let texture = crate::halo_reach::native::material::texture(
            source.cache,
            source.pages,
            native,
            graph,
            &layer.mapping,
            false,
            false,
        )?;
        let slot = if let Some(slot) = textures.iter().position(|s| s == &texture) {
            slot
        } else {
            textures.push(texture);
            textures.len() - 1
        };
        slots.push(slot);
    }
    ensure!(
        textures.len() <= 16,
        "Scope exceeds the native texture binding limit"
    );
    let mut fixed = Vec::new();
    for slot in 0..textures.len() {
        fixed.extend(u32::try_from(slot)?.to_le_bytes());
        fixed.extend(u32::MAX.to_le_bytes());
    }
    append(&mut bytes, 0x2d0, 0x80807211, &fixed, 8)?;
    let vertex = "scope-vertex";
    let pixel = "scope-pixel";
    crate::halo_reach::native::material::program(
        native,
        graph,
        vertex,
        material.u32(0x48)?,
        &overlay_vertex(rows, matrix, focal),
        true,
    )?;
    crate::halo_reach::native::material::program(
        native,
        graph,
        pixel,
        material.u32(0x2c8)?,
        &layers::pixel(source.overlay, &slots, binding[3] & 0x1f)?,
        false,
    )?;
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x2c8, &u32::MAX.to_le_bytes())?;
    let mut patches = vec![
        json!({"offset":0x48,"symbol":vertex}),
        json!({"offset":0x2c8,"symbol":pixel}),
    ];
    for (row, texture) in Payload(bytes.clone())
        .array(0x2d0, 8, Some(0x80807211))?
        .into_iter()
        .zip(&textures)
    {
        patches.push(json!({"offset":row+4,"symbol":texture}));
    }
    let symbol = format!("material-{KEY:08X}");
    graph.add(&symbol, 0x8152c441, &bytes, None, patches)?;
    Ok(
        json!({"body_group":format!("{KEY:08X}"),"material":symbol,"stage":16,"visibility_input":name,"visibility_slot":input,
        "native_visibility_material":"8152C441","projection":"Source HUD placement about the screen centre through the full-screen HUD attributes' virtual canvas, curvature and safe frame, with bitmap size flags and extended borders","source_hud":source.overlay,"textures":textures,
        "color":"Source linear color with native reticle exposure compensation and display rows 6 to 11",
        "shown_from_aim_visibility":0.99,"native_reticle_rows":{"count":rows,"matrix":matrix,"focal_plane":focal},
        "depth_stencil_state":"reticle pass","rasterizer_state":1,
        "gameplay_verified":false}),
    )
}
