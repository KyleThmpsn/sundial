//! Source behavior identities and typed links. No Destiny behavior is inferred from labels.
use super::{
    cache::{Cache, Tag},
    object::Object,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Reach Material Response 8, Airstrike.
const AIRSTRIKE: i64 = 8;

struct Read<'a> {
    cache: &'a Cache,
    queue: VecDeque<Tag>,
    edges: Vec<Value>,
    owner: u32,
}
impl Read<'_> {
    fn link(&mut self, at: usize, role: &str) -> Result<()> {
        if let Some(tag) = self.cache.reference(at, None)? {
            self.edges
                .push(json!({"owner":self.owner,"role":role,"tag":tag,"offset":at}));
            if [
                "weap", "vehi", "bloc", "proj", "effe", "prt3", "snd!", "lsnd", "jmad", "frms",
                "jpt!",
            ]
            .contains(&tag.group.as_str())
            {
                self.queue.push_back(tag);
            }
        }
        Ok(())
    }
    /// Records the reference at `at` without following it.
    fn record(&mut self, at: usize, role: &str) -> Result<()> {
        if let Some(tag) = self.cache.reference(at, None)? {
            self.edges
                .push(json!({"owner":self.owner,"role":role,"tag":tag,"offset":at}));
        }
        Ok(())
    }
    fn links(&mut self, at: usize, fields: &[(usize, &str)]) -> Result<()> {
        for &(offset, role) in fields {
            self.link(at + offset, role)?;
        }
        Ok(())
    }
    fn block_links(&mut self, at: usize, stride: usize, fields: &[(usize, &str)]) -> Result<()> {
        for row in self.cache.block(at, stride)? {
            self.links(row, fields)?;
        }
        Ok(())
    }
    fn object(&mut self, tag: &Tag) -> Result<Value> {
        let c = self.cache;
        let object = Object::read(c, tag)?;
        let at = tag.address()?;
        self.links(
            at,
            &[
                (0x84, "collision_damage"),
                (0x94, "brittle_collision_damage"),
                (0xb0, "creation"),
                (0xc0, "material_response"),
                (0xd0, "melee"),
            ],
        )?;
        self.block_links(at + 0x104, 32, &[(0, "attachment")])?;
        self.block_links(at + 0x128, 16, &[(0, "widget")])?;
        if let Some(model) = &object.model {
            for section in c.block(model.address()? + 0xe0, 0xc0)? {
                self.links(
                    section,
                    &[
                        (0x34, "recharge"),
                        (0x48, "pre_recharge"),
                        (0x5c, "recharge_abort"),
                    ],
                )?;
                self.block_links(
                    section + 0x84,
                    0xb0,
                    &[
                        (0xc, "damage_generic"),
                        (0x20, "damage_specific"),
                        (0x34, "damage_damage"),
                        (0x48, "damage_looping"),
                        (0x7c, "damage_delay"),
                    ],
                )?;
            }
        }
        self.block_links(
            at + 0x15c,
            0x30,
            &[
                (0, "multiplayer_spawn"),
                (0x10, "survival_spawn"),
                (0x20, "campaign_spawn"),
            ],
        )?;
        for branch in object.world.iter().chain(&object.first_person) {
            if let Some(animation) = &branch.animation {
                self.edges
                    .push(json!({"owner":tag.datum,"role":"animation","tag":animation}));
                self.queue.push_back(animation.clone());
            }
        }
        for variant in &object.variants {
            for child in &variant.children {
                if let Some(child) = &child.object {
                    self.edges.push(json!({"owner":tag.datum,"role":"variant_child","variant":variant.name,"tag":child}));
                    if matches!(child.group.as_str(), "weap" | "vehi") {
                        self.queue.push_back(child.clone());
                    }
                }
            }
        }
        if tag.group == "weap" {
            self.links(
                at,
                &[
                    (0x198, "item_detonation_damage"),
                    (0x1b0, "item_detonating"),
                    (0x1c0, "item_detonation"),
                    (0x218, "ready"),
                    (0x228, "ready_damage"),
                    (0x26c, "overheat"),
                    (0x27c, "overheat_damage"),
                    (0x28c, "detonation"),
                    (0x29c, "detonation_damage"),
                    (0x2b8, "clang"),
                    (0x344, "power_on"),
                    (0x354, "power_off"),
                    (0x374, "pickup"),
                    (0x384, "zoom_in"),
                    (0x394, "zoom_out"),
                    (0x42c, "deployed_vehicle"),
                    (0x43c, "tossed_weapon"),
                    (0x44c, "age"),
                    (0x45c, "aged_material_response"),
                ],
            )?;
            self.block_links(
                at + 0x2ac,
                0xc8,
                &[
                    (0x18, "melee_damage"),
                    (0x28, "melee_response"),
                    (0x38, "lunge_damage"),
                    (0x48, "lunge_response"),
                    (0x58, "empty_melee_damage"),
                    (0x68, "empty_melee_response"),
                    (0x78, "clang_damage"),
                    (0x88, "clang_response"),
                    (0x98, "weapon_clang_damage"),
                    (0xa8, "weapon_clang_response"),
                    (0xb8, "lunge_explosive_damage"),
                ],
            )?;
            self.block_links(
                at + 0x310,
                0x38,
                &[(0x18, "tracking_sound"), (0x28, "locked_sound")],
            )?;
            self.block_links(
                at + 0x3f0,
                96,
                &[
                    (0x14, "reload"),
                    (0x24, "reload_damage"),
                    (0x34, "chamber"),
                    (0x44, "chamber_damage"),
                ],
            )?;
            self.block_links(
                at + 0x3fc,
                0x8c,
                &[
                    (0x2c, "charge"),
                    (0x3c, "charge_damage"),
                    (0x4c, "charge_response"),
                    (0x60, "discharge"),
                    (0x70, "discharge_damage"),
                ],
            )?;
            for (barrel, row) in c.block(at + 0x408, 0x184)?.into_iter().enumerate() {
                let first = self.edges.len();
                self.links(
                    row,
                    &[
                        (0x104, "projectile"),
                        (0x114, "secondary_projectile"),
                        (0x124, "damage"),
                        (0x134, "crate_projectile"),
                    ],
                )?;
                self.block_links(
                    row + 0x178,
                    0xc4,
                    &[
                        (4, "fire"),
                        (0x14, "misfire"),
                        (0x24, "empty"),
                        (0x34, "secondary_fire"),
                        (0x44, "fire_damage"),
                        (0x54, "misfire_damage"),
                        (0x64, "empty_damage"),
                        (0x74, "secondary_fire_damage"),
                        (0x84, "fire_rider_response"),
                        (0x94, "misfire_rider_response"),
                        (0xa4, "empty_rider_response"),
                        (0xb4, "secondary_fire_rider_response"),
                    ],
                )?;
                // The barrel index is what a trigger's primary and secondary barrel fields name.
                for edge in &mut self.edges[first..] {
                    edge["barrel"] = json!(barrel);
                }
            }
        } else if tag.group == "vehi" {
            self.block_links(at + 0x42c, 20, &[(0, "vehicle_weapon")])?;
            self.links(
                at,
                &[
                    (0x288, "assassination_response"),
                    (0x298, "assassination_weapon"),
                    (0x354, "melee_damage"),
                    (0x364, "melee_override"),
                    (0x374, "boarding_damage"),
                    (0x384, "boarding_response"),
                    (0x394, "eviction_damage"),
                    (0x3a4, "eviction_response"),
                    (0x3b4, "landing_damage"),
                    (0x3c4, "flurry_damage"),
                    (0x3d4, "obstacle_damage"),
                    (0x3e4, "assassination_damage"),
                    (0x45c, "emp_disabled"),
                    (0x46c, "boost_collision_damage"),
                    (0x4ac, "exit_damage"),
                    (0x4bc, "exit_weapon"),
                    (0x608, "suspension_sound"),
                    (0x618, "special_effect"),
                    (0x628, "driver_boost_damage"),
                    (0x638, "rider_boost_damage"),
                ],
            )?;
            self.block_links(
                at + 0x438,
                0x38,
                &[(0x18, "tracking_sound"), (0x28, "locked_sound")],
            )?;
            for (offset, stride, sound) in [
                (0x4d0, 0x70, 0x4c),
                (0x4dc, 0x58, 0x28),
                (0x4f4, 0x60, 0x28),
                (0x53c, 0x7c, 0x28),
                (0x560, 0x60, 0x34),
                (0x578, 0xa8, 0x4c),
            ] {
                self.block_links(at + offset, stride, &[(sound, "gear_shift")])?;
            }
        }
        Ok(json!({"object":object}))
    }
    fn projectile(&mut self, at: usize) -> Result<Value> {
        let c = self.cache;
        c.meta(at, 0x360)?;
        self.links(
            at,
            &[
                (0x64, "model"),
                (0xb0, "creation"),
                (0xc0, "material_response"),
                (0x1a8, "detonation_started"),
                (0x1b8, "airborne_detonation"),
                (0x1c8, "ground_detonation"),
                (0x1d8, "detonation_damage"),
                (0x1e8, "attached_damage"),
                (0x1f8, "super_detonation"),
                (0x208, "super_damage"),
                (0x218, "detonation_sound"),
                (0x22c, "attached_super_damage"),
                (0x240, "flyby_sound"),
                (0x250, "flyby_response"),
                (0x264, "impact"),
                (0x274, "object_impact"),
                (0x284, "impact_damage"),
                (0x298, "boarding_damage"),
                (0x2a8, "boarding_attached_damage"),
            ],
        )?;
        self.block_links(at + 0x104, 32, &[(0, "attachment")])?;
        // A projectile's contacts play the sounds its material effects name, so that tag joins
        // the closure. No translation reads another object's material effects, which stay edges.
        if let Some(tag) = c
            .reference(at + 0xc0, None)?
            .filter(|tag| tag.group == "foot")
        {
            self.queue.push_back(tag);
        }
        // Material Response rows: a conditional row then a fallback row per material.
        let responses = c
            .block(at + 0x320, 0x34)?
            .into_iter()
            .map(|r| {
                Ok(json!({
            "raw":hex::encode(c.meta(r,0x34)?),"material":c.string_id(c.u32(r)?)?,
            "material_index":c.i16(r+4)?,"flags":c.u16(r+6)?,"chance":c.f32(r+8)?,
            "angle_radians":c.floats::<2>(r+0xc)?,"velocity":c.floats::<2>(r+0x14)?,
            "response":c.i16(r+0x1c)?,"scale_effects_by":c.i16(r+0x1e)?,
            "angular_noise_radians":c.f32(r+0x20)?,"velocity_noise":c.f32(r+0x24)?,
            "initial_friction":c.f32(r+0x28)?,"parallel_friction":c.f32(r+0x2c)?,
            "perpendicular_friction":c.f32(r+0x30)?}))
            })
            .collect::<Result<Vec<_>>>()?;
        // An Airstrike row hands the contact to the scenario's airstrike, so its definition and the
        // bomb its fire effect creates join the closure.
        if responses.iter().any(|row| row["response"] == AIRSTRIKE) {
            self.airstrike()?;
        }
        let conical_spread = c.block(at + 0x344, 12)?.into_iter().map(|r| Ok(json!({"yaw_count":c.i16(r)?,"pitch_count":c.i16(r+2)?,"distribution_exponent":c.f32(r+4)?,"spread_radians":c.f32(r+8)?}))).collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"flags":c.u32(at+0x168)?,"timer_start":c.i16(at+0x16c)?,"collision_radius":c.f32(at+0x170)?,"arming_seconds":c.f32(at+0x174)?,"timer_seconds":c.floats::<2>(at+0x184)?,"minimum_velocity":c.f32(at+0x18c)?,"maximum_range":c.f32(at+0x190)?,"super_detonation_count":c.i16(at+0x1a2)?,"super_detonation_seconds":c.f32(at+0x1a4)?,"air_gravity_scale":c.f32(at+0x2b8)?,"water_gravity_scale":c.f32(at+0x2c4)?,"initial_velocity":c.f32(at+0x2d0)?,"final_velocity":c.f32(at+0x2d4)?,"guided_angular_velocity":c.floats::<2>(at+0x2f0)?,"guided_angular_velocity_at_rest":c.f32(at+0x2f8)?,"acceleration_range":c.floats::<2>(at+0x2fc)?,"acceleration_inverse_width":c.f32(at+0x304)?,"responses":responses,"conical_spread":conical_spread,"length_unit_metres":super::rig::METRES_PER_UNIT}),
        )
    }
    /// Material effects (`foot`), as `effect::material` reads them. Each response type's sound
    /// entries join the closure, so their recordings are exported. Its effect entries and every
    /// secondary reference are recorded without being followed: the contact translation reads its
    /// chosen effect from the cache, and a secondary reference has no established role. The sound
    /// parts of an effect entry that can serve every material join the closure too.
    fn material_effects(&mut self, at: usize) -> Result<Value> {
        let c = self.cache;
        for (index, row) in c.block(at, 0x24)?.into_iter().enumerate() {
            let first = self.edges.len();
            for entry in c.block(row + 0xc, 0x2c)? {
                self.link(entry, "material_sound")?;
                self.record(entry + 0x10, "material_secondary_sound")?;
            }
            for entry in c.block(row + 0x18, 0x2c)? {
                self.record(entry, "material_effect")?;
                self.record(entry + 0x10, "material_secondary_effect")?;
                // The contact host plays the sound parts of an effect that can serve every
                // material, one without a material name or for global material 0, so those
                // sounds join the closure and their recordings are exported. The effect stays
                // an edge.
                if !(matches!(c.u32(entry + 0x20)?, 0 | u32::MAX) || c.i16(entry + 0x24)? == 0) {
                    continue;
                }
                let Some(effect) = c.reference(entry, Some("effe"))? else {
                    continue;
                };
                for event in c.block(effect.address()? + 0x2c, 0x40)? {
                    for part in c.block(event + 0x1c, 0x64)? {
                        if c.reference(part + 0x14, None)?
                            .is_some_and(|tag| tag.group == "snd!")
                        {
                            self.link(part + 0x14, "material_effect_sound")?;
                        }
                    }
                }
            }
            // A projectile response selects its type from 15, Impact (Detonate).
            for edge in &mut self.edges[first..] {
                edge["material_type"] = json!(index);
            }
        }
        super::effect::material::read(c, at)
    }
    fn effect(&mut self, at: usize) -> Result<Value> {
        let c = self.cache;
        for row in c.block(at + 0x2c, 0x40)? {
            for part in c.block(row + 0x1c, 0x64)? {
                self.link(part + 0x14, "effect_part")?;
            }
            self.block_links(row + 0x34, 0x70, &[(4, "particle")])?;
        }
        self.block_links(at + 0x38, 0x14, &[(0, "looping_sound")])?;
        super::effect::source::read(c, at)
    }
    fn animation(&mut self, at: usize) -> Result<Value> {
        let c = self.cache;
        self.links(at, &[(0, "parent_animation"), (0x84, "frame_events")])?;
        self.block_links(at + 0x54, 20, &[(0, "animation_sound")])?;
        self.block_links(at + 0x60, 20, &[(0, "animation_effect")])?;
        let sounds = c
            .block(at + 0x54, 20)?
            .into_iter()
            .map(|row| c.reference(row, Some("snd!")))
            .collect::<Result<Vec<_>>>()?;
        let mut clips = Vec::new();
        for (index, row) in c.block(at + 0x94, 0x3c)?.into_iter().enumerate() {
            self.link(row + 0x1c, "shared_animation")?;
            let shared=c.block(row+0x30,0xd4)?.into_iter().map(|r| {
                let sounds=c.block(r+0x44,8)?.into_iter().map(|s|Ok(json!({"sound_index":c.i16(s)?,"frame":c.i16(s+2)?,"marker":c.string_id(c.u32(s+4)?)?}))).collect::<Result<Vec<_>>>()?;
                let events = c.block(r + 0x38, 4)?.into_iter()
                    .map(|event| Ok(json!({"kind":c.i16(event)?,"frame":c.i16(event + 2)?})))
                    .collect::<Result<Vec<_>>>()?;
                Ok(json!({"raw":hex::encode(c.meta(r,0xd4)?),"frames":c.i16(r+2)?,"animation_type":c.u8(r+4)?,"movement_type":c.u8(r+5)?,"resource_group":c.i16(r+0x20)?,"resource_member":c.i16(r+0x22)?,"sound_events":sounds,"frame_events":events}))
            }).collect::<Result<Vec<_>>>()?;
            clips.push(json!({"index":index,"name":c.string_id(c.u32(row)?)?,"raw":hex::encode(c.meta(row,0x3c)?),"shared_graph":c.reference(row+0x1c,Some("jmad"))?,"shared_index":c.i16(row+0x2c)?,"shared":shared}));
        }
        for offset in [0x128, 0x134] {
            self.block_links(at + offset, 0x2c, &[(0, "inherited_animation")])?;
        }
        let groups = c
            .block(at + 0x1ac, 12)?
            .into_iter()
            .map(|r| Ok(json!({"references":c.i32(r)?,"handle":c.u32(r+4)?})))
            .collect::<Result<Vec<_>>>()?;
        // Vehicle Suspension (`+0x110`, rows of `0x30`, Assembly Reach `jmad` schema): each wheel's
        // overlay animation by graph and animation index, a marker on the vehicle root, a contact
        // marker on the wheel, the wheel's offset along vehicle up and the ground depths at full
        // extension and full compression, then the same set for a destroyed region.
        let suspension = c
            .block(at + 0x110, 0x30)?
            .into_iter()
            .map(|r| {
                let name = |offset: usize| c.string_id(c.u32(r + offset)?);
                Ok(
                    json!({"label":name(0)?,"graph_index":c.i16(r+4)?,"animation_index":c.i16(r+6)?,
                    "function":name(8)?,"marker":name(0xc)?,"contact_marker":name(0x10)?,
                    "mass_point_offset":c.f32(r+0x14)?,"full_extension_ground_depth":c.f32(r+0x18)?,
                    "full_compression_ground_depth":c.f32(r+0x1c)?,
                    "destroyed":{"region":name(0x20)?,"mass_point_offset":c.f32(r+0x24)?,
                        "full_extension_ground_depth":c.f32(r+0x28)?,
                        "full_compression_ground_depth":c.f32(r+0x2c)?}}),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"clips":clips,"sounds":sounds,"resource_groups":groups,"vehicle_suspension":suspension,"playback":"See animations.json for decoded channels and supported source-scene playback"}),
        )
    }
    /// The airstrike definitions the cache's scenarios name at `scnr +0x2E0`, Airstrike Globals,
    /// as edges from the projectile whose contact calls them.
    fn airstrike(&mut self) -> Result<()> {
        let c = self.cache;
        for scenario in c.tags.iter().flatten().filter(|t| t.group == "scnr") {
            let Ok(at) = scenario.address() else {
                continue;
            };
            if let Some(tag) = c.reference(at + 0x2e0, Some("airs"))? {
                self.edges
                    .push(json!({"owner":self.owner,"role":"airstrike","tag":tag,
                    "scenario":scenario,"offset":at + 0x2e0}));
                self.queue.push_back(tag);
            }
        }
        Ok(())
    }
    /// An airstrike definition's battery rows, `0x4C` bytes after the HREK field order: launch
    /// radius, height, warmup, arrival, cooldown and duration, shots per launch and initial
    /// launches, the fire effect, the fire offsets and the request weapon. The fire effect joins
    /// the closure, and with it the projectile its part creates.
    fn airstrike_definition(&mut self, at: usize) -> Result<Value> {
        let c = self.cache;
        let mut batteries = Vec::new();
        for row in c.block(at, 0x4c)? {
            self.link(row + 0x20, "airstrike_fire_effect")?;
            self.record(row + 0x3c, "airstrike_request_weapon")?;
            let offsets = c
                .block(row + 0x30, 8)?
                .into_iter()
                .map(|offset| c.floats::<2>(offset))
                .collect::<Result<Vec<_>>>()?;
            batteries.push(json!({"raw":hex::encode(c.meta(row,0x4c)?),
                "launch_radius":c.f32(row)?,"launch_height":c.f32(row+4)?,
                "warmup_seconds":c.f32(row+8)?,"arrival_seconds":c.f32(row+0xc)?,
                "cooldown_seconds":c.f32(row+0x10)?,"duration_seconds":c.f32(row+0x14)?,
                "shots_per_launch":c.i32(row+0x18)?,"initial_launches":c.i32(row+0x1c)?,
                "fire_effect":c.reference(row+0x20,Some("effe"))?,"fire_offsets":offsets,
                "request_weapon":c.reference(row+0x3c,Some("weap"))?}));
        }
        Ok(json!({"batteries":batteries,"length_unit_metres":super::rig::METRES_PER_UNIT}))
    }
    fn damage(&mut self, at: usize) -> Result<Value> {
        let c = self.cache;
        self.links(at, &[(0xa4, "damage_response"), (0xb4, "melee_sound")])?;
        self.block_links(at + 0xc4, 0x14, &[(0, "damage_sound")])?;
        let size = usize::try_from(c.i32(at + 0x2c)?)?;
        ensure!(size <= 1024 * 1024, "Oversized damage falloff function");
        let falloff = if size == 0 {
            Vec::new()
        } else {
            c.meta(c.expand(c.u32(at + 0x38)?)?, size)?.to_vec()
        };
        Ok(
            json!({"raw":hex::encode(c.meta(at,0xe8)?),"radius":c.floats::<2>(at)?,
            "cutoff_scale":c.f32(at+8)?,"area_flags":c.u32(at+12)?,"side_effect":c.i16(at+0x10)?,
            "category":c.i16(at+0x12)?,"flags":c.u32(at+0x18)?,"core_radius":c.f32(at+0x1c)?,
            "lower_bound":c.f32(at+0x20)?,"upper_bound":c.floats::<2>(at+0x24)?,"falloff":hex::encode(falloff),
            "cone_radians":c.floats::<2>(at+0x40)?,"stun":c.f32(at+0x4c)?,"maximum_stun":c.f32(at+0x50)?,
            "stun_seconds":c.f32(at+0x54)?,"instantaneous_acceleration":c.f32(at+0x5c)?,
            "general_damage":c.string_id(c.u32(at+0x70)?)?,"specific_damage":c.string_id(c.u32(at+0x74)?)?,
            "custom_responses":c.block(at+0x78,4)?.into_iter().map(|r|c.string_id(c.u32(r)?)).collect::<Result<Vec<_>>>()?,
            "emp_radius":c.f32(at+0x94)?,"length_unit_metres":super::rig::METRES_PER_UNIT}),
        )
    }
}

