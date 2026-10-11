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
    pub probability: f32,
    pub states: Vec<String>,
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
    pub gameplay: Value,
}

impl Object {
    pub fn read(cache: &Cache, tag: &Tag) -> Result<Self> {
        ensure!(
            ["weap", "vehi", "scen", "bloc", "eqip", "proj"].contains(&tag.group.as_str()),
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
                            probability: cache.f32(perm + 8)?,
                            states: cache
                                .block(perm + 12, 12)?
                                .into_iter()
                                .map(|s| cache.string_id(cache.u32(s)?))
                                .collect::<Result<_>>()?,
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
        let gameplay = match tag.group.as_str() {
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
                    "shots_per_fire":[cache.i16(r+0x24)?,cache.i16(r+0x26)?], "fire_recovery_seconds":cache.f32(r+0x28)?,
                    "magazine":cache.i16(r+0x38)?, "rounds_per_shot":cache.i16(r+0x3a)?,
                    "marker":cache.string_id(cache.u32(r+0x40)?)?, "projectiles_per_shot":cache.i16(r+0x6a)?,
                    "distribution_angle_radians":cache.f32(r+0x6c)?, "error_angle_radians":cache.floats::<2>(r+0x74)?,
                    "projectile":cache.reference(r+0x104, Some("proj"))?, "secondary_projectile":cache.reference(r+0x114, Some("proj"))?,
                    "heat_per_round":cache.f32(r+0x150)?
                }))).collect::<Result<Vec<_>>>()?;
                let triggers = cache.block(at+0x3fc, 0x8c)?.into_iter().map(|r| Ok(json!({
                    "flags":cache.u32(r)?, "input":cache.i16(r+4)?, "behavior":cache.i16(r+6)?,
                    "primary_barrel":cache.i16(r+8)?, "secondary_barrel":cache.i16(r+10)?,
                    "autofire_seconds":cache.f32(r+0x10)?, "charging_seconds":cache.f32(r+0x1c)?, "charged_seconds":cache.f32(r+0x20)?
                }))).collect::<Result<Vec<_>>>()?;
                json!({"kind":"weapon", "weapon_class":cache.string_id(cache.u32(at+0x3ac)?)?,
                    "weapon_name":cache.string_id(cache.u32(at+0x3b0)?)?, "weapon_type":cache.i16(at+0x3b4)?,
                    "handle_node":cache.string_id(cache.u32(at+0x3a8)?)?, "flags":cache.u32(at+0x204)?,
                    "ready_seconds":cache.f32(at+0x214)?, "magnification":cache.floats::<2>(at+0x2cc)?,
                    "magazines":magazines,"barrels":barrels,"triggers":triggers})
            }
            "vehi" => vehicle(cache, at)?,
            _ => json!({"kind":"attachment"}),
        };
        let attachments = cache.block(at+0x104, 32)?.into_iter().map(|r| Ok(json!({
            "tag":cache.reference(r,None)?, "marker":cache.string_id(cache.u32(r+16)?)?,
            "primary_scale":cache.string_id(cache.u32(r+24)?)?, "secondary_scale":cache.string_id(cache.u32(r+28)?)?
        }))).collect::<Result<Vec<_>>>()?;
        Ok(Self {
            tag: tag.clone(),
            default_variant: cache.string_id(cache.u32(at + 0x60)?)?,
            model,
            world,
            first_person,
            variants,
            attachments,
            gameplay,
        })
    }
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
        "ai_type":c.i16(r+0x3c)?, "yaw_radians":c.floats::<2>(r+0xf8)?
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
            if let Some(speed) = match name {
                "alien_scout" => Some(8),
                "alien_fighter" => Some(0x14),
                _ => None,
            } {
                p["forward_source_units_per_second"] = json!(c.f32(row + speed)?);
                p["reverse_source_units_per_second"] = json!(c.f32(row + speed + 4)?);
                p["acceleration_source_units_per_second_squared"] = json!(c.f32(row + speed + 8)?);
            }
            physics.push(p);
        }
    }
    Ok(
        json!({"kind":"vehicle","weapons":weapons,"seats":seats,"physics":physics,
        "gravity_scale":c.f32(at+0x5ac)?,"boost_peak_power":c.f32(at+0x480)?,
        "boost_recharge_rate":c.f32(at+0x490)?,"boost_recharge_delay_seconds":c.f32(at+0x494)?}),
    )
}
