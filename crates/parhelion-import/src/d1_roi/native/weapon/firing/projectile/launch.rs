//! The source movement's launch speed and distance curve on the private carrier movement.
//!
//! Both builds keep a launch speed multiplier and an optional distance curve in the movement
//! definition, with copies in its instance. Rise of Iron `808020AC` holds the multiplier at
//! definition `+98`, which its launch consumer reads from instance `+134`, and the curve, class
//! `80802336`, through the relative reference at definition `+D8`, with its state at instance
//! `+13C` and the scale after it. Shadowkeep `8080388F` holds them at `+88` and instance `+144`,
//! and its curve `80803803` at `+C8`, with state at instance `+14C` and the scale at `+15C`. The
//! curve holds final speed, final gravity, curve start and curve end. The multiplier scales the
//! speed the firing Barrel supplies, and the final speed scales its final launch input.
//!
//! A private carrier fires from the base weapon's Barrel, whose launch inputs were tuned for
//! movements holding the source's values, while a carrier from another weapon's content row may
//! hold its own. Napalm carrier `80BBD01A`, which no stock weapon fires, launches at 25 times the
//! Barrel speed and ends its curve at 65 times, where Rise of Iron's Dragon's Breath and the base's
//! rocket `80BBD121` hold 1 and 1. The private movement therefore takes the source's multiplier and
//! curve, as it takes response counts.
use super::*;
use sundial::package_authoring::sandbox_perk::program::NativeAssetResourcePatch;

/// Field offsets of one build: multiplier in the definition and its instance copy, curve
/// reference, curve class and curve state in the instance.
struct Layout {
    speed: usize,
    speed_state: usize,
    curve: usize,
    curve_class: u32,
    curve_state: usize,
}

const SOURCE: Layout = Layout {
    speed: 0x98,
    speed_state: 0x134,
    curve: 0xD8,
    curve_class: 0x8080_2336,
    curve_state: 0x13C,
};
const NATIVE: Layout = Layout {
    speed: 0x88,
    speed_state: 0x144,
    curve: 0xC8,
    curve_class: 0x8080_3803,
    curve_state: 0x14C,
};
/// An interval narrower than this takes a scale of one, as native reset `487FAD` derives it.
const NARROW_INTERVAL: f32 = 0.0001;

struct Launch {
    speed: f32,
    /// The curve settings object's offset and its four settings.
    curve: Option<(usize, [f32; 4])>,
}

fn read(owner: &graph::Owner, layout: &Layout) -> Result<Launch> {
    let p = &owner.payload;
    let (d, i) = (owner.definition, owner.instance);
    let speed = p.f32(d + layout.speed)?;
    ensure!(
        speed.is_finite()
            && speed > 0.
            && p.u32(d + layout.speed)? == p.u32(i + layout.speed_state)?,
        "Projectile launch speed and its instance copy disagree"
    );
    let curve = if p.u64(d + layout.curve)? == 0 {
        None
    } else {
        let at = p.pointer(d + layout.curve)?;
        ensure!(
            at >= 4 && p.u32(at - 4)? == layout.curve_class,
            "Projectile speed curve class differs"
        );
        let mut settings = [0f32; 4];
        for (k, value) in settings.iter_mut().enumerate() {
            *value = p.f32(at + 4 * k)?;
            ensure!(
                value.is_finite() && p.u32(at + 4 * k)? == p.u32(i + layout.curve_state + 4 * k)?,
                "Projectile speed curve and its instance state disagree"
            );
        }
        Some((at, settings))
    };
    Ok(Launch { speed, curve })
}

/// The distance scale native reset derives from a curve's ends, clamped to 0 through 10000.
fn scale(start: f32, end: f32) -> f32 {
    let interval = end - start;
    if interval.abs() < NARROW_INTERVAL {
        1.
    } else {
        (1. / interval).clamp(0., 10000.)
    }
}

/// Patches giving the private carrier movement the source's launch speed and curve, addressed from
/// the carrier movement's instance, and their report.
pub(super) fn patches(
    source: &graph::Owner,
    carrier: &graph::Owner,
) -> Result<(Vec<Value>, Value)> {
    let from = read(source, &SOURCE)?;
    let to = read(carrier, &NATIVE)?;
    let p = &carrier.payload;
    let mut patches = Vec::new();
    let mut patch = |at: usize, bytes: Vec<u8>| -> Result<()> {
        let expected =
            p.0.get(at..at + bytes.len())
                .context("Projectile launch field extent")?
                .to_vec();
        if expected != bytes {
            patches.push(serde_json::to_value(NativeAssetResourcePatch {
                binding_hash: MOVEMENT,
                resource_index: 0,
                offset: u32::try_from(
                    at.checked_sub(carrier.instance)
                        .context("Launch field precedes movement resource")?,
                )?,
                expected,
                bytes,
                imported_particle: None,
            })?);
        }
        Ok(())
    };
    let speed = from.speed.to_le_bytes().to_vec();
    patch(carrier.definition + NATIVE.speed, speed.clone())?;
    patch(carrier.instance + NATIVE.speed_state, speed)?;
    let curve = match (from.curve, to.curve) {
        (None, None) => None,
        (Some((_, settings)), Some((at, _))) => {
            let bytes = settings
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>();
            patch(at, bytes.clone())?;
            let distance = scale(settings[2], settings[3]);
            let mut state = bytes;
            state.extend(distance.to_le_bytes());
            patch(carrier.instance + NATIVE.curve_state, state)?;
            Some(distance)
        }
        _ => anyhow::bail!("Projectile speed curve presence differs"),
    };
    let report = json!({"source_speed":from.speed,"carrier_speed":to.speed,
        "source_curve":from.curve.map(|(_, settings)| settings),
        "carrier_curve":to.curve.map(|(_, settings)| settings),"curve_scale":curve,
        "patches":patches.len()});
    Ok((patches, report))
}
