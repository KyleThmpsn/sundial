//! Constant-term irradiance used by emissive and ribbon materials.
use super::*;

pub(super) fn replace(
    lines: &mut Vec<String>,
    start: usize,
    samples: &[usize],
    shadow: usize,
) -> Result<()> {
    ensure!(samples.len() == 3, "ambient coefficient count differs");
    let mut vector = None;
    for (channel, &at) in ['x', 'y', 'z'].into_iter().zip(samples) {
        let (destination, expression) = assignment(&lines[at]).context("ambient coefficient")?;
        let name = destination
            .strip_suffix(&format!(".{channel}"))
            .context("ambient coefficient channels differ")?;
        ensure!(
            expression.ends_with(").w"),
            "ambient coefficient is not the constant term"
        );
        ensure!(
            vector.is_none_or(|previous| previous == name),
            "ambient coefficient vectors differ"
        );
        vector = Some(name);
    }
    let vector = vector.context("ambient coefficient vector")?;
    let clamp = unique(lines, &format!("= max(float3(0,0,0), {vector}.xyz);"))?;
    ensure!(
        start < samples[0] && samples[2] < clamp && clamp < shadow,
        "ambient lighting equation order differs"
    );
    let (color, _) = assignment(&lines[clamp]).context("ambient clamp output")?;
    let color = color
        .strip_suffix(".xyz")
        .context("ambient color channels differ")?
        .to_owned();
    let shadow_result = assignment(lines.get(shadow + 2).context("shadow blend extent")?)
        .context("shadow blend destination")?
        .0
        .to_owned();

    // Keep the authored sunlight exponent and scale. Require that this block
    // only reads its own temporaries and constants, not removed clipmap state.
    let mut assigned = Components::new();
    for line in &lines[clamp + 1..shadow] {
        let (left, right) = assignment(line).context("ambient sunlight control flow")?;
        ensure!(
            references(right).is_subset(&assigned)
                && !right.contains("cb3[")
                && !right.contains("Sample"),
            "ambient sunlight depends on removed clipmap state"
        );
        assigned.extend(references(left));
    }
    let mut replacement = ['x', 'y', 'z'].into_iter().enumerate().map(|(index, channel)| {
        format!("  {color}.{channel} = t{}.SampleLevel(s5_s, cb8[4].zw * float2(1,0.5) + float2(0,0.5), 0).w;", 28 + index)
    }).collect::<Vec<_>>();
    replacement.push(format!("  {color}.xyz = max(float3(0,0,0), {color}.xyz);"));
    replacement.extend_from_slice(&lines[clamp + 1..shadow]);
    replacement.push(format!(
        "  {shadow_result} = t31.SampleLevel(s4_s, cb8[4].zw, 0).x;"
    ));
    replace_region(lines, start, shadow + 3, replacement)
}

/// Recognize the paired RGB/directional fog reconstruction. An unrecognized
/// use of these coefficients must not silently choose either interpretation.
pub(super) fn colors_fog(text: &str) -> Result<bool> {
    let lines = text.lines().collect::<Vec<_>>();
    let candidates = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("cb8[5].xyz") || line.contains("cb8[6].xyz"))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(false);
    }
    ensure!(
        candidates.len() == 2 && candidates[1].0 == candidates[0].0 + 1,
        "fog color coefficient uses differ"
    );
    let (destination, first) = assignment(candidates[0].1).context("fog color reconstruction")?;
    let sample = first
        .strip_prefix("cb8[5].xyz * ")
        .and_then(|value| value.strip_suffix(".xyz"))
        .context("fog RGB coefficient expression differs")?;
    ensure!(
        destination.ends_with(".xyz")
            && references(sample).is_empty()
            && sample.len() > 1
            && sample.starts_with('r')
            && sample[1..].chars().all(|c| c.is_ascii_digit()),
        "fog sample register differs"
    );
    let (second_destination, second) =
        assignment(candidates[1].1).context("directional fog reconstruction")?;
    ensure!(
        second_destination == destination
            && second == format!("{sample}.www * cb8[6].xyz + {destination}"),
        "directional fog coefficient expression differs"
    );
    Ok(true)
}
