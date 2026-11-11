use super::*;

pub(super) const HEIGHT: usize = 35;
pub(super) const SCALE: usize = 36;

/// The source offsets transformed height-map coordinates, then returns to base UV space.
/// Its interpolated mode chooses between a crossing interpolation and a second full step.
pub(super) fn coordinates(
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
    notes: &mut Vec<String>,
) -> Result<String> {
    let option = surface
        .material
        .options
        .get("parallax")
        .map(String::as_str)
        .or_else(|| {
            (surface.material.options.get("warp").map(String::as_str) == Some("parallax_simple"))
                .then_some("simple")
        });
    if matches!(option, None | Some("off")) {
        return Ok(String::new());
    }
    ensure!(
        matches!(option, Some("simple" | "interpolated" | "simple_detail")),
        "Unsupported surface parallax mode {option:?}"
    );
    let sample = samples
        .get(&HEIGHT)
        .context("Parallax height texture is absent")?;
    let (map, sampler, uv) = surface::sample_parts(sample).context("Parallax height sample")?;
    let mapping = surface
        .material
        .mapping(&["height_map"])
        .context("Parallax height transform")?;
    let origin = animation::coordinates(mapping, "float2(0,0)", notes);
    let corner = animation::coordinates(mapping, "float2(1,1)", notes);
    let height = surface.scalar("height_scale", 0.);
    let mut source = format!(
        "float3 parallaxView=mul(float3x3(v1.xyz,v2.xyz,v0.xyz),normalize(v5.xyz));float2 parallaxOrigin={origin};float2 parallaxScale=({corner})-parallaxOrigin;float2 parallaxUv={uv};float parallaxHeight=({map}.Sample({sampler},parallaxUv).g-0.5)*{height};"
    );
    match option {
        Some("simple") => source.push_str("parallaxView.z=max(parallaxView.z,0.8);parallaxView=normalize(parallaxView);parallaxUv+=parallaxView.xy*parallaxHeight/parallaxView.z;"),
        Some("simple_detail") => {
            let scale = samples.get(&SCALE).context("Parallax scale texture is absent")?;
            source.push_str(&format!("parallaxUv+=parallaxHeight*({scale}).g*parallaxView.xy;"));
        }
        Some("interpolated") => source.push_str(&format!("float2 parallaxStep=parallaxHeight*parallaxView.xy;float parallaxCurrent=parallaxHeight*parallaxView.z;float parallaxSecond=({map}.Sample({sampler},parallaxUv+parallaxStep).g-0.5)*{height};float parallaxDelta=parallaxSecond-parallaxCurrent;if(sign(parallaxDelta)!=sign(parallaxHeight-parallaxCurrent)){{parallaxUv+=parallaxStep*(parallaxHeight/(parallaxCurrent-parallaxSecond+parallaxHeight));}}else{{parallaxUv+=parallaxStep+parallaxDelta*parallaxView.xy;}}")),
        _ => unreachable!(),
    }
    // A zero scale makes the source inverse undefined. Keep the original coordinate there.
    source.push_str(
        "if(all(abs(parallaxScale)>1e-8)){v3.xy=(parallaxUv-parallaxOrigin)/parallaxScale;}",
    );
    Ok(source)
}
