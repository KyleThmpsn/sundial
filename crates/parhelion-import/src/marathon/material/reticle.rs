//! Source artwork equations on the native scope mask and reticle projection.
use super::*;

/// Static target inputs for source-only reticle channels whose cached storage is zero.
/// These preserve source colors and artwork with unit extent, scale and visibility.
/// They do not reproduce the source controller's color customization or zoom fade.
pub(super) fn preset(name: u32) -> Option<(&'static str, [f32; 4])> {
    Some(match name {
        0x4269E744 => ("Reticle extent", [1.; 4]),
        0xC026EB02 => ("Reticle scale", [1.; 4]),
        0x79FD498A => ("Reticle visibility", [1.; 4]),
        // The M77 procedural reticle multiplies its color and opacity by this channel through the
        // same fade-in cubic as aim visibility, so a zero cached value erases its artwork.
        0x3F8A2821 => ("Sight visibility", [1.; 4]),
        // The depth controller evaluates a power law of this positive scale.
        // A zero cached value is outside its logarithm's domain.
        0x1B68B957 => ("Reticle depth scale", [1.; 4]),
        _ => return None,
    })
}

fn stencil(listing: &str) -> Result<()> {
    let definitions = listing
        .lines()
        .filter_map(|line| {
            let line = line.trim().split(';').next()?.trim().replace(" fast ", " ");
            let (name, code) = line.strip_prefix('%')?.split_once(" = ")?;
            Some((name.to_owned(), code.to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    let next = |code: &str| -> Result<String> {
        let found = definitions
            .iter()
            .filter(|(_, value)| value.as_str() == code)
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        ensure!(
            found.len() == 1,
            "Unsupported source reticle stencil operation: {code}"
        );
        Ok(found[0].clone())
    };
    let loads = definitions
        .iter()
        .filter(|(_, code)| code.starts_with("call %dx.types.ResRet.i32 @dx.op.textureLoad.i32("))
        .collect::<Vec<_>>();
    ensure!(loads.len() == 1, "Reticle needs one unsigned stencil load");
    let extract = next(&format!(
        "extractvalue %dx.types.ResRet.i32 %{}, 1",
        loads[0].0
    ))?;
    let cast = next(&format!("uitofp i32 %{extract} to float"))?;
    let add = next(&format!("fadd float %{cast}, 5.000000e-01"))?;
    let scale = next(&format!("fmul float %{add}, 1.250000e-01"))?;
    let fraction = next(&format!(
        "call float @dx.op.unary.f32(i32 22, float %{scale})"
    ))?;
    let comparison = next(&format!("fcmp olt float %{fraction}, 5.000000e-01"))?;
    ensure!(
        listing.contains(&format!("br i1 %{comparison}, label %")),
        "Reticle stencil comparison has no branch"
    );
    Ok(())
}

pub(super) fn pixel(
    listing: &str,
    parsed: &dxil::Program,
    options: &mut dxil::Options,
) -> Result<()> {
    stencil(listing)?;
    ensure!(
        parsed.resources()?.get(&(0, 0)) == Some(&(2, 0x405)),
        "Source reticle has no unsigned stencil texture"
    );
    options.texture_registers.insert(0, 6);
    options
        .constants
        .insert((13, 1), "float4(cb13[1].z,1,1,cb13[1].w)".into());
    Ok(())
}

/// The reticle of a sight whose source eye sits inside its sight box covers the whole aimed view,
/// as the source shows it, while the housing that holds its aperture is hidden. Stencil value 4
/// sets the bit the source test selects, so the reticle no longer stops at the aperture mask.
/// Aim visibility still sets its opacity through the source expression.
pub(super) fn unmasked(pixel: String) -> Result<String> {
    ensure!(
        pixel.contains("Texture2D<uint4> t0:register(t6);"),
        "Reticle stencil texture differs"
    );
    let loads = pixel.match_indices("t0.Load(").collect::<Vec<_>>();
    ensure!(loads.len() == 1, "Reticle needs one stencil load");
    let start = loads[0].0;
    let end = pixel[start..]
        .find(';')
        .context("Reticle stencil load statement")?
        + start;
    Ok(format!(
        "{}uint4(0,4,0,0){}",
        &pixel[..start],
        &pixel[end..]
    ))
}

/// An enclosed sight with an angular override shows the source's aimed composition. Its reticle
/// takes the source view's projection in the vertex adapter. Once aim visibility in `row` passes
/// 0.99, where the rest of the weapon disappears, it darkens the view outside the aperture as the
/// source housing does. The native viewport row `cb12[12]` holds dimensions in XY and their
/// reciprocals in ZW, so screen position gives each pixel's tangent in the source view.
pub(super) fn framed(pixel: String, row: u8, framing: &geometry::Framing) -> Result<String> {
    let mut pixel = scope::declare_row(unmasked(pixel)?, row)?;
    ensure!(
        pixel.contains("register(b12)"),
        "Reticle has no viewport constants"
    );
    let start = pixel.find("void main(").context("Reticle entry")? + "void main(".len();
    let end = pixel[start..]
        .find(") {")
        .context("Reticle parameter list")?
        + start;
    let parameters = pixel[start..end]
        .split(',')
        .map(str::trim)
        .collect::<Vec<_>>();
    let position = parameters
        .iter()
        .find_map(|p| p.strip_suffix(":SV_Position0")?.split_whitespace().nth(1))
        .context("Reticle screen position input")?
        .to_owned();
    ensure!(
        parameters.contains(&"out float4 o0:SV_Target0"),
        "Reticle color output differs"
    );
    let mut code = format!(
        "\nfloat2 framedScreen={position}.xy*cb12[12].zw;\nfloat2 framedTangent=float2(framedScreen.x*2-1,(1-framedScreen.y*2)*cb12[12].y*cb12[12].z)*{};\nfloat framedInside=1e20;\n",
        vertex::float(framing.tangent)
    );
    for edge in &framing.window {
        code.push_str(&format!(
            "framedInside=min(framedInside,dot(float3(framedTangent,1),float3({},{},{})));\n",
            vertex::float(edge[0]),
            vertex::float(edge[1]),
            vertex::float(edge[2])
        ));
    }
    code.push_str(&format!(
        "if(framedInside<=0){{o0=float4(0,0,0,saturate((cb0[{row}].x-0.99)*100));return;}}\n"
    ));
    pixel.insert_str(end + ") {".len(), &code);
    Ok(pixel)
}

pub(super) fn validate(native: &mut Reader, source: &Payload, shell: &Payload) -> Result<()> {
    ensure!(
        source
            .array(0x280, 24, None)?
            .into_iter()
            .all(|at| source.u32(at).is_ok_and(|slot| !matches!(slot, 0 | 6))),
        "Reticle stencil conflicts with a fixed texture"
    );
    // The native reticle reads the same stencil channel and bit as the source shaders.
    // Its material expression publishes the target in register 6 and its viewport dimensions.
    let code = shell
        .array(0x2E8, 1, None)?
        .into_iter()
        .map(|at| shell.u8(at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        code.windows(5)
            .filter(|bytes| *bytes == [0x3F, 0x49, 0x0C, 0x47, 0x26])
            .count()
            == 1
            && code.ends_with(&[0x3D, 0x49, 5, 0x43, 18]),
        "Native reticle renderer bindings differ"
    );
    let shader = native.tag(native.reference(shell.u32(0x2C8)?)?, None)?;
    ensure!(
        Inspection::read(Stage::Pixel, &shader.0)?
            .inputs
            .resources
            .get(&6)
            .is_some_and(|kind| kind == "dcl_resource_texture2d (uint,uint,uint,uint) @"),
        "Native reticle stencil scalar type differs"
    );
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) struct Projection {
    pub matrix: u8,
    pub offset: u8,
    pub depth: u8,
    pub vectors: usize,
    /// For a framed sight, the tangent of half the source horizontal view.
    pub framing: Option<f32>,
}

pub(super) struct Vertex {
    pub projection: Projection,
    pub plane: Value,
    code: Vec<u8>,
    constants: Vec<[u8; 16]>,
    values: Vec<[u8; 16]>,
    objects: u32,
}

pub(super) fn vertex(
    native: &mut Reader,
    material: &Payload,
    shell: &Payload,
    code: &[u8],
    values: &[[u8; 16]],
    mut context: expression::Context<'_>,
    framing: Option<&geometry::Framing>,
) -> Result<Vertex> {
    // Source projection matrix, screen offset, then the source-owned depth channel.
    // The material's row allocation varies with its other declared inputs.
    ensure!(
        code.len() >= 10
            && code[..4] == [0x4C, 2, 10, 0x55]
            && code[5..9] == [0x4B, 75, 4, 0x53]
            && code[4].checked_add(4) == Some(code[9]),
        "Unsupported source reticle projection"
    );
    let projection = Projection {
        matrix: code[4],
        offset: code[9],
        depth: code[9]
            .checked_add(1)
            .context("Reticle depth row overflow")?,
        vectors: values.len(),
        framing: framing.map(|framing| framing.tangent),
    };
    ensure!(
        values.len() > usize::from(projection.depth),
        "Source reticle projection buffer is truncated"
    );
    context.constants = vectors(material, 0x88)?;
    context.authored.clear();
    let live = BTreeSet::from([u32::from(projection.depth)]);
    let lowered = context.lower(&code[10..], values, &live, true)?;
    ensure!(
        lowered.resources.is_empty() && lowered.samplers.is_empty(),
        "Reticle vertex requires a resource"
    );
    let nominal = lowered
        .constants
        .get(&usize::from(projection.depth))
        .context("Source reticle has no finite nominal focal plane")?;
    let nominal = f32::from_le_bytes(nominal[..4].try_into()?);
    ensure!(
        nominal.is_finite() && nominal > 0.,
        "Source reticle focal plane is not positive"
    );
    let target = focal_plane(native, shell)?;
    ensure!(
        lowered.code.ends_with(&[0x43, projection.depth]),
        "Reticle depth output is not final"
    );
    // The native reticle projection takes its horizontal view from the private optic's angle at
    // `+80`, which carries the source sight's angular override, so the source depth gives the
    // artwork its source screen size. A framed sight projects with the source view instead.
    let mut code = vec![
        0x3E,
        2,
        6,
        0x44,
        projection.matrix,
        0x3D,
        73,
        4,
        0x43,
        projection.offset,
    ];
    code.extend(lowered.code);
    let mut values = values.to_vec();
    let policy = match projection.framing {
        Some(_) => "Source depth in the source aimed view",
        None => "Source depth in the optic's aimed view",
    };
    values[usize::from(projection.depth)] = resources::vector_bytes([nominal; 4]);
    Ok(Vertex {
        projection,
        plane: json!({"source_nominal":nominal,"native_nominal":target,
            "aimed_view_tangent":projection.framing,"policy":policy}),
        code,
        constants: context.constants,
        values,
        objects: u32::try_from(context.objects.len())?,
    })
}

fn focal_plane(native: &mut Reader, shell: &Payload) -> Result<f32> {
    let code = shell
        .array(0x68, 1, None)?
        .into_iter()
        .map(|at| shell.u8(at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        code.len() == 10
            && code[..4] == [0x3E, 2, 6, 0x44]
            && code[5..9] == [0x3D, 73, 4, 0x43]
            && code[4].checked_add(4) == Some(code[9]),
        "Native reticle projection producer differs"
    );
    let depth = code[9]
        .checked_add(1)
        .context("Native reticle depth overflow")?;
    let shader = native.tag(native.reference(shell.u32(0x48)?)?, None)?;
    ensure!(
        Inspection::read(Stage::Vertex, &shader.0)?
            .inputs
            .constant_reads
            .get(&0)
            .is_some_and(|reads| reads
                .iter()
                .any(|r| r.index == Some(u32::from(depth)) && r.lanes.contains('x'))),
        "Native reticle shader has no focal-plane consumer"
    );
    let values = resources::values(native, shell, 0x48)?;
    let row = values
        .get(usize::from(depth))
        .context("Native reticle focal plane is absent")?;
    let value = f32::from_le_bytes(row[..4].try_into()?);
    ensure!(
        value.is_finite() && value > 0.,
        "Native reticle focal plane is invalid"
    );
    Ok(value)
}

/// Aimed glass draws in the reticle pass and projects as a native reticle does. The template's
/// own producer fills the reticle rows, and its focal plane gives the glass its depth.
pub(super) fn native_vertex(native: &mut Reader, shell: &Payload) -> Result<Vertex> {
    focal_plane(native, shell)?;
    let code = shell
        .array(0x68, 1, None)?
        .into_iter()
        .map(|at| shell.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let values = resources::values(native, shell, 0x48)?;
    Ok(Vertex {
        projection: Projection {
            matrix: code[4],
            offset: code[9],
            depth: code[9] + 1,
            vectors: values.len(),
            framing: None,
        },
        plane: Value::Null,
        code,
        constants: Vec::new(),
        values,
        objects: 0,
    })
}

impl Vertex {
    pub(super) fn append(self, bytes: &mut Vec<u8>) -> Result<()> {
        append(bytes, 0x68, 0x80800009, &self.code, 1)?;
        append(
            bytes,
            0x78,
            0x80800090,
            &self.constants.into_iter().flatten().collect::<Vec<_>>(),
            16,
        )?;
        append(
            bytes,
            0x98,
            0x80800090,
            &self.values.into_iter().flatten().collect::<Vec<_>>(),
            16,
        )?;
        put(bytes, 0xB8, &self.objects.to_le_bytes())?;
        bytes[0xBC] = 0x10;
        put(bytes, 0xC8, &0u32.to_le_bytes())
    }
}
