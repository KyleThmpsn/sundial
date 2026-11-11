use super::cache::{Cache, Tag};
use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
pub struct Branch {
    pub model: Option<Tag>,
    pub animation: Option<Tag>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Child {
    pub parent_marker: String,
    pub child_marker: String,
    pub variant: String,
    pub object: Option<Tag>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Variant {
    pub name: String,
    pub regions: BTreeMap<String, Vec<Permutation>>,
    pub children: Vec<Child>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Permutation {
    pub name: String,
    pub runtime_index: i8,
    pub flags: u8,
    pub probability: f32,
    pub states: Vec<State>,
    pub runtime_states: [i8; 5],
}
#[derive(Clone, Debug, Serialize)]
pub struct State {
    pub name: String,
    pub runtime_index: i8,
    pub flags: u8,
    /// Source state ordinal from intact through destroyed, inclusive.
    pub state: i16,
    pub probability: f32,
}

/// Select one deterministic source variant. Equal-weight hidden alternatives can
/// have different damage coverage, so retain the stateful alternative on a tie.
pub(super) fn select_permutation<'a>(
    permutations: &'a [Permutation],
    choice: Option<&str>,
) -> Option<(usize, &'a Permutation)> {
    let matches = |p: &Permutation| {
        choice.is_none_or(|name| {
            if p.runtime_index < 0 {
                name.is_empty()
            } else {
                name == p.name
            }
        })
    };
    permutations
        .iter()
        .enumerate()
        .filter(|(_, p)| matches(p) && p.probability > 0.)
        .max_by(|(_, a), (_, b)| {
            a.probability
                .total_cmp(&b.probability)
                .then_with(|| (a.runtime_index >= 0).cmp(&(b.runtime_index >= 0)))
                .then_with(|| a.states.len().cmp(&b.states.len()))
        })
        .or_else(|| permutations.iter().enumerate().find(|(_, p)| matches(p)))
}

#[derive(Clone, Debug, Serialize)]
pub struct Object {
    pub tag: Tag,
    pub default_variant: String,
    pub model: Option<Tag>,
    pub world: Option<Branch>,
    pub first_person: Vec<Branch>,
    pub variants: Vec<Variant>,
    pub attachments: Vec<Value>,
    pub functions: Vec<Value>,
    pub gameplay: Value,
}

impl Object {
    pub fn read(cache: &Cache, tag: &Tag) -> Result<Self> {
        Self::read_with(cache, tag, true)
    }

    /// Reads `tag` for how it looks: its models, variants, attachments and functions, without the
    /// rigid body and damage a crate or vehicle carries, which an object drawn where it stands in a
    /// level never uses.
    pub fn read_appearance(cache: &Cache, tag: &Tag) -> Result<Self> {
        Self::read_with(cache, tag, false)
    }

    fn read_with(cache: &Cache, tag: &Tag, physics: bool) -> Result<Self> {
        // Every group here begins with the object definition this reads, the devices (machines and
        // controls) and effect scenery included.
        ensure!(
            [
                "weap", "vehi", "scen", "bloc", "eqip", "proj", "mach", "ctrl", "efsc"
            ]
            .contains(&tag.group.as_str()),
            "Unsupported source object {}",
            tag.group
        );
        let at = tag.address()?;
        cache.meta(at, 0x168)?;
        let model = cache.reference(at + 0x64, Some("hlmt"))?;
        let mut world = None;
        let mut variants = Vec::new();
        if let Some(model) = &model {
            let p = model.address()?;
            world = Some(Branch {
                model: cache.reference(p, Some("mode"))?,
                animation: cache.reference(p + 0x20, Some("jmad"))?,
            });
            for row in cache.block(p + 0x84, 0x38)? {
                let name = cache.string_id(cache.u32(row)?)?;
                let mut regions = BTreeMap::new();
                for region in cache.block(row + 0x14, 0x18)? {
                    let region_name = cache.string_id(cache.u32(region)?)?;
                    let mut permutations = Vec::new();
                    for perm in cache.block(region + 8, 0x24)? {
                        permutations.push(Permutation {
                            name: cache.string_id(cache.u32(perm)?)?,
                            runtime_index: cache.u8(perm + 4)? as i8,
                            flags: cache.u8(perm + 5)?,
                            probability: cache.f32(perm + 8)?,
                            states: cache
                                .block(perm + 12, 12)?
                                .into_iter()
                                .map(|s| {
                                    let state = cache.i16(s + 6)?;
                                    let probability = cache.f32(s + 8)?;
                                    ensure!(
                                        (0..=4).contains(&state)
                                            && probability.is_finite()
                                            && (0.0..=1.0).contains(&probability),
                                        "Invalid source model damage state"
                                    );
                                    Ok(State {
                                        name: cache.string_id(cache.u32(s)?)?,
                                        runtime_index: cache.u8(s + 4)? as i8,
                                        flags: cache.u8(s + 5)?,
                                        state,
                                        probability,
                                    })
                                })
                                .collect::<Result<_>>()?,
                            runtime_states: cache
                                .bytes(perm + 0x18, 5)?
                                .try_into()
                                .map(|states: [u8; 5]| states.map(|value| value as i8))?,
                        });
                    }
                    ensure!(
                        regions.insert(region_name, permutations).is_none(),
                        "Duplicate variant region"
                    );
                }
                let children = cache
                    .block(row + 0x20, 0x20)?
                    .into_iter()
                    .map(|r| {
                        Ok(Child {
                            parent_marker: cache.string_id(cache.u32(r)?)?,
                            child_marker: cache.string_id(cache.u32(r + 4)?)?,
                            variant: cache.string_id(cache.u32(r + 8)?)?,
                            object: cache.reference(r + 12, None)?,
                        })
                    })
                    .collect::<Result<_>>()?;
                variants.push(Variant {
                    name,
                    regions,
                    children,
                });
            }
        }
        let mut first_person = Vec::new();
        let mut gameplay = match tag.group.as_str() {
            "weap" => {
                cache.meta(at, 0x4b8)?;
                for row in cache.block(at + 0x3b8, 32)? {
                    first_person.push(Branch {
                        model: cache.reference(row, Some("mode"))?,
                        animation: cache.reference(row + 16, Some("jmad"))?,
                    });
                }
                let magazines = cache.block(at+0x3f0, 96)?.into_iter().map(|r| Ok(json!({
                    "initial":cache.i16(r+6)?, "total":cache.i16(r+8)?, "loaded":cache.i16(r+10)?, "reloaded":cache.i16(r+14)?
                }))).collect::<Result<Vec<_>>>()?;
                let barrels = cache.block(at+0x408, 0x184)?.into_iter().map(|r| Ok(json!({
                    "flags":cache.u32(r)?, "rounds_per_second":cache.floats::<2>(r+4)?,
                    "acceleration_seconds":cache.f32(r+0x10)?,
                    "raw":hex::encode(cache.meta(r, 0x40)?),
                    "shots_per_fire":[cache.i16(r+0x24)?,cache.i16(r+0x26)?], "fire_recovery_seconds":cache.f32(r+0x28)?,
                    "magazine":cache.i16(r+0x38)?, "rounds_per_shot":cache.i16(r+0x3a)?,
                    "marker":cache.string_id(cache.u32(r+0x40)?)?, "projectiles_per_shot":cache.i16(r+0x6a)?,
                    "distribution_angle_radians":cache.f32(r+0x6c)?, "error_angle_radians":cache.floats::<2>(r+0x74)?,
                    "distribution_function":cache.i16(r+0x68)?, "minimum_error_radians":cache.f32(r+0x70)?,
                    "error_deceleration_seconds":cache.f32(r+0x50)?, "accuracy_penalties":cache.floats::<4>(r+0x7c)?,
                    "error_penalty_functions":[cache.block(r+0x8c, 0x14)?.len(), cache.block(r+0x98, 0x14)?.len(),
                        cache.block(r+0xa4, 0x14)?.len(), cache.block(r+0xb0, 0x14)?.len()],
                    "projectile":cache.reference(r+0x104, Some("proj"))?, "secondary_projectile":cache.reference(r+0x114, Some("proj"))?,
                    "projectile_damage_range":damage_range(cache, r+0x104)?,
                    "heat_per_round":cache.f32(r+0x150)?, "heat_function":cache.string_id(cache.u32(r+0x154)?)?,
                    "age_per_round":cache.f32(r+0x158)?, "campaign_age_per_round":cache.f32(r+0x15c)?
                }))).collect::<Result<Vec<_>>>()?;
                let triggers = cache.block(at+0x3fc, 0x8c)?.into_iter().map(|r| Ok(json!({
                    "flags":cache.u32(r)?, "input":cache.i16(r+4)?, "behavior":cache.i16(r+6)?,
                    "primary_barrel":cache.i16(r+8)?, "secondary_barrel":cache.i16(r+10)?, "prediction":cache.i16(r+0xc)?,
                    "autofire_seconds":cache.f32(r+0x10)?, "secondary_action":cache.i16(r+0x18)?, "primary_action":cache.i16(r+0x1a)?,
                    "charging_seconds":cache.f32(r+0x1c)?, "charged_seconds":cache.f32(r+0x20)?,
                    "overcharged_action":cache.u8(r+0x24)?, "charge_flags":cache.u8(r+0x25)?,
                    "charged_drain_per_second":cache.f32(r+0x5c)?,
                    "fire_fractions":cache.block(r+0x80, 4)?.into_iter().map(|f| cache.f32(f)).collect::<Result<Vec<_>>>()?
                }))).collect::<Result<Vec<_>>>()?;
                json!({"kind":"weapon", "weapon_class":cache.string_id(cache.u32(at+0x3ac)?)?,
                    "weapon_name":cache.string_id(cache.u32(at+0x3b0)?)?, "weapon_type":cache.i16(at+0x3b4)?,
                    "handle_node":cache.string_id(cache.u32(at+0x3a8)?)?, "flags":cache.u32(at+0x204)?,
                    "ready_seconds":cache.f32(at+0x214)?, "zoom_levels":cache.u16(at+0x2ca)?,
                    "magnification":cache.floats::<2>(at+0x2cc)?,
                    "secondary_flags":cache.u32(at+0x208)?, "aim_assist":aim_assist(cache, at)?,
                    "target_tracking":target_tracking(cache, at)?,
                    "heat":heat(cache, at)?, "age":age(cache, at)?,
                    "magazines":magazines,"barrels":barrels,"triggers":triggers})
            }
            "vehi" => vehicle(cache, at)?,
            _ => json!({"kind":"attachment"}),
        };
        if physics
            && matches!(tag.group.as_str(), "vehi" | "bloc")
            && let Some(model) = &model
        {
            gameplay["rigid_body"] = super::vehicle::body(cache, model)?;
            gameplay["damage"] = super::damage::read(cache, model)?;
        }
        let attachments = cache.block(at+0x104, 32)?.into_iter().map(|r| Ok(json!({
            "tag":cache.reference(r,None)?, "marker":cache.string_id(cache.u32(r+16)?)?,
            "primary_scale":cache.string_id(cache.u32(r+24)?)?, "secondary_scale":cache.string_id(cache.u32(r+28)?)?
        }))).collect::<Result<Vec<_>>>()?;
        let functions = cache.block(at + 0xec, 0x40)?.into_iter().map(|r| {
            let interpolation = cache.block(r + 0x30, 0x18)?.into_iter().map(|p| Ok(json!({
                "mode":cache.u32(p)?,"travel_seconds":cache.f32(p+4)?,
                "acceleration":cache.f32(p+8)?,"spring_k":cache.f32(p+12)?,
                "spring_c":cache.f32(p+16)?,"fraction":cache.f32(p+20)?
            }))).collect::<Result<Vec<_>>>()?;
            Ok(json!({"flags":cache.u32(r)?,"input":cache.string_id(cache.u32(r+4)?)?,
                "output":cache.string_id(cache.u32(r+8)?)?,"turn_off_with":cache.string_id(cache.u32(r+12)?)?,
                "ranged":cache.string_id(cache.u32(r+16)?)?,"minimum":cache.f32(r+20)?,
                "mapping":super::vehicle::function(cache,r+24)?,"scale_by":cache.string_id(cache.u32(r+44)?)?,
                "interpolation":interpolation}))
        }).collect::<Result<Vec<_>>>()?;
        Ok(Self {
            tag: tag.clone(),
            default_variant: cache.string_id(cache.u32(at + 0x60)?)?,
            model,
            world,
            first_person,
            variants,
            attachments,
            functions,
            gameplay,
        })
    }
}

/// The weapon's heat (`+0x238..+0x26C`): the heat it overheats above and recovers below, the heat
/// it loses each second while idle and while overheated, the object functions and venting that
/// replace those rates, and the external heat at `+0x474`.
fn heat(c: &Cache, at: usize) -> Result<Value> {
    Ok(
        json!({"recovery_threshold":c.f32(at+0x238)?,"overheated_threshold":c.f32(at+0x23c)?,
        "detonation_threshold":c.f32(at+0x240)?,"detonation_fraction":c.f32(at+0x244)?,
        "loss_per_second":c.f32(at+0x248)?,"loss_function":c.string_id(c.u32(at+0x24c)?)?,
        "venting_loss_function":c.string_id(c.u32(at+0x250)?)?,"venting_seconds":c.f32(at+0x254)?,
        "venting_exit_heat":c.f32(at+0x258)?,"illumination":c.f32(at+0x25c)?,
        "warning_threshold":c.f32(at+0x260)?,"overheated_loss_per_second":c.f32(at+0x264)?,
        "overheated_loss_function":c.string_id(c.u32(at+0x268)?)?,"external_heat":c.f32(at+0x474)?}),
    )
}

/// The weapon's aim assist (`+0x2D4..+0x2F4`): the autoaim and magnetism angles, in radians, and
/// their full-strength, falloff and near falloff ranges, in world units, then the deviation angle a
/// shot may leave the barrel by.
fn aim_assist(c: &Cache, at: usize) -> Result<Value> {
    let [
        autoaim_angle,
        autoaim_range,
        autoaim_falloff,
        autoaim_near,
        magnetism_angle,
        magnetism_range,
        magnetism_falloff,
        magnetism_near,
        deviation,
    ] = c.floats::<9>(at + 0x2d4)?;
    Ok(
        json!({"autoaim_angle_radians":autoaim_angle,"autoaim_range":autoaim_range,
        "autoaim_falloff_range":autoaim_falloff,"autoaim_near_falloff_range":autoaim_near,
        "magnetism_angle_radians":magnetism_angle,"magnetism_range":magnetism_range,
        "magnetism_falloff_range":magnetism_falloff,"magnetism_near_falloff_range":magnetism_near,
        "deviation_angle_radians":deviation}),
    )
}

/// The weapon's target tracking (`+0x310`, rows of `0x38`): the kinds of target it locks on to,
/// a string list at `+0x0`, the seconds a lock takes to acquire (`+0xC`), lasts once the target
/// leaves (`+0x10`) and takes to decay after that (`+0x14`), and its tracking and locked sounds
/// (`+0x18`, `+0x28`).
fn target_tracking(c: &Cache, at: usize) -> Result<Value> {
    let rows = c
        .block(at + 0x310, 0x38)?
        .into_iter()
        .map(|row| {
            let types = c
                .block(row, 4)?
                .into_iter()
                .map(|entry| c.string_id(c.u32(entry)?))
                .collect::<Result<Vec<_>>>()?;
            Ok(json!({"types":types,"acquire_seconds":c.f32(row + 0xc)?,
                "grace_seconds":c.f32(row + 0x10)?,"decay_seconds":c.f32(row + 0x14)?,
                "tracking_sound":c.reference(row + 0x18, None)?,
                "locked_sound":c.reference(row + 0x28, None)?}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Value::from(rows))
}

/// The weapon's age (`+0x364..+0x370`): the heat recovery and rate of fire penalties it takes as
/// it ages, the age it starts to misfire from and the chance per shot at full age, and the age a
/// use adds when the weapon is not fired (`+0x46C`, `+0x470` in campaign).
fn age(c: &Cache, at: usize) -> Result<Value> {
    Ok(
        json!({"heat_recovery_penalty":c.f32(at+0x364)?,"rate_of_fire_penalty":c.f32(at+0x368)?,
        "misfire_start":c.f32(at+0x36c)?,"misfire_chance":c.f32(at+0x370)?,
        "external_aging":c.f32(at+0x46c)?,"campaign_external_aging":c.f32(at+0x470)?}),
    )
}

/// The air and water ranges, in world units, over which the projectile a barrel names at
/// `reference` scales its damage (`proj +0x2BC` and `+0x2C8`).
fn damage_range(c: &Cache, reference: usize) -> Result<Value> {
    let Some(at) = c
        .reference(reference, Some("proj"))?
        .and_then(|projectile| projectile.address().ok())
    else {
        return Ok(Value::Null);
    };
    c.meta(at, 0x2d0)?;
    Ok(json!({"air":c.floats::<2>(at+0x2bc)?,"water":c.floats::<2>(at+0x2c8)?}))
}

/// A unit camera (`s_unit_camera`, `0x78` bytes), as a seat holds it at `+0x70` and a unit at
/// `+0x198`: flags, the marker it pivots on, pitch auto-level and range, the pitch springs and
/// spring velocity, and the field of view override, all angles in radians. Each `0x20` track row
/// names a `trak` at `+0x0`, and a screen effect this reader leaves out at `+0x10`. A `trak` holds
/// its flags at `+0x0` and its `0x1C` control points at `+0x4`, each a position offset from the
/// pivot in world units at `+0x0` and a view rotation at `+0xC`.
fn camera(c: &Cache, at: usize) -> Result<Value> {
    c.meta(at, 0x78)?;
    let tracks = c
        .block(at + 0x14, 0x20)?
        .into_iter()
        .map(|row| {
            let track = c.reference(row, Some("trak"))?;
            let points = match track.as_ref().and_then(|tag| tag.offset) {
                Some(trak) => {
                    c.meta(trak, 0x10)?;
                    c.block(trak + 4, 0x1c)?
                        .into_iter()
                        .map(|point| {
                            Ok(json!({"position":c.floats::<3>(point)?,
                                "orientation":c.floats::<4>(point + 0xc)?}))
                        })
                        .collect::<Result<Vec<_>>>()?
                }
                None => Vec::new(),
            };
            Ok(json!({"track":track,"control_points":points}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"flags":c.u16(at)?,"marker":c.string_id(c.u32(at + 4)?)?,
        "pitch_auto_level_radians":c.f32(at + 8)?,"pitch_range_radians":c.floats::<2>(at + 0xc)?,
        "tracks":tracks,"pitch_spring_radians":c.floats::<2>(at + 0x20)?,
        "spring_velocity_radians":c.f32(at + 0x28)?,"override_fov_radians":c.f32(at + 0x38)?}),
    )
}

fn vehicle(c: &Cache, at: usize) -> Result<Value> {
    c.meta(at, 0x648)?;
    let weapons = c
        .block(at + 0x42c, 20)?
        .into_iter()
        .map(|r| {
            Ok(json!({"tag":c.reference(r, Some("weap"))?,"variant":c.string_id(c.u32(r+16)?)?}))
        })
        .collect::<Result<Vec<_>>>()?;
    let seats = c.block(at+0x444, 0x12c)?.into_iter().map(|r| Ok(json!({
        "flags":c.u32(r)?, "animation":c.string_id(c.u32(r+4)?)?, "marker":c.string_id(c.u32(r+8)?)?,
        "entry_marker":c.string_id(c.u32(r+12)?)?, "camera_marker":c.string_id(c.u32(r+0x74)?)?,
        "ai_type":c.i16(r+0x3c)?, "yaw_radians":c.floats::<2>(r+0xf8)?, "camera":camera(c, r+0x70)?
    }))).collect::<Result<Vec<_>>>()?;
    let mut physics = Vec::new();
    for (i, (name, stride)) in [
        ("human_tank", 0x70),
        ("human_jeep", 0x58),
        ("human_plane", 0x4c),
        ("wolverine", 0x60),
        ("alien_scout", 0x74),
        ("alien_fighter", 0x64),
        ("turret", 0x14),
        ("mantis", 0xac),
        ("vtol", 0xd0),
        ("chopper", 0x7c),
        ("guardian", 0x30),
        ("jackal_glider", 0x170),
        ("boat", 0x60),
        ("space_fighter", 0xc4),
        ("revenant", 0xa8),
    ]
    .into_iter()
    .enumerate()
    {
        for row in c.block(at + 0x4d0 + i * 12, stride)? {
            let mut p = json!({"kind":name,"metadata_offset":row});
            p.as_object_mut().unwrap().extend(
                super::vehicle::controls(c, name, row)?
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            if let Some(speed) = match name {
                "alien_scout" => Some(8),
                "alien_fighter" => Some(0x14),
                _ => None,
            } {
                p["forward_source_units_per_second"] = json!(c.f32(row + speed)?);
                p["reverse_source_units_per_second"] = json!(c.f32(row + speed + 4)?);
                p["acceleration_source_units_per_second_squared"] = json!(c.f32(row + speed + 8)?);
            }
            // The turret block names the nodes its yaw and pitch physically turn, at `+0x4`
            // and `+0x8`.
            if name == "turret" {
                p["physical_yaw_node"] = json!(c.string_id(c.u32(row + 4)?)?);
                p["physical_pitch_node"] = json!(c.string_id(c.u32(row + 8)?)?);
            }
            physics.push(p);
        }
    }
    Ok(
        json!({"kind":"vehicle","weapons":weapons,"seats":seats,"camera":camera(c, at + 0x198)?,"physics":physics,
        "flags":c.u32(at+0x4cc)?,"tricks":super::vehicle::tricks(c,at)?,
        "contacts":super::vehicle::contacts(c,at)?,
        "gravity_scale":c.f32(at+0x5ac)?,"boost_peak_power":c.f32(at+0x480)?,
        "boost_recharge_rate":c.f32(at+0x490)?,"boost_recharge_delay_seconds":c.f32(at+0x494)?}),
    )
}