pub(super) fn animation_data(cache: &Cache, tag: &Tag) -> Result<Value> {
    ensure!(tag.group == "jmad", "Expected an animation graph");
    Read {
        cache,
        queue: VecDeque::new(),
        edges: Vec::new(),
        owner: tag.datum,
    }
    .animation(tag.address()?)
}

fn pitch_parameters(c: &Cache, parameters: &[usize], index: i16) -> Result<Value> {
    if index == -1 {
        return Ok(Value::Null);
    }
    let parameter = *parameters
        .get(usize::try_from(index)?)
        .context("Sound pitch parameters outside gestalt")?;
    Ok(json!({"raw":hex::encode(c.meta(parameter,16)?),
        "natural_pitch_cents":c.i16(parameter)?,"word_1":c.i16(parameter+2)?,
        "bend_bounds_cents":[c.i16(parameter+4)?,c.i16(parameter+6)?],
        "maximum_gain_pitch_bounds_cents":[c.i16(parameter+8)?,c.i16(parameter+10)?],
        "playback_pitch_bounds_cents":[c.i16(parameter+12)?,c.i16(parameter+14)?]}))
}

fn pitch_name(c: &Cache, names: &[usize], index: i16) -> Result<Value> {
    if index == -1 {
        return Ok(Value::Null);
    }
    let name = *names
        .get(usize::try_from(index)?)
        .context("Sound pitch name outside gestalt")?;
    Ok(json!(c.string_id(c.u32(name)?)?))
}

