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
    };
    ensure!(
        values.len() > usize::from(projection.depth),
        "Source reticle projection buffer is truncated"
    );
    context.constants = vectors(material, 0x88)?;
    context.authored.clear();
    let live = BTreeSet::from([u32::from(projection.depth)]);
    let mut lowered = context.lower(&code[10..], values, &live, true)?;
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
    let factor = target / nominal;
    ensure!(
        factor.is_finite() && factor > 0.,
        "Reticle focal-plane conversion is invalid"
    );
    ensure!(
        lowered.code.ends_with(&[0x43, projection.depth]),
        "Reticle depth output is not final"
    );
    // The native reticle projection uses its own virtual focal plane. Preserve the source
    // response relative to its nominal depth instead of importing a much nearer plane, which
    // enlarges all procedural artwork. The native template supplies this rendering baseline.
    let coefficient = u8::try_from(context.constants.len()).context("Reticle constant capacity")?;
    context.constants.push(resources::vector_bytes([factor; 4]));
    lowered.code.truncate(lowered.code.len() - 2);
    lowered
        .code
        .extend([0x34, coefficient, 0x04, 0x43, projection.depth]);
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
    values[usize::from(projection.depth)] = resources::vector_bytes([target; 4]);
    Ok(Vertex {
        projection,
        plane: json!({"source_nominal":nominal,"native_nominal":target,"scale":factor,
            "policy":"Native virtual focal-plane baseline with relative source depth response"}),
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
