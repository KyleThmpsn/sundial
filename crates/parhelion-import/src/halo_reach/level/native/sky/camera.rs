//! Scenario camera effects with per-parameter global defaults. MCC U13's `matg` reference is
//! at +0x558, unlike the July 2011 Xbox layout. The selected values describe the source camera,
//! not a replacement for Shadowkeep's view-dependent exposure or bloom passes.
use crate::halo_reach::cache::{Cache, Tag};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

const SCENARIO_EFFECTS: usize = 0x6A8;
const GLOBAL_EFFECTS: usize = 0x558;
const USE_DEFAULT: u16 = 1;
const AUTOMATIC: u16 = 4;
/// Mid-gray albedo used by the opaque-surface approximation of very wide bloom.
const SCENE_AVERAGE: f32 = 0.18;

pub(super) struct Camera {
    /// The exposure scale, `2^stops`, a clamped manual target or an automatic range's midpoint.
    pub shown: f32,
    /// The screen brightness automatic exposure aims for, `2^stops`.
    pub brightness: f32,
    pub automatic: bool,
    /// The manual target and the exposure range, in stops.
    pub target: f32,
    pub range: [f32; 2],
    /// The preferred self illumination exposure in stops and the share of the main exposure it
    /// follows ([`crate::halo_reach::native::material::self_illumination_exposure`]).
    pub self_illumination: [f32; 2],
    pub veil: [f32; 3],
    pub report: Value,
}

struct Settings<'a> {
    cache: &'a Cache,
    scenario: Option<Tag>,
    defaults: Tag,
}

impl Settings<'_> {
    /// Reach selects the first parameter whose Use Default bit is clear
    /// (`choose_valid_parameter`, `82595E20` in the Xbox `halo3_tag_play` build). The scenario's
    /// whole exposure block is one parameter, while each bloom strength and color is independent.
    fn parameter(&self, offset: usize) -> Result<(&Tag, usize)> {
        if let Some(tag) = &self.scenario {
            let at = tag.address()? + offset;
            if self.cache.u16(at)? & USE_DEFAULT == 0 {
                return Ok((tag, at));
            }
        }
        let at = self.defaults.address()? + offset;
        ensure!(
            self.cache.u16(at)? & USE_DEFAULT == 0,
            "Global camera effects {} leave parameter {offset:#x} unresolved",
            self.defaults.path
        );
        Ok((&self.defaults, at))
    }

    fn values<const N: usize>(&self, offset: usize) -> Result<([f32; N], Value)> {
        let (tag, at) = self.parameter(offset)?;
        let values = self.cache.floats::<N>(at + 4)?;
        ensure!(
            values.iter().all(|value| value.is_finite()),
            "Camera effects {} hold a nonfinite parameter at {offset:#x}",
            tag.path
        );
        Ok((
            values,
            json!({"source": tag.path, "values": values.as_slice()}),
        ))
    }
}

pub(super) fn read(cache: &Cache) -> Result<Camera> {
    let scenario = cache.only("scnr")?.address()?;
    let globals = cache.only("matg")?.address()?;
    let settings = Settings {
        cache,
        scenario: cache.reference(scenario + SCENARIO_EFFECTS, Some("cfxs"))?,
        defaults: cache
            .reference(globals + GLOBAL_EFFECTS, Some("cfxs"))?
            .context("Global camera effects are missing")?,
    };
    let (exposure, at) = settings.parameter(0)?;
    let automatic = cache.u16(at)? & AUTOMATIC != 0;
    let target = cache.f32(at + 4)?;
    let [low, high] = cache.floats::<2>(at + 0x10)?;
    let brightness = cache.f32(at + 0x18)?;
    ensure!(
        [target, low, high, brightness]
            .iter()
            .all(|value| value.is_finite())
            && low <= high,
        "Camera effects {} hold an invalid exposure range",
        exposure.path
    );
    // This estimate only sets the no-lightmap fallback for the average lit level. The sky's white
    // clamp follows the frame's exposure ([`SkyScale::white`]). Native
    // automatic adaptation follows its own meter within the bounds [`NativeExposure`] sets.
    let stops = if automatic {
        low * 0.5 + high * 0.5
    } else {
        target.clamp(low, high)
    };
    let shown = stops.exp2();
    let screen = brightness.exp2();
    ensure!(
        shown.is_finite() && shown > 0. && screen.is_finite() && screen > 0.,
        "Camera effects {} exceed the supported exposure scale",
        exposure.path
    );
    let mut bloom = serde_json::Map::new();
    let mut strengths = [0.; 3];
    for (index, (name, offset)) in [("highlight", 0x28), ("inherent", 0x38), ("intensity", 0x48)]
        .into_iter()
        .enumerate()
    {
        let ([value, maximum_change, blend_speed], mut report) = settings.values::<3>(offset)?;
        ensure!(value >= 0., "Negative camera bloom {name}");
        strengths[index] = value;
        report["value"] = json!(value);
        report["maximum_change"] = json!(maximum_change);
        report["blend_speed"] = json!(blend_speed);
        bloom.insert(name.to_owned(), report);
    }
    let mut large = [0.; 3];
    for (name, offset) in [("large", 0x58), ("medium", 0x68), ("small", 0x78)] {
        let (color, report) = settings.values::<3>(offset)?;
        ensure!(
            color.iter().all(|value| *value >= 0.),
            "Negative camera bloom {name} color"
        );
        if name == "large" {
            large = color;
        }
        bloom.insert(name.to_owned(), report);
    }
    let [_, inherent, intensity] = strengths;
    let veil = large.map(|value| value * inherent * intensity * SCENE_AVERAGE);
    ensure!(
        veil.iter().all(|value| value.is_finite()),
        "Camera bloom floor overflows"
    );
    let preferred = settings.values::<3>(0xBC)?;
    let change = settings.values::<3>(0xCC)?;
    let report = json!({
        "camera_fx": settings.scenario.as_ref().map(|tag| &tag.path),
        "default_camera_fx": settings.defaults.path,
        "exposure_source": exposure.path,
        "automatic": automatic,
        "target_stops": target,
        "range_stops": [low, high],
        "screen_brightness_stops": brightness,
        "exposure_stops": stops,
        "exposure_estimate": if automatic {
            "Range midpoint for sky normalization only"
        } else {
            "Clamped manual target"
        },
        "maximum_change": cache.f32(at + 8)?,
        "blend_speed": cache.f32(at + 0xC)?,
        "delay": cache.f32(at + 0x1C)?,
        "sensitivity": settings.values::<1>(0x20)?.1,
        "bloom": bloom,
        "bloom_floor": veil,
        "bloom_method": "Opaque-surface average-color approximation. Native bloom passes remain unchanged.",
        "self_illumination": preferred.1,
        "self_illumination_change": change.1,
    });
    Ok(Camera {
        shown,
        brightness: screen,
        automatic,
        target,
        range: [low, high],
        self_illumination: [preferred.0[0], change.0[0]],
        veil,
        report,
    })
}