pub(super) fn sound(c: &Cache, tag: &Tag) -> Result<Value> {
    let at = tag.address()?;
    let gestalt = c.only("ugh!")?.address()?;
    let class_tag = c.only("sncl")?;
    let classes = c.block(class_tag.address()?, 0xd4)?;
    let class = *classes
        .get(usize::from(c.u8(at + 2)?))
        .context("Sound class outside global table")?;
    let limits = json!({"source":class_tag,"maximum_per_tag":c.i16(class)?,
        "maximum_per_object_per_tag":c.i16(class+2)?,"maximum_per_class":c.i16(class+4)?,
        "maximum_per_object_per_class":c.i16(class+6)?,"preemption_ms":c.i32(class+8)?,
        "internal_flags":c.u16(class+12)?,"flags":c.u16(class+14)?});
    let pitches = c.block(gestalt + 0x4c, 12)?;
    let permutations = c.block(gestalt + 0x58, 24)?;
    let names = c.block(gestalt + 0x28, 4)?;
    let parameters = c.block(gestalt + 0x40, 16)?;
    let first = usize::try_from(c.i16(at + 6)?)?;
    let count = usize::from(c.u8(at + 3)?);
    ensure!(
        first <= pitches.len() && count <= pitches.len() - first,
        "Sound pitch range outside gestalt"
    );
    let mut ranges = Vec::new();
    for &row in &pitches[first..first + count] {
        let start = usize::from(c.u16(row + 8)?);
        let count = usize::from((c.u16(row + 10)? >> 4) & 63);
        ensure!(
            start <= permutations.len() && count <= permutations.len() - start,
            "Sound permutations outside gestalt"
        );
        let mut variants = Vec::new();
        for &r in &permutations[start..start + count] {
            variants.push(json!({"fsb_info":c.u32(r+20)?,"source_samples":c.u32(r+4)?,"skip_fraction_encoded":c.i16(r+2)?,"gain_encoded":c.u8(r+14)? as i8,"raw":hex::encode(c.meta(r,24)?)}));
        }
        ranges.push(
            json!({"raw":hex::encode(c.meta(row,12)?),"variants":variants,
            "name":pitch_name(c,&names,c.i16(row)?)?,
            "parameters":pitch_parameters(c,&parameters,c.i16(row+2)?)?}),
        );
    }
    let playback = c.i16(at + 14)?;
    let playback = if playback < 0 {
        Value::Null
    } else {
        let rows = c.block(gestalt + 0x10, 0x54)?;
        let r = *rows
            .get(playback as usize)
            .context("Sound playback outside gestalt")?;
        json!({"raw":hex::encode(c.meta(r,0x54)?),"skip_fraction":c.f32(r+4)?,"gain_db":c.f32(r+0x2c)?,"gain_variance_db":c.f32(r+0x30)?,"pitch_bounds":[c.i16(r+0x34)?,c.i16(r+0x36)?],"first_person_gain_db":c.f32(r+0x50)?})
    };
    Ok(
        json!({"class":c.u8(at+2)?,"flags":c.u16(at)?,"bank_suffix":c.string_id(c.u32(at+0x30)?)?,"pitch_ranges":ranges,"playback":playback,
            "playback_limits":limits,"promotion_index":c.u8(at+0x12)? as i8,"custom_playback_index":c.u8(at+0x13)? as i8}),
    )
}

