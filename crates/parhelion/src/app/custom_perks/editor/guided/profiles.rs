//! Allowlisted gameplay mappings, not guesses based on a scalar's type or value.
//!
//! A new profile needs native path/owner proof, units and default values, a compiler
//! round-trip test, and a controlled gameplay observation of that effect path.
//! Structurally similar weapon-owned projectiles must not become private-perk
//! controls without proving that the selected private action owns their path.

pub(super) struct ProjectileProfile {
    pub perk_index: u16,
    pub id: &'static str,
    pub perk: &'static str,
    pub action_tag: u32,
    pub graph_tag: u32,
    pub owner_tag: u32,
    pub default_multiplier: f32,
    pub evidence: &'static str,
}

pub(super) const PROJECTILE_PROFILES: &[ProjectileProfile] = &[ProjectileProfile {
    perk_index: 1178,
    id: "micro_missile.projectile_speed",
    perk: "Micro-Missile",
    action_tag: 0x80BC_2BBD,
    graph_tag: 0x8152_82E1,
    owner_tag: 0x8152_82E7,
    default_multiplier: 1.0,
    evidence: "Verified with private Micro-Missile launch captures: both instance and definition speed fields must agree. The multiplier acts on the weapon's launch speed, not an absolute metres-per-second value. Other weapon combinations still need gameplay testing. No gameplay-safe maximum has been established.",
}];