pub(super) fn read(cache: &Cache, scene: &super::Scene) -> Result<Value> {
    let object = &scene.object;
    let mut reader = Read {
        cache,
        queue: VecDeque::from([object.tag.clone()]),
        edges: Vec::new(),
        owner: 0,
    };
    // Selected attachments can be scenery or equipment rather than weapons or vehicles.
    // Their validated model branches still own animation graphs and timed sound links.
    for placed in &scene.models {
        if let Some(animation) = &placed.animation {
            reader.edges.push(json!({"owner":placed.model.tag.datum,
                "role":"selected_model_animation","tag":animation}));
            reader.queue.push_back(animation.clone());
        }
    }
    let mut nodes = BTreeMap::new();
    let mut seen = BTreeSet::new();
    while let Some(tag) = reader.queue.pop_front() {
        if !seen.insert(tag.datum) {
            continue;
        }
        ensure!(
            seen.len() <= 16384,
            "Behavior dependency closure exceeds limit"
        );
        crate::cancellation::check()?;
        reader.owner = tag.datum;
        let at = tag.address()?;
        let data = match tag.group.as_str() {
            "weap" | "vehi" | "bloc" => reader.object(&tag)?,
            "proj" => reader.projectile(at)?,
            "effe" => reader.effect(at)?,
            "prt3" => super::effect::source::particle(cache, &tag)?,
            "snd!" => sound(cache, &tag)?,
            "jmad" => reader.animation(at)?,
            "lsnd" => {
                reader.block_links(
                    at + 0x20,
                    0xb0,
                    &[
                        (0xc, "loop_start"),
                        (0x1c, "loop"),
                        (0x2c, "loop_end"),
                        (0x3c, "alternate_loop"),
                        (0x4c, "alternate_end"),
                        (0x5c, "transition_in"),
                        (0x6c, "transition_out"),
                    ],
                )?;
                reader.block_links(at + 0x2c, 0x48, &[(4, "detail")])?;
                let tracks = cache.block(at + 0x20, 0xb0)?;
                let controls = tracks.iter().map(|&r| {
                    let mut track = json!({"name":cache.string_id(cache.u32(r)?)?,
                        "flags":cache.u16(r+4)?,"output_effect":cache.u16(r+6)?,"gain_db":cache.f32(r+8)?,
                        "layer":cache.u16(r+0x9c)?,"layer_flags":cache.u16(r+0x9e)?});
                    for (name,offset) in [("start",0xc),("loop",0x1c),("end",0x2c),("alternate_loop",0x3c),
                        ("alternate_end",0x4c),("transition_in",0x5c),("transition_out",0x6c)] {
                        track[name] = serde_json::to_value(cache.reference(r+offset,Some("snd!"))?)?;
                    }
                    for (name,offset) in [("fade_in",0x7c),("fade_out",0x84),("alternate_crossfade",0x8c),
                        ("alternate_fade_out",0x94),("layer_fade_in",0xa0),("layer_fade_out",0xa8)] {
                        track[name] = json!({"seconds":cache.f32(r+offset)?,"mode":cache.u32(r+offset+4)?});
                    }
                    Ok(track)
                }).collect::<Result<Vec<_>>>()?;
                json!({"flags":cache.u32(at)?,"distance_bounds_source":cache.floats::<2>(at+0xc)?,
                    "class":cache.u16(at+0x18)?,"master_marker_track":cache.string_id(cache.u32(at+0x1c)?)?,
                    "tracks":tracks.into_iter().map(|r|cache.meta(r,0xb0).map(hex::encode)).collect::<Result<Vec<_>>>()?,
                    "track_controls":controls})
            }
            "frms" => {
                reader.block_links(at, 20, &[(0, "animation_sound")])?;
                reader.block_links(at + 12, 20, &[(0, "animation_effect")])?;
                json!({"raw":hex::encode(cache.meta(at,0x24)?)})
            }
            "jpt!" => reader.damage(at)?,
            "airs" => reader.airstrike_definition(at)?,
            "foot" => reader.material_effects(at)?,
            _ => unreachable!(),
        };
        nodes.insert(format!("{:08X}", tag.datum), json!({"tag":tag,"data":data}));
    }
    Ok(json!({"root":object.tag,"nodes":nodes,"edges":reader.edges,"gameplay_verified":false}))
}
