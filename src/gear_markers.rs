//! The named points a weapon's gear art carries: where it is held, where it fires, where a
//! case leaves it.
//!
//! A marker set is one component of a gear entity. Each row holds a position and the FNV-1
//! hash of the marker's name. The runtime resolves markers by that name, which is why an
//! imported model that keeps its donor's marker set aims at the donor's sight.
//!
//! The names themselves are not stored. Those below were recovered by generating candidates
//! from a weapon vocabulary and keeping only hashes exactly one candidate reached, the same
//! method and the same caution as the runtime member names. A recovered name identifies a
//! marker consistently. It is not evidence of what it is for.
use crate::{
    package_authoring::resolve_live_named_tag,
    package_payload::*,
    package_runtime::{index_cache, parallel, reader::PackageManager},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
mod shards;

const GEAR_ART_TABLE: u32 = 0x8080_5DF5;
const ARRANGEMENT_ROW: u32 = 0x8080_5DFB;
const ARRANGEMENT_SLOTS: u32 = 0x8080_5DFE;
const ARRANGEMENT_KEYS: u32 = 0x8080_5E01;
const ASSIGNMENT_MAP: u32 = 0x8080_56EA;
const ASSIGNMENT_ROW: u32 = 0x8080_56EC;
const RELATION: u32 = 0x8080_744A;
const ENTITY: u32 = 0x8080_9C0F;
const COMPONENT_ROW: u32 = 0x8080_9C04;
/// The marker-set component, "Query Markers", and the row class of the array inside it.
const MARKER_COMPONENT: u32 = 0x8080_8506;
const MARKER_ROW: u32 = 0x8080_8513;
/// The marker array's descriptor, relative to the component's data struct.
const MARKER_DESCRIPTOR: usize = 0xB0;
const MARKER_STRIDE: usize = 64;
/// A row is a rotation, then a position, then the name. Two rows may share a name and a
/// position and differ only in rotation, which is how a weapon aims one point two ways.
const MARKER_ORIENTATION: usize = 0x10;
const MARKER_POSITION: usize = 0x20;
const MARKER_NAME: usize = 0x30;
/// An appearance draws few objects, and each carries at most a handful of marker sets.
const MAX_ENTITIES: usize = 64;

/// Recovered marker names, by the FNV-1 hash the gear art stores.
///
/// Every entry hashes to its key, which `every_recovered_name_hashes_to_its_key` enforces.
/// That catches a typo. It does **not** make a name right: any string can be offered against
/// any hash, so what matters is how many strings were offered against how many hashes. Before
/// believing a batch, compute `candidates * targets / 2^32`. If that approaches the number of
/// hits, the batch is noise, and two searches were thrown out on exactly that arithmetic.
///
/// Most of the game's markers turned out to belong to characters rather than to gear. The
/// widest unnamed hashes sit on Cabal, Fallen and vendor patterns, which is why the vocabulary
/// that found them is anatomy and rigging rather than gunsmithing.
///
/// 799 of the game's 4,621 marker names are resolved, which is 43% of every marker row in it.
///
/// Six kinds of evidence put a name here.
///
/// A **search against the marker's own carriers**. This is the strongest, and it is what
/// finally settled `head`, `body` and `fx_center`. The packages name many objects with an
/// authored content path, so a marker can be searched against only the words of the objects
/// that actually carry it. That keeps the space per target to a few thousand strings, and 68
/// million candidates spread one-per-target came to 0.016 expected collisions across the whole
/// run. The same strings a wide search could not justify are safe here.
///
/// A **bounded search**. The run that found most of this table offered 2.4 million strings
/// against about 4,000 open hashes, for 8.2 expected collisions against 548 hits, so roughly
/// 98.5% of those were right on their own, and each kept one needed a family or a pair behind
/// it as well.
///
/// A **low-byte family**. Names differing only in their final character produce hashes
/// differing only in their low byte, because FNV-1 ends each byte with `hash * prime ^ byte`.
/// Two *independently generated* names landing in one such family corroborate each other: for
/// the second to be a collision it would have to fall inside the few thousand hashes the game
/// uses, about one in a million. `fx_vent_01` to `fx_vent_05` and `marker01` to `marker20`
/// came back whole this way.
///
/// An **opposite pair**. A name whose counterpart under left/right, upper/lower or
/// primary/secondary is also present is corroborated by it, since both halves being collisions
/// is that one in a million squared. This carried `left_hand` with `right_hand` and
/// `primary_weapon` with `secondary_weapon`.
///
/// A **literal string in the packages**. Scanning every structured resource found `grip`,
/// `primary_fire`, `fx_1`, `fx_2` and `fx_3` written out, the last three having been derived
/// here first and found verbatim afterwards. `support` was found this way and nowhere else;
/// 200,000 decoy hashes through the same scan put a meaningful word landing by chance at about
/// one in six thousand.
///
/// An **authored content-path word**. The TFT index supplies 28,826 whole segments and short
/// contiguous underscore spans from installed paths. Only 0.031 accidental matches are
/// expected against every marker hash, while 67 were found, including 12 already established
/// names. The other 55 are recorded in [`PATH_NAMES`].
///
/// A lone hit from a wide search is never enough on its own. `marker` is a real hash in the
/// game that only a wide search has reached, so it is not claimed. Other once-uncertain short
/// words are now corroborated by authored content paths in [`PATH_NAMES`].
///
/// One method was tried and **dropped**: deriving the rest of a family from one known member.
/// The hash check passes automatically for anything sharing the top three bytes, so it is not
/// evidence at all, and it produced `light_markerw` before that was understood.
const NAMES: &[(u32, &str)] = &[
    (0x003C_CD23, "fx_tower_left"),
    (0x006A_774D, "fx_hand"),
    (0x02D3_36BD, "eye_light"),
    (0x0314_1760, "audio_l"),
    (0x0314_1768, "audio_d"),
    (0x0314_1769, "audio_e"),
    (0x0314_176A, "audio_f"),
    (0x0314_176D, "audio_a"),
    (0x0314_176E, "audio_b"),
    (0x0314_176F, "audio_c"),
    (0x0314_177E, "audio_r"),
    (0x0387_F099, "front_blast"),
    (0x0447_A258, "fx_ambient_01"),
    (0x0447_A25A, "fx_ambient_03"),
    (0x0447_A25B, "fx_ambient_02"),
    (0x0447_A25C, "fx_ambient_05"),
    (0x0447_A25D, "fx_ambient_04"),
    (0x048E_0BE0, "fx_spawn_2"),
    (0x048E_0BE1, "fx_spawn_3"),
    (0x048E_0BE3, "fx_spawn_1"),
    (0x06C9_ABDC, "fx_debris03"),
    (0x06C9_ABDD, "fx_debris02"),
    (0x06C9_ABDE, "fx_debris01"),
    (0x0754_4D98, "marker_fx"),
    (0x0860_0F73, "spawn_point_left"),
    (0x087B_08C8, "spine_marker_02"),
    (0x087B_08C9, "spine_marker_03"),
    (0x087B_08CB, "spine_marker_01"),
    (0x095D_7357, "marker_back"),
    (0x0A49_8FBF, "fx_thruster_left"),
    (0x0AA4_C564, "seeker_1"),
    (0x0AA4_C566, "seeker_3"),
    (0x0AA4_C567, "seeker_2"),
    (0x0AC4_AEEE, "attach"),
    (0x0D74_6C58, "trooper_3"),
    (0x0D74_6C59, "trooper_2"),
    (0x0D74_6C5A, "trooper_1"),
    (0x0E9C_B668, "fx_spawn_11"),
    (0x0E9C_B66A, "fx_spawn_13"),
    (0x0E9C_B66B, "fx_spawn_12"),
    (0x0EAB_1902, "fx_shoulder"),
    (0x0F7D_86A4, "fx_arm_left"),
    (0x0FE4_53C4, "fx_tentacle_gun_r"),
    (0x0FE4_53DA, "fx_tentacle_gun_l"),
    (0x1176_7640, "light_wing_r"),
    (0x1176_765E, "light_wing_l"),
    (0x1453_CEE0, "light_eyes"),
    (0x1485_90C7, "fx_core"),
    (0x14FC_3FC8, "marker_impact_a"),
    (0x14FC_3FCB, "marker_impact_b"),
    (0x1704_4191, "fx_dodge_left"),
    (0x17EF_5500, "swarm_right"),
    (0x181E_140B, "light_back"),
    (0x1900_38B0, "fx_mine_right"),
    (0x1950_2755, "fx_fin_1"),
    (0x1950_2756, "fx_fin_2"),
    (0x1950_2757, "fx_fin_3"),
    (0x1A3D_D5A4, "fx_exhaust"),
    (0x1B41_29C5, "primary_trigger12"),
    (0x1B41_29C6, "primary_trigger11"),
    (0x1B41_29C7, "primary_trigger10"),
    (0x1B85_C674, "fx_elevator_1"),
    (0x1B85_C677, "fx_elevator_2"),
    (0x1BFB_03F9, "fx_buildup_1"),
    (0x1BFB_03FA, "fx_buildup_2"),
    (0x1BFB_03FB, "fx_buildup_3"),
    (0x1D34_4DC9, "foot_right_mid"),
    (0x1D38_0907, "fx_tentacle_r_back"),
    (0x1DB0_6BEC, "fx_primary"),
    (0x1DB2_9D31, "vent_d"),
    (0x1DB2_9D34, "vent_a"),
    (0x1DB2_9D36, "vent_c"),
    (0x1DB2_9D37, "vent_b"),
    (0x1E3D_1378, "fx_eye_light"),
    (0x1E7F_D669, "fx_tether_04"),
    (0x1E7F_D66C, "fx_tether_01"),
    (0x1E7F_D66E, "fx_tether_03"),
    (0x1E7F_D66F, "fx_tether_02"),
    (0x1EC1_7899, "weapon_back"),
    (0x1F0F_76AF, "eye_r"),
    (0x1F0F_76B1, "eye_l"),
    (0x2074_F50C, "fx_jetpack"),
    (0x2107_B205, "fx_mine_left"),
    (0x21CC_A1E8, "char_light_1"),
    (0x21CC_A1EB, "char_light_2"),
    (0x21DB_5B77, "light_front"),
    (0x220B_0C65, "lower_arc"),
    (0x2280_C005, "engine_l_b"),
    (0x2280_C006, "engine_l_a"),
    (0x22B8_661C, "marker_gun"),
    (0x2318_B567, "fx_right_shoulder"),
    (0x2390_0F95, "fx_tentacle_l_back"),
    (0x23B8_BF08, "audio_marker_c"),
    (0x23B8_BF09, "audio_marker_b"),
    (0x23B8_BF0A, "audio_marker_a"),
    (0x23B8_BF0F, "audio_marker_d"),
    (0x23B8_BF58, "audio_marker_3"),
    (0x23B8_BF59, "audio_marker_2"),
    (0x23B8_BF5A, "audio_marker_1"),
    (0x24FD_3D9C, "fx_front_right"),
    (0x271B_2F3C, "fx_destruction_back"),
    (0x275C_67E4, "flare_b"),
    (0x275C_67E7, "flare_a"),
    (0x288B_A13E, "second_right"),
    (0x28BB_BDF0, "fx_sparks_03"),
    (0x28BB_BDF1, "fx_sparks_02"),
    (0x28BB_BDF2, "fx_sparks_01"),
    (0x28BB_BDF6, "fx_sparks_05"),
    (0x28BB_BDF7, "fx_sparks_04"),
    (0x29E0_BDAD, "light_shield_front"),
    (0x2A47_EB1C, "fx_engine_r_2"),
    (0x2A47_EB1F, "fx_engine_r_1"),
    (0x2A73_77FD, "fx_engine_l_1"),
    (0x2A73_77FE, "fx_engine_l_2"),
    (0x2A8A_A3FE, "fx_destruction_front"),
    (0x2AE5_0407, "fx_rear_right"),
    (0x2B1C_F048, "fx_spawn1"),
    (0x2B1C_F04B, "fx_spawn2"),
    (0x2B2D_340B, "fx_tentacle_r_front"),
    (0x2BC9_E642, "fx_debris_l"),
    (0x2BC9_E65C, "fx_debris_r"),
    (0x2CB8_5BB6, "secondary_weapon_lower"),
    (0x2DE1_9209, "fx_jetpack_jet_l"),
    (0x2DE1_9217, "fx_jetpack_jet_r"),
    (0x2E08_5C75, "grip"),
    (0x2E9A_6126, "fx_right_wing"),
    (0x2F2E_DDAA, "radar_04"),
    (0x2F2E_DDAB, "radar_05"),
    (0x2F2E_DDAC, "radar_02"),
    (0x2F2E_DDAD, "radar_03"),
    (0x2F2E_DDAF, "radar_01"),
    (0x3085_7018, "smoke_left"),
    (0x30AB_4DFD, "audio_bottom"),
    (0x30B3_0F92, "fx_drum"),
    (0x3119_9B4F, "fx_marker_weakspot_front"),
    (0x324D_42F9, "marker_sparks01"),
    (0x324D_42FA, "marker_sparks02"),
    (0x324D_42FB, "marker_sparks03"),
    (0x3281_7890, "fx_right"),
    (0x341A_464C, "flare1"),
    (0x341A_464F, "flare2"),
    (0x3590_E5EE, "spawn_point_right"),
    (0x35A2_D297, "flare_back"),
    (0x38E3_6A6B, "marker_top"),
    (0x39D2_8E00, "marker_fx_left"),
    (0x3A2A_80A8, "fx_eye_l"),
    (0x3A2A_80B6, "fx_eye_r"),
    (0x3A2A_80F5, "fx_eye_1"),
    (0x3A2A_80F6, "fx_eye_2"),
    (0x3A32_2C9C, "fx_barrel_c"),
    (0x3A32_2C9D, "fx_barrel_b"),
    (0x3A32_2C9E, "fx_barrel_a"),
    (0x3B33_0194, "fx_tube_03"),
    (0x3B33_0195, "fx_tube_02"),
    (0x3B33_0196, "fx_tube_01"),
    (0x3C2A_2452, "mouth"),
    (0x3D6B_6CFE, "fx_light_r_2"),
    (0x4045_D393, "fx_shield_center"),
    (0x4058_B45E, "shoulder_r"),
    (0x410E_C650, "fx_left_thigh"),
    (0x415A_27F5, "fx_shield"),
    (0x41D0_BFD9, "fx_arc_b"),
    (0x41D0_BFDA, "fx_arc_a"),
    (0x41EA_57CD, "fx_arm_l"),
    (0x41EA_57D3, "fx_arm_r"),
    (0x4226_DCE0, "right_foot"),
    (0x4248_40D2, "fx_fin_1_b"),
    (0x4279_EFC1, "fx_upperarm_l"),
    (0x4279_EFDF, "fx_upperarm_r"),
    (0x43CE_CAA0, "fx_destruction_01"),
    (0x43CE_CAA2, "fx_destruction_03"),
    (0x43CE_CAA3, "fx_destruction_02"),
    (0x44FE_BE8E, "marker_light"),
    (0x4591_158F, "top_light"),
    (0x4594_1417, "marker_fx_right"),
    (0x4633_366C, "pigeon_marker"),
    (0x463C_3408, "flare_marker_03"),
    (0x463C_3409, "flare_marker_02"),
    (0x463C_340A, "flare_marker_01"),
    (0x467F_763A, "fx_light_eye_back"),
    (0x469C_7F81, "fx_leg_r"),
    (0x469C_7F9F, "fx_leg_l"),
    (0x46AA_BF09, "fx_center_01"),
    (0x46AA_BF0A, "fx_center_02"),
    (0x46AE_B439, "light_marker1"),
    (0x46AE_B43A, "light_marker2"),
    (0x46B3_AE88, "fx_marker_head_04"),
    (0x46B3_AE8D, "fx_marker_head_01"),
    (0x46B3_AE8E, "fx_marker_head_02"),
    (0x46B3_AE8F, "fx_marker_head_03"),
    (0x4767_278F, "marker_bottom"),
    (0x48C1_199C, "death_fx_3"),
    (0x48C1_199D, "death_fx_2"),
    (0x48C1_199E, "death_fx_1"),
    (0x48EC_85D4, "fx_foot_c"),
    (0x48EC_85D5, "fx_foot_b"),
    (0x48EC_85D6, "fx_foot_a"),
    (0x48F2_4719, "marker_frame_a"),
    (0x48F2_471A, "marker_frame_b"),
    (0x48F2_471B, "marker_frame_c"),
    (0x48F2_471C, "marker_frame_d"),
    (0x48F2_471D, "marker_frame_e"),
    (0x48F2_471E, "marker_frame_f"),
    (0x49B2_B7F5, "fire_spawn_1"),
    (0x49B2_B7F6, "fire_spawn_2"),
    (0x49B2_B7F7, "fire_spawn_3"),
    (0x49CA_94AA, "fx_upper"),
    (0x4BC8_EE36, "right_fx"),
    (0x4BDF_AAEA, "light_hand_r"),
    (0x4BDF_AAF4, "light_hand_l"),
    (0x4C3C_F454, "fx_secondary"),
    (0x4D44_8AB0, "fx_rear_left"),
    (0x4D5C_A5AD, "marker_volume_11"),
    (0x4D5C_A5AE, "marker_volume_12"),
    (0x4E7D_80F9, "fx_debris_left"),
    (0x4EBE_F880, "fx_bomb_01"),
    (0x4EBE_F882, "fx_bomb_03"),
    (0x4EBE_F883, "fx_bomb_02"),
    (0x4F12_6D95, "swarm_left"),
    (0x5018_7E97, "fx_front"),
    (0x5087_8189, "fx_exhaust_02"),
    (0x5087_818A, "fx_exhaust_01"),
    (0x50AD_E9A8, "audio_marker_center_01"),
    (0x50AD_E9AB, "audio_marker_center_02"),
    (0x51EE_4278, "fx_left_shoulder"),
    (0x5219_307B, "fx_engines"),
    (0x5219_CF94, "fx_tendril_3"),
    (0x5219_CF95, "fx_tendril_2"),
    (0x5219_CF96, "fx_tendril_1"),
    (0x525D_0589, "screen_fx_right"),
    (0x52E2_34D8, "fx_engine"),
    (0x533A_1F50, "marker14"),
    (0x533A_1F51, "marker15"),
    (0x533A_1F52, "marker16"),
    (0x533A_1F53, "marker17"),
    (0x533A_1F54, "marker10"),
    (0x533A_1F55, "marker11"),
    (0x533A_1F56, "marker12"),
    (0x533A_1F57, "marker13"),
    (0x533A_1F5C, "marker18"),
    (0x533A_1F5D, "marker19"),
    (0x543A_20C0, "marker07"),
    (0x543A_20C1, "marker06"),
    (0x543A_20C2, "marker05"),
    (0x543A_20C3, "marker04"),
    (0x543A_20C4, "marker03"),
    (0x543A_20C5, "marker02"),
    (0x543A_20C6, "marker01"),
    (0x543A_20CE, "marker09"),
    (0x543A_20CF, "marker08"),
    (0x563A_242C, "marker21"),
    (0x563A_242D, "marker20"),
    (0x5783_AB7A, "fx_bullet"),
    (0x57A4_B5E4, "fx_spine_2"),
    (0x57A4_B5E5, "fx_spine_3"),
    (0x58E2_1FD6, "fx_destruction_left"),
    (0x590C_CFCD, "fx_knee_left"),
    (0x5994_B255, "light_shield"),
    (0x5A9C_D839, "fx_marker01"),
    (0x5A9C_D83A, "fx_marker02"),
    (0x5A9C_D83B, "fx_marker03"),
    (0x5A9C_D83C, "fx_marker04"),
    (0x5A9C_D83D, "fx_marker05"),
    (0x5A9C_D83E, "fx_marker06"),
    (0x5ABB_A778, "lightning_d"),
    (0x5ABB_A77D, "lightning_a"),
    (0x5ABB_A77E, "lightning_b"),
    (0x5ABB_A77F, "lightning_c"),
    (0x5D25_5CB1, "fx_shield_large_1"),
    (0x5D25_5CB2, "fx_shield_large_2"),
    (0x5D25_5CB3, "fx_shield_large_3"),
    (0x5F1C_B809, "fx_shoulder_r"),
    (0x5F1C_B817, "fx_shoulder_l"),
    (0x5FC3_1E85, "fx_engine_light"),
    (0x5FC7_F42F, "left_fx"),
    (0x60FE_CDD4, "primary_trigger"),
    (0x6241_974B, "bounce_light_right"),
    (0x625C_FF84, "engine_r_a"),
    (0x625C_FF87, "engine_r_b"),
    (0x6301_0EC4, "fx_indicator01"),
    (0x6301_0EC7, "fx_indicator02"),
    (0x651B_D2F4, "mine_left"),
    (0x6575_5F50, "fx_wing_right"),
    (0x657E_0B21, "light_tentacle_l"),
    (0x657E_0B3F, "light_tentacle_r"),
    (0x6591_6488, "teleport_spawn"),
    (0x6750_3036, "head_ejection_right"),
    (0x68B0_4FB1, "fx_sphere_bottom"),
    (0x691E_A3B6, "fx_engine_right_light"),
    (0x6969_5F77, "left_light"),
    (0x69B3_59B4, "fx_light_eye_front"),
    (0x6A93_A7D6, "marker_audio_bottom"),
    (0x6BFC_89E8, "fx_barrel_05"),
    (0x6BFC_89E9, "fx_barrel_04"),
    (0x6BFC_89EC, "fx_barrel_01"),
    (0x6BFC_89EE, "fx_barrel_03"),
    (0x6BFC_89EF, "fx_barrel_02"),
    (0x6C2D_3AFB, "fx_bottom"),
    (0x6DC2_E0D0, "fx_thruster_04"),
    (0x6DC2_E0D1, "fx_thruster_05"),
    (0x6DC2_E0D5, "fx_thruster_01"),
    (0x6DC2_E0D6, "fx_thruster_02"),
    (0x6DC2_E0D7, "fx_thruster_03"),
    (0x6DFE_676D, "body"),
    (0x70BC_9655, "neck_light"),
    (0x71ED_0A3B, "head_ejection_left"),
    (0x7228_B70D, "fx_bottom_left"),
    (0x72E4_7874, "scan2"),
    (0x72E4_7875, "scan3"),
    (0x72E4_7877, "scan1"),
    (0x7385_4300, "marker_pod_b"),
    (0x7385_4301, "marker_pod_c"),
    (0x7385_4303, "marker_pod_a"),
    (0x7385_4306, "marker_pod_d"),
    (0x7454_5BEC, "marker_audio_top"),
    (0x745C_37F5, "audio_top"),
    (0x746F_FE9C, "primary_weapon"),
    (0x74FD_9FB8, "fx_missile_01"),
    (0x74FD_9FBA, "fx_missile_03"),
    (0x74FD_9FBB, "fx_missile_02"),
    (0x74FD_9FBC, "fx_missile_05"),
    (0x74FD_9FBD, "fx_missile_04"),
    (0x75C0_A6DB, "fx_tip_lower"),
    (0x761F_EDCA, "fx_hand_r"),
    (0x761F_EDD4, "fx_hand_l"),
    (0x76A4_06C2, "fx_hip_r"),
    (0x76A4_06DC, "fx_hip_l"),
    (0x775C_031F, "fx_left_bot"),
    (0x779F_7260, "light_body_core"),
    (0x77AA_BA50, "fx_thruster_l"),
    (0x77AA_BA5D, "fx_thruster_a"),
    (0x77AA_BA5E, "fx_thruster_b"),
    (0x7855_2CB4, "light_head"),
    (0x7930_F5AE, "fx_vent_neck_l"),
    (0x7930_F5B0, "fx_vent_neck_r"),
    (0x79DC_B088, "fx_vent_b"),
    (0x79DC_B089, "fx_vent_c"),
    (0x79DC_B08B, "fx_vent_a"),
    (0x79DC_B08E, "fx_vent_d"),
    (0x7A1D_A148, "fx_left_leg_1"),
    (0x7A1D_A14B, "fx_left_leg_2"),
    (0x7BCE_6EDD, "fx_right_thigh"),
    (0x7C6B_3DAC, "fx_shell"),
    (0x7D05_B410, "light_marker08"),
    (0x7D05_B411, "light_marker09"),
    (0x7D05_B419, "light_marker01"),
    (0x7D05_B41A, "light_marker02"),
    (0x7D05_B41B, "light_marker03"),
    (0x7D05_B41C, "light_marker04"),
    (0x7D05_B41D, "light_marker05"),
    (0x7D05_B41E, "light_marker06"),
    (0x7D05_B41F, "light_marker07"),
    (0x7DF2_6431, "fx_damage_1"),
    (0x7DF2_6432, "fx_damage_2"),
    (0x7DF2_6433, "fx_damage_3"),
    (0x7DF2_6461, "fx_damage_a"),
    (0x7DF2_6462, "fx_damage_b"),
    (0x7DF6_EDE2, "fx_thigh_left"),
    (0x7E63_5DFC, "fx_top_right"),
    (0x824A_E465, "fx_fin_2_b"),
    (0x82B1_5CEB, "fx_vent"),
    (0x8341_77F0, "fx_light_01"),
    (0x8341_77F2, "fx_light_03"),
    (0x8341_77F3, "fx_light_02"),
    (0x83EA_8EB4, "fx_02"),
    (0x83EA_8EB7, "fx_01"),
    (0x84CB_61CE, "weapon_light_attach_3"),
    (0x84CB_61CF, "weapon_light_attach_2"),
    (0x851E_0484, "primary_trigger8"),
    (0x851E_0485, "primary_trigger9"),
    (0x851E_0488, "primary_trigger4"),
    (0x851E_0489, "primary_trigger5"),
    (0x851E_048A, "primary_trigger6"),
    (0x851E_048B, "primary_trigger7"),
    (0x851E_048D, "primary_trigger1"),
    (0x851E_048E, "primary_trigger2"),
    (0x851E_048F, "primary_trigger3"),
    (0x8575_3838, "fx_exhaust_1"),
    (0x8575_383B, "fx_exhaust_2"),
    (0x8592_B68D, "fx_thruster_b_2"),
    (0x8592_B68E, "fx_thruster_b_1"),
    (0x8992_6790, "secondary_trigger"),
    (0x89E7_B110, "fx_sparks_1"),
    (0x89E7_B112, "fx_sparks_3"),
    (0x89E7_B113, "fx_sparks_2"),
    (0x89E7_B140, "fx_sparks_a"),
    (0x89E7_B143, "fx_sparks_b"),
    (0x8A10_5DE1, "rocket_spawn_1"),
    (0x8A10_5DE2, "rocket_spawn_2"),
    (0x8A10_5DE3, "rocket_spawn_3"),
    (0x8AFD_F889, "fx_vents_r"),
    (0x8AFD_F897, "fx_vents_l"),
    (0x8C8B_EAE5, "light_flare_r"),
    (0x8C8B_EAFB, "light_flare_l"),
    (0x8C8F_0652, "foot_right_front"),
    (0x8D0D_D3CD, "iron_sight"),
    (0x8DD4_EB77, "front_light"),
    (0x8F09_0264, "fx_barrel_end"),
    (0x8F31_4B5E, "fx_tower_right"),
    (0x8F47_4BE8, "flare_marker_a"),
    (0x8F47_4BEB, "flare_marker_b"),
    (0x8F68_4A2D, "marker_light_b"),
    (0x8F68_4A2E, "marker_light_a"),
    (0x8F68_4A7D, "marker_light_2"),
    (0x8F68_4A7E, "marker_light_1"),
    (0x900A_D39A, "primary_weapon_lower"),
    (0x906E_353D, "elbow_r"),
    (0x9079_20D8, "light_marker"),
    (0x90CB_4B09, "fx_top_left"),
    (0x910E_5F56, "fx_shoulder_left"),
    (0x9160_607D, "spark1"),
    (0x9160_607E, "spark2"),
    (0x9160_607F, "spark3"),
    (0x9216_EEA9, "fx_ejection"),
    (0x92A1_1080, "secondary_fire"),
    (0x92F2_F49E, "support"),
    (0x93EA_BCEC, "light_jetpack"),
    (0x9452_8C6C, "fx_damage3"),
    (0x9452_8C6D, "fx_damage2"),
    (0x947C_D0A6, "body1"),
    (0x94A7_1887, "fx_engine_r"),
    (0x94A7_1894, "fx_engine_a"),
    (0x94A7_1897, "fx_engine_b"),
    (0x94A7_1899, "fx_engine_l"),
    (0x94D5_51F8, "explosion_marker03"),
    (0x94D5_51F9, "explosion_marker02"),
    (0x95E2_7DAF, "right_hand"),
    (0x967C_21E0, "fx_rod01"),
    (0x967C_21E3, "fx_rod02"),
    (0x96D5_3C70, "audio_marker_01"),
    (0x96D5_3C73, "audio_marker_02"),
    (0x97AF_A670, "sfx_marker_02"),
    (0x97AF_A673, "sfx_marker_01"),
    (0x981F_0F71, "fx_tentacle_l_front"),
    (0x98DA_9A6B, "fx_left_hand"),
    (0x9925_1137, "foot_left_front"),
    (0x9987_C5FF, "fx_left_tip"),
    (0x9AD2_0665, "fx_left"),
    (0x9C05_E494, "light_marker_a"),
    (0x9C05_E496, "light_marker_c"),
    (0x9C05_E497, "light_marker_b"),
    (0x9D6C_4D42, "fx_thruster_right"),
    (0x9D76_82DE, "fx_left_mid"),
    (0x9DF5_5427, "light_core"),
    (0x9E34_0319, "fx_marker_leg_1"),
    (0x9E34_031A, "fx_marker_leg_2"),
    (0x9E34_031B, "fx_marker_leg_3"),
    (0x9E55_EC40, "fx_engine_left"),
    (0x9EAD_0C65, "light_foot_r"),
    (0x9EAD_0C7B, "light_foot_l"),
    (0x9FC0_88ED, "ghost_body"),
    (0x9FFA_CC29, "light_shoulder_r"),
    (0x9FFA_CC37, "light_shoulder_l"),
    (0xA077_CEA7, "fx_eye_left"),
    (0xA0D3_A093, "mine_right"),
    (0xA12A_6595, "fx_base"),
    (0xA366_6133, "fx_hand_left"),
    (0xA394_784C, "bounce_light_left"),
    (0xA3DE_FAC8, "foot_left_rear"),
    (0xA55B_0008, "m_statue_d"),
    (0xA55B_0009, "m_statue_e"),
    (0xA55B_000A, "m_statue_f"),
    (0xA55B_000D, "m_statue_a"),
    (0xA55B_000E, "m_statue_b"),
    (0xA55B_000F, "m_statue_c"),
    (0xA5BC_7129, "blast_01"),
    (0xA5BC_712A, "blast_02"),
    (0xA5BC_712B, "blast_03"),
    (0xA5BC_712C, "blast_04"),
    (0xA5BC_712D, "blast_05"),
    (0xA610_C121, "fx_eye"),
    (0xA6E3_8008, "backpack_fire_3"),
    (0xA6E3_8009, "backpack_fire_2"),
    (0xA6E3_800A, "backpack_fire_1"),
    (0xA713_2B57, "fx_engine_right"),
    (0xA7E5_384F, "smoke_right"),
    (0xA80B_A1AC, "fx_debris_right"),
    (0xA88C_A4C0, "right_light"),
    (0xA8CF_B5DC, "fx_tip_inner"),
    (0xA93A_A6A2, "marker_d"),
    (0xA93A_A6A4, "marker_b"),
    (0xA93A_A6A5, "marker_c"),
    (0xA93A_A6A7, "marker_a"),
    (0xA977_9185, "marker8"),
    (0xA977_9188, "marker5"),
    (0xA977_9189, "marker4"),
    (0xA977_918A, "marker7"),
    (0xA977_918B, "marker6"),
    (0xA977_918C, "marker1"),
    (0xA977_918E, "marker3"),
    (0xA977_918F, "marker2"),
    (0xAA0A_7F16, "left_hand"),
    (0xAA0F_F824, "fx_tentacle_spike_r"),
    (0xAA0F_F83A, "fx_tentacle_spike_l"),
    (0xAA25_B574, "marker_volume_1"),
    (0xAA25_B576, "marker_volume_3"),
    (0xAA25_B577, "marker_volume_2"),
    (0xAB33_7067, "fx_beam"),
    (0xAB7A_D770, "shin_light"),
    (0xAC66_E96A, "light_phalanx_eye"),
    (0xAC8F_C9CB, "fx_neck"),
    (0xACEC_5E83, "fx_arm_right"),
    (0xAE57_0B05, "fx_shield_core"),
    (0xAE7F_B6F0, "fx_jet_b"),
    (0xAE7F_B6F1, "fx_jet_c"),
    (0xAE7F_B6F3, "fx_jet_a"),
    (0xAE7F_B6F4, "fx_jet_f"),
    (0xAE7F_B6F6, "fx_jet_d"),
    (0xAE7F_B6F7, "fx_jet_e"),
    (0xAF16_FB84, "fx_flare_3"),
    (0xAF16_FB85, "fx_flare_2"),
    (0xAF16_FB86, "fx_flare_1"),
    (0xAF3A_2CF9, "fx_tracer_01"),
    (0xAF3A_2CFA, "fx_tracer_02"),
    (0xAF3A_2CFB, "fx_tracer_03"),
    (0xAF3A_2CFC, "fx_tracer_04"),
    (0xAF3A_2CFD, "fx_tracer_05"),
    (0xAF7D_4A48, "fx_marker_arm_02"),
    (0xAF7D_4A4B, "fx_marker_arm_01"),
    (0xB029_8D28, "fx_trail_3"),
    (0xB029_8D29, "fx_trail_2"),
    (0xB029_8D2A, "fx_trail_1"),
    (0xB06A_671A, "fx_vent_04"),
    (0xB06A_671B, "fx_vent_05"),
    (0xB06A_671C, "fx_vent_02"),
    (0xB06A_671D, "fx_vent_03"),
    (0xB06A_671F, "fx_vent_01"),
    (0xB16A_F80D, "fx_engine_left_light"),
    (0xB19C_5262, "fx_wall_left"),
    (0xB1E7_8651, "fx_tip"),
    (0xB3E7_8937, "fx_top"),
    (0xB446_843A, "primary_trigger_beam"),
    (0xB53B_10F9, "fx_wall_right"),
    (0xB57D_5BD0, "weapon_light_attach"),
    (0xB71D_60B4, "break02"),
    (0xB71D_60B5, "break03"),
    (0xB71D_60B7, "break01"),
    (0xB72B_0E35, "back_light"),
    (0xB813_4A67, "foot_right_rear"),
    (0xB8EA_44A8, "fx_laser_2"),
    (0xB8EA_44A9, "fx_laser_3"),
    (0xB8EA_44AB, "fx_laser_1"),
    (0xB9DF_D919, "spawn_point_a"),
    (0xB9DF_D91A, "spawn_point_b"),
    (0xB9DF_D91B, "spawn_point_c"),
    (0xB9DF_D91C, "spawn_point_d"),
    (0xB9DF_D91D, "spawn_point_e"),
    (0xB9DF_D91E, "spawn_point_f"),
    (0xBB30_B1A8, "fx_right_hand"),
    (0xBB6A_AB30, "muzzle_flash_04"),
    (0xBB6A_AB35, "muzzle_flash_01"),
    (0xBB6A_AB36, "muzzle_flash_02"),
    (0xBB6A_AB37, "muzzle_flash_03"),
    (0xBCC4_CF05, "fx_pelvis"),
    (0xBD66_D250, "marker_rocket_c"),
    (0xBD66_D251, "marker_rocket_b"),
    (0xBD66_D252, "marker_rocket_a"),
    (0xBD66_D255, "marker_rocket_f"),
    (0xBD66_D256, "marker_rocket_e"),
    (0xBD66_D257, "marker_rocket_d"),
    (0xBE01_CFB5, "fx_tip_outer"),
    (0xBE4E_0728, "light1"),
    (0xBE4E_072A, "light3"),
    (0xBE4E_072B, "light2"),
    (0xBEC4_067B, "fx_torso"),
    (0xBF5C_95D8, "fx_tunnel_03"),
    (0xBF5C_95D9, "fx_tunnel_02"),
    (0xBF5C_95DA, "fx_tunnel_01"),
    (0xBF5C_95DE, "fx_tunnel_05"),
    (0xBF5C_95DF, "fx_tunnel_04"),
    (0xBFCE_11E0, "projectile_1"),
    (0xBFCE_11E2, "projectile_3"),
    (0xBFCE_11E3, "projectile_2"),
    (0xC0E8_8321, "lower_impact"),
    (0xC260_9030, "marker_explosion_2"),
    (0xC260_9031, "marker_explosion_3"),
    (0xC260_9033, "marker_explosion_1"),
    (0xC385_9B0A, "fx_eye_right"),
    (0xC487_E320, "fx_2"),
    (0xC487_E321, "fx_3"),
    (0xC487_E323, "fx_1"),
    (0xC49B_9301, "fx_foot_right"),
    (0xC51C_1674, "fx_thrusters_a"),
    (0xC51C_1676, "fx_thrusters_c"),
    (0xC51C_1677, "fx_thrusters_b"),
    (0xC636_FA7A, "thigh_light"),
    (0xC64D_D383, "fx_marker_weakspot_back"),
    (0xC842_C948, "marker_impulse_2"),
    (0xC842_C94B, "marker_impulse_1"),
    (0xC8F9_C6F6, "fx_front_light"),
    (0xC9E7_A8D6, "fx_right_tip"),
    (0xCA3F_2A48, "foot_left_mid"),
    (0xCAD5_2289, "fx_chest"),
    (0xCB0E_C586, "fx_light_m"),
    (0xCB0E_C587, "fx_light_l"),
    (0xCB0E_C599, "fx_light_r"),
    (0xCB36_C0FB, "flare_front"),
    (0xCB5F_3FA1, "rocket_a"),
    (0xCB5F_3FA2, "rocket_b"),
    (0xCB5F_3FA3, "rocket_c"),
    (0xCB5F_3FA4, "rocket_d"),
    (0xCB89_38EC, "fx_exhaust_left"),
    (0xCBE1_74EE, "fx_mouth_light"),
    (0xCC1F_58B2, "fx_right_bot"),
    (0xCC48_4E2F, "fx_taken_spawn"),
    (0xCD15_9BA4, "body_light"),
    (0xCD1E_B68F, "fx_sword_tip"),
    (0xCD88_7E07, "head"),
    (0xCEE2_FF60, "fx_launch_011"),
    (0xCEE2_FF63, "fx_launch_012"),
    (0xCF00_7231, "marker_arc_a_04"),
    (0xCF00_7234, "marker_arc_a_01"),
    (0xCF00_7236, "marker_arc_a_03"),
    (0xCF00_7237, "marker_arc_a_02"),
    (0xCF20_4425, "fx_wing_left"),
    (0xCF4F_543C, "spawn_c"),
    (0xCF4F_543D, "spawn_b"),
    (0xCF4F_543E, "spawn_a"),
    (0xCF4F_546C, "spawn_3"),
    (0xCF4F_546D, "spawn_2"),
    (0xCF4F_546E, "spawn_1"),
    (0xD1BA_876B, "fx_exhaust_right"),
    (0xD29C_4DC1, "marker_arc_b_01"),
    (0xD29C_4DC2, "marker_arc_b_02"),
    (0xD29C_4DC3, "marker_arc_b_03"),
    (0xD29C_4DC4, "marker_arc_b_04"),
    (0xD477_036D, "fx_center"),
    (0xD595_B2F9, "fx_sphere_top"),
    (0xD696_6607, "fx_marker_08"),
    (0xD696_6609, "fx_marker_06"),
    (0xD696_660A, "fx_marker_05"),
    (0xD696_660B, "fx_marker_04"),
    (0xD696_660C, "fx_marker_03"),
    (0xD696_660D, "fx_marker_02"),
    (0xD696_660E, "fx_marker_01"),
    (0xD72E_2B66, "head_light"),
    (0xD73E_D1F8, "fx_tip_upper"),
    (0xD7BB_0BB8, "leg_light_3"),
    (0xD7BB_0BB9, "leg_light_2"),
    (0xD7BB_0BBA, "leg_light_1"),
    (0xD860_8A21, "light_leg_r"),
    (0xD860_8A3F, "light_leg_l"),
    (0xD87E_92B8, "fx_knee_right"),
    (0xD92C_18C0, "explosion_01"),
    (0xD92C_18C2, "explosion_03"),
    (0xD92C_18C3, "explosion_02"),
    (0xD942_7E71, "light_sword_a"),
    (0xD942_7E72, "light_sword_b"),
    (0xDA7A_6F25, "fx_lower"),
    (0xDAD5_7350, "light_b"),
    (0xDAD5_7353, "light_a"),
    (0xDB5D_5C80, "fx_lock_f"),
    (0xDB5D_5C82, "fx_lock_d"),
    (0xDB5D_5C83, "fx_lock_e"),
    (0xDB5D_5C84, "fx_lock_b"),
    (0xDB5D_5C85, "fx_lock_c"),
    (0xDB5D_5C87, "fx_lock_a"),
    (0xDB5D_A90A, "fx_foot_left"),
    (0xDFAC_9C32, "m_tube_h"),
    (0xDFAC_9C38, "m_tube_b"),
    (0xDFAC_9C39, "m_tube_c"),
    (0xDFAC_9C3B, "m_tube_a"),
    (0xDFAC_9C3C, "m_tube_f"),
    (0xDFAC_9C3D, "m_tube_g"),
    (0xDFAC_9C3E, "m_tube_d"),
    (0xDFAC_9C3F, "m_tube_e"),
    (0xDFD9_420F, "fx_mouth"),
    (0xE073_1E98, "upper_impact"),
    (0xE1B5_715C, "fx_marker_center"),
    (0xE371_2211, "fx_spark_1"),
    (0xE371_2212, "fx_spark_2"),
    (0xE371_2213, "fx_spark_3"),
    (0xE37E_E3D5, "fx_shoulder_right"),
    (0xE385_0328, "cup1"),
    (0xE385_032A, "cup3"),
    (0xE385_032B, "cup2"),
    (0xE5C8_F8A2, "upper_arc"),
    (0xE5DB_0413, "fx_platform"),
    (0xE5F9_1DBF, "fx_right_mid"),
    (0xE7DD_DB79, "fx_thigh_right"),
    (0xE804_F450, "fx_muzzle_flash_05"),
    (0xE804_F451, "fx_muzzle_flash_04"),
    (0xE804_F454, "fx_muzzle_flash_01"),
    (0xE804_F456, "fx_muzzle_flash_03"),
    (0xE804_F457, "fx_muzzle_flash_02"),
    (0xE940_E4C1, "smoke01"),
    (0xE940_E4C2, "smoke02"),
    (0xE99E_1A08, "fx_launch_02"),
    (0xE99E_1A09, "fx_launch_03"),
    (0xE99E_1A0B, "fx_launch_01"),
    (0xEA0F_A529, "fx_front_left"),
    (0xEA97_4E30, "charge_fire_1"),
    (0xEA97_4E32, "charge_fire_3"),
    (0xEA97_4E33, "charge_fire_2"),
    (0xEAD6_0BB1, "left_foot"),
    (0xEB43_A219, "fx_shield_small_1"),
    (0xEB43_A21A, "fx_shield_small_2"),
    (0xEB43_A21B, "fx_shield_small_3"),
    (0xEB9A_FC50, "secondary_weapon"),
    (0xED14_A1CF, "fx_antenna"),
    (0xEDE3_EC12, "sparks_d"),
    (0xEDE3_EC14, "sparks_b"),
    (0xEDE3_EC15, "sparks_c"),
    (0xEDE3_EC17, "sparks_a"),
    (0xEEB9_150B, "thruster_front_l"),
    (0xEEB9_1515, "thruster_front_r"),
    (0xEEEA_9D18, "projectile_spawn_1"),
    (0xEEEA_9D1A, "projectile_spawn_3"),
    (0xEEEA_9D1B, "projectile_spawn_2"),
    (0xEF7F_F2C2, "fx_root"),
    (0xF0D4_1ABB, "marker_front"),
    (0xF15F_D244, "fx_heat"),
    (0xF15F_D254, "fx_head"),
    (0xF1C7_94E9, "rocks_a"),
    (0xF1C7_94EA, "rocks_b"),
    (0xF1CE_E91E, "fx_barrel"),
    (0xF213_CFBA, "target_eye_05"),
    (0xF213_CFBB, "target_eye_04"),
    (0xF213_CFBC, "target_eye_03"),
    (0xF213_CFBD, "target_eye_02"),
    (0xF213_CFBE, "target_eye_01"),
    (0xF2A0_71CC, "primary_fire"),
    (0xF44F_7283, "second_left"),
    (0xF491_DC08, "light_knee_r"),
    (0xF491_DC16, "light_knee_l"),
    (0xF499_CE44, "fx_hands"),
    (0xF50A_2247, "fx_forearm_l"),
    (0xF50A_2259, "fx_forearm_r"),
    (0xF54D_00C9, "marker_center"),
    (0xF5D6_ED55, "fx_destruction_right"),
    (0xF773_4D72, "screen_fx_left"),
    (0xF7CF_D729, "fx_left_wing"),
    (0xF841_4052, "fx_light"),
    (0xFA0A_CD7C, "fp_02"),
    (0xFA0A_CD7F, "fp_01"),
    (0xFC60_5AF9, "fx_sword_hilt"),
    (0xFCEF_9408, "fx_elbow_right"),
    (0xFD29_80CD, "light_center"),
    (0xFE96_E204, "fx_dodge_right"),
    (0xFF64_7143, "back_blast"),
    (0xFFDD_8D18, "fx_side_b"),
    (0xFFDD_8D19, "fx_side_c"),
    (0xFFDD_8D1B, "fx_side_a"),
    (0xFFDD_8D1E, "fx_side_d"),
];

/// Names independently present as whole content-path segments or contiguous underscore words
/// in the installed TFT index. The 28,826 distinct candidates were checked against 4,621 marker
/// hashes, giving 0.031 accidental matches expected across the entire search. Fifty-five names
/// beyond `NAMES` matched. These are literal authored strings, not names inferred from geometry.
const PATH_NAMES: &[(u32, &str)] = &[
    (0x043E_76FE, "spawn"),
    (0x06BB_2B68, "target_eye"),
    (0x08EF_6A62, "scan"),
    (0x0965_647A, "rumble"),
    (0x0FFF_B22A, "civilian"),
    (0x1249_6650, "camera"),
    (0x145A_D6A8, "waypoint"),
    (0x1A5F_3825, "barrel"),
    (0x213D_F07E, "screen_fx"),
    (0x2505_5C1F, "step"),
    (0x253F_6F5C, "projectile"),
    (0x2836_47F6, "damage_impulse"),
    (0x292F_EA33, "loot"),
    (0x2AAD_129F, "fairchild"),
    (0x2E5F_9B17, "pigeon"),
    (0x3693_2030, "eye"),
    (0x3C9E_096E, "spider"),
    (0x3D75_2D6A, "debug"),
    (0x3E9C_2724, "aim"),
    (0x3FC4_0859, "audio"),
    (0x4194_5228, "neck"),
    (0x47F7_ACA3, "ghost_light"),
    (0x4B21_A032, "front"),
    (0x4CF9_B596, "base"),
    (0x4DB7_3382, "main_cannon"),
    (0x5524_75D4, "aim_target"),
    (0x5767_3EB7, "interact"),
    (0x5BCD_E1AF, "flare"),
    (0x5F0A_4B88, "detain"),
    (0x6A36_E14D, "conduit"),
    (0x6AB2_CD38, "proximity"),
    (0x6BB1_4E40, "alpha_strike"),
    (0x6FED_AE4D, "door"),
    (0x735C_F023, "light"),
    (0x7682_2486, "center"),
    (0x81E5_C3A8, "phantom"),
    (0x8BFC_E4A9, "dust"),
    (0x8D31_B469, "enter"),
    (0x9015_5FB9, "target_weakspot"),
    (0x9F6D_B313, "turret"),
    (0xA253_0E93, "swarm"),
    (0xA771_EE50, "splash_damage"),
    (0xB510_D885, "fx_end"),
    (0xB793_BB4E, "osiris"),
    (0xBA23_0E77, "tail"),
    (0xC216_4892, "volume_light"),
    (0xCF60_F347, "shaxx_pvp_crucible_vendor"),
    (0xDD84_6A12, "impulse"),
    (0xE882_D22F, "interaction"),
    (0xE89B_DBB6, "target"),
    (0xEA77_CA98, "amanda_holliday"),
    (0xED61_BC6E, "laser"),
    (0xF21C_75B3, "ai_spawn"),
    (0xF389_5473, "fx_wipe"),
    (0xFEDC_4DA7, "projectiles"),
];

/// The recovered name of one marker, when it has one.
#[must_use]
pub fn marker_name(hash: u32) -> Option<&'static str> {
    NAMES
        .binary_search_by_key(&hash, |(key, _)| *key)
        .ok()
        .map(|index| NAMES[index].1)
        .or_else(|| {
            PATH_NAMES
                .binary_search_by_key(&hash, |(key, _)| *key)
                .ok()
                .map(|index| PATH_NAMES[index].1)
        })
}

/// One named point on a piece of gear art, in the model's own space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub name: u32,
    pub position: [f32; 3],
    /// The direction the marker faces, as a quaternion in x, y, z, w order. Two markers may
    /// share a name and a position and differ only here.
    pub orientation: [f32; 4],
}

impl Marker {
    /// Whether the marker faces straight ahead, which most do. Only a turned marker is worth
    /// reporting, so the report stays readable.
    #[must_use]
    pub fn is_aligned(&self) -> bool {
        let [x, y, z, w] = self.orientation;
        x.abs() < 1e-4 && y.abs() < 1e-4 && z.abs() < 1e-4 && (w.abs() - 1.0).abs() < 1e-4
    }

    /// The marker's recovered name, or its hash when it has none.
    #[must_use]
    pub fn label(&self) -> String {
        marker_name(self.name).map_or_else(|| format!("0x{:08X}", self.name), ToOwned::to_owned)
    }
}

/// One gear entity's marker set.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkerSet {
    pub entity: u32,
    pub component: u32,
    pub markers: Vec<Marker>,
}

/// How a marker relates to the ones around it: the named marker nearest to it, and how far.
///
/// A hash whose name is unrecovered is still placed somewhere meaningful, and saying it sits
/// eleven millimetres from `primary_fire` tells a reader more than the hash does. This is a
/// statement about geometry, which the data supports, and not a guess at the name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Neighbour {
    pub name: &'static str,
    /// Distance in the model's own units, which are metres.
    pub distance: f32,
}

impl std::fmt::Display for Neighbour {
    /// Millimetres, because every distance that matters here is a centimetre or two. Markers
    /// that share a point are common and saying "0mm" reads as a missing number, so they say
    /// so instead.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.distance < 0.000_5 {
            write!(out, "on {}", self.name)
        } else {
            write!(out, "by {} {:.0}mm", self.name, self.distance * 1000.0)
        }
    }
}

/// The named marker nearest to `marker`, across every set of one appearance.
///
/// `None` when the marker is itself named, when nothing named shares the appearance, or when
/// the nearest named marker is too far off to say anything useful about it.
#[must_use]
pub fn nearest_named(sets: &[MarkerSet], marker: &Marker) -> Option<Neighbour> {
    /// Past this, "near" stops meaning anything on a weapon a metre long.
    const REACH: f32 = 0.5;
    if marker_name(marker.name).is_some() {
        return None;
    }
    // A character's marker set puts every row at the origin and lets the skeleton place them
    // at runtime. Everything is then "on" everything, which is true and tells a reader
    // nothing, so a set that collapses to one point is left unplaced.
    let mut positions = sets.iter().flat_map(|set| &set.markers).map(|m| m.position);
    let first = positions.next()?;
    if !positions.any(|position| position != first) {
        return None;
    }
    sets.iter()
        .flat_map(|set| &set.markers)
        .filter_map(|other| Some((marker_name(other.name)?, other.position)))
        .map(|(name, position)| Neighbour {
            name,
            distance: position
                .iter()
                .zip(&marker.position)
                .map(|(a, b)| (a - b) * (a - b))
                .sum::<f32>()
                .sqrt(),
        })
        .filter(|neighbour| neighbour.distance <= REACH)
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

/// A marker's name, or its hash with the named marker it sits next to.
#[must_use]
pub fn describe(sets: &[MarkerSet], marker: &Marker) -> String {
    let label = marker.label();
    match nearest_named(sets, marker) {
        Some(near) => format!("{label} {near}"),
        None => label,
    }
}

/// One marker name across the whole game: how widely it is used, where to find it, and what
/// it sits beside.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkerEntry {
    pub hash: u32,
    /// The recovered name, when there is one.
    pub name: Option<&'static str>,
    /// How many objects carry a marker of this name.
    pub objects: usize,
    /// A few of those objects, so one can be opened and looked at.
    pub examples: Vec<u32>,
    /// The named marker that most often sits nearest to this one, with the median distance
    /// across every object that carries both. This is what stands in for an unrecovered name.
    pub neighbour: Option<Neighbour>,
}

impl MarkerEntry {
    /// What to show in a list: the name, or the hash and what it sits beside.
    #[must_use]
    pub fn label(&self) -> String {
        match (self.name, self.neighbour) {
            (Some(name), _) => name.to_owned(),
            (None, Some(near)) => format!("0x{:08X} {near}", self.hash),
            (None, None) => format!("0x{:08X}", self.hash),
        }
    }
}

/// Every marker name in the game, in descending order of how many objects carry it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarkerIndex {
    pub entries: Vec<MarkerEntry>,
    /// Every object that carries any marker, with the markers it carries, sorted by tag.
    /// Only a few thousand objects in the game have a marker set, so holding them all is what
    /// makes the reverse lookup possible: from an object to the points on it.
    pub objects: Vec<MarkerObject>,
    /// Native names attached directly to object tags, when the package metadata has them.
    pub tag_names: std::collections::BTreeMap<u32, String>,
    /// How many objects were read, most of which carry no marker at all.
    pub scanned: usize,
    pub sets: usize,
}

/// One object and the markers on it.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkerObject {
    pub entity: u32,
    pub markers: Vec<Marker>,
}

impl MarkerIndex {
    #[must_use]
    pub fn named(&self) -> usize {
        self.entries.iter().filter(|e| e.name.is_some()).count()
    }

    /// The markers on one object.
    #[must_use]
    pub fn object(&self, entity: u32) -> Option<&MarkerObject> {
        self.objects
            .binary_search_by_key(&entity, |object| object.entity)
            .ok()
            .map(|index| &self.objects[index])
    }

    /// Where one marker sits on one object, when that object carries it.
    #[must_use]
    pub fn placement(&self, entity: u32, hash: u32) -> Option<&Marker> {
        self.object(entity)?
            .markers
            .iter()
            .find(|marker| marker.name == hash)
    }
}

/// How many objects of each name to keep as examples. Enough to look at, not enough to bloat.
const MAX_EXAMPLES: usize = 12;

/// Objects that carry markers, as read from the packages. The index is derived from this, and
/// this is what the disk keeps. Names are applied when the index is derived, so a release that
/// recovers more of them reads nothing again.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Scan {
    /// How many objects were read, most of which carry no marker at all.
    scanned: usize,
    /// In tag order.
    carriers: Vec<Carrier>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Carrier {
    entity: u32,
    /// How many marker-set components the object holds.
    sets: usize,
    markers: Vec<Marker>,
}

static CACHE: index_cache::Cache<Scan> = index_cache::Cache::new();

/// The error a cancelled read reports.
pub const CANCELLED: &str = "The marker read was cancelled";

/// The whole-game marker index, from the disk when these packages were read before.
///
/// An uncached first read opens every object in the game on a pool of workers, so it belongs on
/// a background thread. Later reads reuse unchanged object and component packages independently.
/// `progress` is called with how far it is, out of a fixed total. Setting `cancel` stops the
/// read. Complete package shards survive a cancelled read.
pub fn cached_index(
    packages: &Path,
    manager: &PackageManager,
    progress: impl Fn(usize, usize) + Sync,
    cancel: &AtomicBool,
) -> Result<MarkerIndex, String> {
    let scan = index_cache::cached(
        packages,
        crate::sandbox_perk::CACHE_DIRECTORY,
        "markers-v1",
        &CACHE,
        || scan(manager, &progress, cancel),
        |_| true,
    )?;
    Ok(index_with_tag_names(index_from(&scan), manager))
}

/// The whole-game marker index, read fresh from the packages.
pub fn build_index(
    manager: &PackageManager,
    progress: impl Fn(usize, usize) + Sync,
) -> MarkerIndex {
    let scan = scan(manager, &progress, &AtomicBool::new(false))
        .expect("a read nobody cancels runs to the end");
    index_with_tag_names(index_from(&scan), manager)
}

fn index_with_tag_names(mut index: MarkerIndex, manager: &PackageManager) -> MarkerIndex {
    let objects: BTreeSet<u32> = index.objects.iter().map(|object| object.entity).collect();
    index.tag_names = manager
        .lookup
        .named_tags
        .iter()
        .filter(|named| objects.contains(&named.hash.0))
        .map(|named| (named.hash.0, named.name.clone()))
        .collect();
    index
}

/// Reads every object's marker sets in two passes, each grouped by package.
///
/// Most of an object's components live in other packages than the object itself, and far
/// more packages hold them than the reader keeps open. Reading object by object therefore
/// reopened packages constantly and read shared components once per object. Reading every
/// object's component list first, then every distinct component once in package order, opens
/// each package about once per pass.
fn scan(
    manager: &PackageManager,
    progress: &(impl Fn(usize, usize) + Sync),
    cancel: &AtomicBool,
) -> Result<Scan, String> {
    use std::collections::BTreeMap;
    let stopped = || cancel.load(Ordering::Relaxed);
    let entities: Vec<u32> = manager
        .get_all_by_reference(ENTITY)
        .into_iter()
        .map(|(tag, _)| tag.0)
        .collect();
    let total = entities.len();
    // The two passes are reported as one figure. Objects are the first part, components
    // the rest, since there are several components to each object.
    let objects_share = PROGRESS_STEPS / 5;
    progress(0, PROGRESS_STEPS);

    let lists = shards::read(
        manager,
        shards::Kind::Objects,
        &package_jobs(&entities),
        |entity| entity_components(manager, entity),
        cancel,
        |read| progress(read * objects_share / total.max(1), PROGRESS_STEPS),
    )?;
    if stopped() {
        return Err(CANCELLED.to_owned());
    }

    let components: Vec<u32> = lists
        .iter()
        .filter_map(|(_, list)| list.as_ref().ok())
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let distinct = components.len();
    let read = shards::read(
        manager,
        shards::Kind::Components,
        &package_jobs(&components),
        |tag| component_markers(manager, tag),
        cancel,
        |finished| {
            progress(
                objects_share + finished * (PROGRESS_STEPS - objects_share) / distinct.max(1),
                PROGRESS_STEPS,
            )
        },
    )?;
    if stopped() {
        return Err(CANCELLED.to_owned());
    }
    let read: BTreeMap<u32, Result<Option<Vec<Marker>>, String>> = read.into_iter().collect();

    // Each object as `entity_marker_sets` would have read it: its marker sets in its own
    // order, and nothing at all when one of them does not parse.
    let mut carriers = Vec::new();
    for (entity, list) in lists {
        let Ok(list) = list else {
            continue;
        };
        let mut sets = 0;
        let mut markers = Vec::new();
        let mut whole = true;
        for tag in list {
            match read.get(&tag) {
                Some(Ok(Some(found))) => {
                    sets += 1;
                    markers.extend_from_slice(found);
                }
                Some(Ok(None)) => {}
                Some(Err(_)) | None => {
                    whole = false;
                    break;
                }
            }
        }
        if whole && sets > 0 {
            carriers.push(Carrier {
                entity,
                sets,
                markers,
            });
        }
    }
    carriers.sort_by_key(|carrier| carrier.entity);
    progress(PROGRESS_STEPS, PROGRESS_STEPS);
    Ok(Scan {
        scanned: total,
        carriers,
    })
}

/// How finely a read reports its progress.
const PROGRESS_STEPS: usize = 1000;

/// One job per package keeps workers on different reader locks. Splitting each package into
/// consecutive chunks puts the first several workers on the same lock and serializes the scan.
fn package_jobs(tags: &[u32]) -> Vec<Vec<u32>> {
    let mut by_package: std::collections::BTreeMap<u16, Vec<u32>> = Default::default();
    for &tag in tags {
        by_package
            .entry(tiger_pkg::TagHash(tag).pkg_id())
            .or_default()
            .push(tag);
    }
    by_package.into_values().collect()
}

/// Counts, examples and neighbours for every marker name, from the objects that carry them.
fn index_from(scan: &Scan) -> MarkerIndex {
    use std::collections::BTreeMap;
    let mut objects: BTreeMap<u32, usize> = BTreeMap::new();
    let mut examples: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    // Every nearest-named observation, so the reported distance is a median over objects
    // rather than whatever the first object happened to say.
    let mut near: BTreeMap<u32, BTreeMap<&'static str, Vec<f32>>> = BTreeMap::new();
    let mut sets = 0usize;
    let mut carriers = Vec::with_capacity(scan.carriers.len());
    for carrier in &scan.carriers {
        sets += carrier.sets;
        // Neighbours are measured across every set of an object at once, so one set holding
        // all of its markers answers the same.
        let found = [MarkerSet {
            entity: carrier.entity,
            component: 0,
            markers: carrier.markers.clone(),
        }];
        let mut seen = BTreeSet::new();
        for marker in &found[0].markers {
            if !seen.insert(marker.name) {
                continue;
            }
            *objects.entry(marker.name).or_default() += 1;
            let shown = examples.entry(marker.name).or_default();
            if shown.len() < MAX_EXAMPLES {
                shown.push(carrier.entity);
            }
            if let Some(neighbour) = nearest_named(&found, marker) {
                near.entry(marker.name)
                    .or_default()
                    .entry(neighbour.name)
                    .or_default()
                    .push(neighbour.distance);
            }
        }
        let [set] = found;
        carriers.push(MarkerObject {
            entity: carrier.entity,
            markers: set.markers,
        });
    }
    let mut entries: Vec<_> = objects
        .into_iter()
        .map(|(hash, count)| MarkerEntry {
            hash,
            name: marker_name(hash),
            objects: count,
            examples: examples.remove(&hash).unwrap_or_default(),
            neighbour: near.remove(&hash).and_then(|by_name| {
                // The name seen nearest most often wins; ties go to the closer one.
                by_name
                    .into_iter()
                    .map(|(name, mut distances)| {
                        distances.sort_by(f32::total_cmp);
                        let median = distances[distances.len() / 2];
                        (distances.len(), name, median)
                    })
                    .max_by(|a, b| a.0.cmp(&b.0).then(b.2.total_cmp(&a.2)))
                    .map(|(_, name, distance)| Neighbour { name, distance })
            }),
        })
        .collect();
    entries.sort_by(|a, b| b.objects.cmp(&a.objects).then(a.hash.cmp(&b.hash)));
    MarkerIndex {
        entries,
        // Already in tag order, which `object` relies on to find one without a second map.
        objects: carriers,
        tag_names: Default::default(),
        scanned: scan.scanned,
        sets,
    }
}

/// Every marker set reached from one gear-art arrangement, in entity order.
///
/// An appearance whose objects carry no markers yields an empty list rather than an error:
/// most of a weapon's parts are geometry alone.
pub fn read_appearance(packages: &Path, arrangement: u16) -> Result<Vec<MarkerSet>, String> {
    let manager = crate::investment::discovery::open_packages(packages)?;
    read_appearance_with_manager(&manager, arrangement, false)
}

/// As [`read_appearance`], with every alternative of every region: the barrels, sights and
/// magazines a socket can swap in, each carrying its own markers. A weapon aims with its sight
/// part's own set, which the default parts alone often lack.
pub fn read_appearance_parts(packages: &Path, arrangement: u16) -> Result<Vec<MarkerSet>, String> {
    let manager = crate::investment::discovery::open_packages(packages)?;
    read_appearance_with_manager(&manager, arrangement, true)
}

fn read_appearance_with_manager(
    manager: &PackageManager,
    arrangement: u16,
    alternatives: bool,
) -> Result<Vec<MarkerSet>, String> {
    let globals = manager.read_tag(resolve_live_named_tag(manager, "investment_globals", None)?)?;
    let table = checked(manager, u32_at(&globals, 0x430)?, GEAR_ART_TABLE)?;
    let assets = manager.read_tag(resolve_live_named_tag(manager, "investment_assets", None)?)?;
    let map = checked(manager, u32_at(&assets, 0x20)?, ASSIGNMENT_MAP)?;
    let (count, rows) = array(&map, 8, ASSIGNMENT_ROW, 8, 100_000)?;
    let mut entities = BTreeSet::new();
    for key in assignment_keys(&table, arrangement, alternatives)? {
        for index in 0..count {
            let row = rows + index * 8;
            if u32_at(&map, row)? != key {
                continue;
            }
            let relation = checked(manager, u32_at(&map, row + 4)?, RELATION)?;
            // An empty part names no entity and so carries no markers.
            let entity = u32_at(&relation, 0x10)?;
            if entity != u32::MAX {
                entities.insert(entity);
            }
            break;
        }
    }
    if entities.len()
        > if alternatives {
            MAX_ENTITIES * 4
        } else {
            MAX_ENTITIES
        }
    {
        return Err("This appearance draws more objects than markers are read for".into());
    }
    let mut sets = Vec::new();
    for entity in entities {
        sets.extend(entity_marker_sets(manager, entity)?);
    }
    Ok(sets)
}

/// The marker sets held by one gear entity.
fn entity_marker_sets(manager: &PackageManager, entity: u32) -> Result<Vec<MarkerSet>, String> {
    let mut sets = Vec::new();
    for tag in entity_components(manager, entity)? {
        if let Some(markers) = component_markers(manager, tag)? {
            sets.push(MarkerSet {
                entity,
                component: tag,
                markers,
            });
        }
    }
    Ok(sets)
}

/// The components one gear entity lists, in its own order.
fn entity_components(manager: &PackageManager, entity: u32) -> Result<Vec<u32>, String> {
    let payload = checked(manager, entity, ENTITY)?;
    let (count, rows) = array(&payload, 0x10, COMPONENT_ROW, 12, 4096)?;
    (0..count)
        .map(|index| u32_at(&payload, rows + index * 12))
        .collect()
}

/// The markers of one component, `None` when it is not a marker set.
///
/// A component row can point at anything the object holds, so an unreadable one is not a
/// marker set rather than an error. A marker set that does not parse is an error.
fn component_markers(manager: &PackageManager, tag: u32) -> Result<Option<Vec<Marker>>, String> {
    let Ok(component) = manager.read_tag(tag) else {
        return Ok(None);
    };
    match marker_data(&component)? {
        Some(data) => read_component(&component, data).map(Some),
        None => Ok(None),
    }
}

/// The data struct of `component` when it is a marker set, `None` when it is another component.
/// A marker set whose data struct cannot be reached is an error.
fn marker_data(component: &[u8]) -> Result<Option<usize>, String> {
    // The component's class is the word before its header, not before its data struct.
    let Ok(header) = pointer(component, 0x10) else {
        return Ok(None);
    };
    if header < 4 || u32_at(component, header - 4) != Ok(MARKER_COMPONENT) {
        return Ok(None);
    }
    pointer(component, 0x18).map(Some)
}

/// Whether `component`, a component's whole payload, is a marker set.
#[must_use]
pub fn is_marker_set(component: &[u8]) -> bool {
    matches!(marker_data(component), Ok(Some(_)))
}

/// Moves every row of a marker set whose name has an offset by that offset, in the model's own
/// units, which are metres. Rows that share a name move together, so a marker kept in two
/// orientations stays one point. Only positions change: the rows keep their count, names,
/// orientations and binding words, so nothing that references the set moves. Returns how many
/// rows moved.
pub fn offset_markers(component: &mut [u8], offsets: &[(u32, [f32; 3])]) -> Result<usize, String> {
    let data = marker_data(component)?.ok_or("The component is not a marker set")?;
    let descriptor = data
        .checked_add(MARKER_DESCRIPTOR)
        .ok_or("The marker array descriptor is outside the component")?;
    let (count, rows) = array(component, descriptor, MARKER_ROW, MARKER_STRIDE, 4096)?;
    let mut moved = 0;
    for index in 0..count {
        let row = rows + index * MARKER_STRIDE;
        let name = u32_at(component, row + MARKER_NAME)?;
        let Some((_, offset)) = offsets.iter().find(|(marker, _)| *marker == name) else {
            continue;
        };
        for (axis, delta) in offset.iter().enumerate() {
            let at = row + MARKER_POSITION + axis * 4;
            let value = f32::from_bits(u32_at(component, at)?) + delta;
            if !value.is_finite() {
                return Err("A moved marker has an unusable position".into());
            }
            component[at..at + 4].copy_from_slice(&value.to_bits().to_le_bytes());
        }
        moved += 1;
    }
    Ok(moved)
}

/// Moves every row of a marker set by `delta`, in metres, as when the whole model moves with
/// them. Only positions change, as in [`offset_markers`]. Returns how many rows moved.
pub fn shift_markers(component: &mut [u8], delta: [f32; 3]) -> Result<usize, String> {
    let data = marker_data(component)?.ok_or("The component is not a marker set")?;
    let descriptor = data
        .checked_add(MARKER_DESCRIPTOR)
        .ok_or("The marker array descriptor is outside the component")?;
    let (count, rows) = array(component, descriptor, MARKER_ROW, MARKER_STRIDE, 4096)?;
    for index in 0..count {
        let row = rows + index * MARKER_STRIDE;
        for (axis, delta) in delta.iter().enumerate() {
            let at = row + MARKER_POSITION + axis * 4;
            let value = f32::from_bits(u32_at(component, at)?) + delta;
            if !value.is_finite() {
                return Err("A moved marker has an unusable position".into());
            }
            component[at..at + 4].copy_from_slice(&value.to_bits().to_le_bytes());
        }
    }
    Ok(count)
}

/// The markers of one marker-set component, given its data struct's offset.
fn read_component(component: &[u8], data: usize) -> Result<Vec<Marker>, String> {
    let descriptor = data
        .checked_add(MARKER_DESCRIPTOR)
        .ok_or("The marker array descriptor is outside the component")?;
    let (count, rows) = array(component, descriptor, MARKER_ROW, MARKER_STRIDE, 4096)?;
    (0..count)
        .map(|index| {
            let row = rows + index * MARKER_STRIDE;
            let read = |at: usize| -> Result<f32, String> {
                let value = f32::from_bits(u32_at(component, at)?);
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err("A marker has an unusable transform".to_owned())
                }
            };
            let mut position = [0.0; 3];
            for (axis, value) in position.iter_mut().enumerate() {
                *value = read(row + MARKER_POSITION + axis * 4)?;
            }
            let mut orientation = [0.0; 4];
            for (axis, value) in orientation.iter_mut().enumerate() {
                *value = read(row + MARKER_ORIENTATION + axis * 4)?;
            }
            Ok(Marker {
                name: u32_at(component, row + MARKER_NAME)?,
                position,
                orientation,
            })
        })
        .collect()
}

/// The gear-art assignment keys of one arrangement row. A region lists alternatives rather
/// than simultaneous geometry, so only its base attachment is followed, matching the preview.
fn assignment_keys(
    table: &[u8],
    arrangement: u16,
    alternatives: bool,
) -> Result<BTreeSet<u32>, String> {
    let (count, rows) = array(table, 8, ARRANGEMENT_ROW, 0x20, 65536)?;
    if usize::from(arrangement) >= count {
        return Err("Appearance row is outside the installed table".into());
    }
    let row = rows + usize::from(arrangement) * 0x20;
    let mut keys = BTreeSet::new();
    if u64_at(table, row + 0x10)? == 0 {
        keys.insert(u32_at(table, row + 8)?);
        keys.insert(u32_at(table, row + 12)?);
    } else {
        let (count, rows) = array(table, row + 0x10, ARRANGEMENT_SLOTS, 8, 4096)?;
        for index in 0..count {
            let resource = pointer(table, rows + index * 8)?;
            let (count, rows) = array(table, resource + 8, ARRANGEMENT_KEYS, 4, 65536)?;
            let followed = if alternatives { count } else { count.min(1) };
            for key in 0..followed {
                keys.insert(u32_at(table, rows + key * 4)?);
            }
        }
    }
    keys.remove(&0);
    keys.remove(&u32::MAX);
    keys.remove(&crate::hash::FNV1_EMPTY_HASH);
    Ok(keys)
}

fn pointer(bytes: &[u8], offset: usize) -> Result<usize, String> {
    relative_offset(offset, 0, i64_at(bytes, offset)?)
}

fn checked(manager: &PackageManager, tag: u32, class: u32) -> Result<Vec<u8>, String> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| format!("Resource 0x{tag:08X} is missing"))?;
    if entry.reference != class {
        return Err(format!("Resource 0x{tag:08X} is not a 0x{class:08X}"));
    }
    manager.read_tag(tag)
}

fn array(
    bytes: &[u8],
    offset: usize,
    class: u32,
    stride: usize,
    limit: usize,
) -> Result<(usize, usize), String> {
    let (count, _, rows, actual) = native_array_at(bytes, offset)?;
    // A native empty array has a zero pointer and no header, so it has no class to check.
    if count != 0 && actual != class {
        return Err(format!(
            "Array at {offset:#x} has class {actual:08X}, expected {class:08X}"
        ));
    }
    if count > limit
        || count
            .checked_mul(stride)
            .and_then(|size| rows.checked_add(size))
            .is_none_or(|end| end > bytes.len())
    {
        return Err(format!(
            "Array at {offset:#x} holds {count} rows of {stride} bytes, which its payload cannot"
        ));
    }
    Ok((count, rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::fnv1_name_hash as fnv1;

    /// A recovered name is only a name if it hashes to the key it is filed under. This is what
    /// makes the table safe to extend by hand.
    #[test]
    fn every_recovered_name_hashes_to_its_key() {
        assert!(NAMES.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(PATH_NAMES.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for (hash, name) in NAMES.iter().chain(PATH_NAMES) {
            assert_eq!(fnv1(name), *hash, "{name}");
        }
        let hashes: BTreeSet<_> = NAMES
            .iter()
            .chain(PATH_NAMES)
            .map(|(hash, _)| *hash)
            .collect();
        assert_eq!(
            hashes.len(),
            NAMES.len() + PATH_NAMES.len(),
            "duplicate key in the table"
        );
        assert_eq!(marker_name(0xF2A0_71CC), Some("primary_fire"));
        assert_eq!(marker_name(0), None);
    }

    /// A hash says nothing on its own. Naming what it sits next to is the whole point of the
    /// explorer, so an unnamed marker has to come back described by a named neighbour.
    #[test]
    fn an_unnamed_marker_is_described_by_what_it_sits_beside() {
        let at = |name: u32, position: [f32; 3]| Marker {
            name,
            position,
            orientation: [0.0, 0.0, 0.0, 1.0],
        };
        let sets = vec![MarkerSet {
            entity: 0x8087_1F2A,
            component: 0x8161_ECC9,
            markers: vec![
                at(fnv1("primary_fire"), [0.3806, 0.0, 0.1358]),
                at(0x76BC_E859, [0.3717, 0.0, 0.1421]),
                at(fnv1("grip"), [0.1, 0.0, 0.0]),
                // Far enough away that nothing on the weapon is "near" it.
                at(0x0B7B_A45D, [9.0, 9.0, 9.0]),
            ],
        }];
        let marker = sets[0].markers[1];
        let near = nearest_named(&sets, &marker).unwrap();
        // `grip` is also named but much further off, so the nearer one wins.
        assert_eq!(near.name, "primary_fire");
        assert!((near.distance - 0.0109).abs() < 1e-3, "{near:?}");
        assert_eq!(describe(&sets, &marker), "0x76BCE859 by primary_fire 11mm");
        // Markers that share a point are common, and "0mm" reads as a missing number.
        let stacked = at(0x1234_5678, [0.3806, 0.0, 0.1358]);
        assert_eq!(
            nearest_named(&sets, &stacked).unwrap().to_string(),
            "on primary_fire"
        );
        // A named marker describes itself and has no neighbour.
        assert_eq!(nearest_named(&sets, &sets[0].markers[0]), None);
        assert_eq!(describe(&sets, &sets[0].markers[0]), "primary_fire");
        // Nothing within reach means no claim is made at all.
        assert_eq!(nearest_named(&sets, &sets[0].markers[3]), None);
        assert_eq!(describe(&sets, &sets[0].markers[3]), "0x0B7BA45D");
    }

    /// Builds one marker-set component: a data struct at 0x20 whose descriptor at +0xB0 owns a
    /// 16-byte array header followed by fixed-stride rows.
    fn component(markers: &[(u32, [f32; 3])]) -> Vec<u8> {
        let data = 0x20usize;
        let descriptor = data + MARKER_DESCRIPTOR;
        let header = descriptor + 0x30;
        let rows = header + 16;
        let mut bytes = vec![0u8; rows + MARKER_STRIDE * markers.len()];
        let count = markers.len() as u64;
        write_bytes(&mut bytes, 0x18, &(data as i64 - 0x18).to_le_bytes()).unwrap();
        write_bytes(&mut bytes, data - 4, &MARKER_COMPONENT.to_le_bytes()).unwrap();
        write_bytes(&mut bytes, descriptor, &count.to_le_bytes()).unwrap();
        let delta = header as i64 - (descriptor + 8) as i64;
        write_bytes(&mut bytes, descriptor + 8, &delta.to_le_bytes()).unwrap();
        write_bytes(&mut bytes, header, &count.to_le_bytes()).unwrap();
        write_bytes(&mut bytes, header + 8, &MARKER_ROW.to_le_bytes()).unwrap();
        for (index, (name, position)) in markers.iter().enumerate() {
            let row = rows + index * MARKER_STRIDE;
            for (axis, value) in position.iter().enumerate() {
                write_bytes(
                    &mut bytes,
                    row + MARKER_POSITION + axis * 4,
                    &value.to_le_bytes(),
                )
                .unwrap();
            }
            write_bytes(&mut bytes, row + MARKER_NAME, &name.to_le_bytes()).unwrap();
        }
        bytes
    }

    #[test]
    fn a_damaged_marker_component_is_refused_rather_than_read_past() {
        let bytes = component(&[(fnv1("grip"), [0.0; 3])]);
        assert!(read_component(&bytes[..bytes.len() - 1], 0x20).is_err());
        assert!(read_component(&bytes, usize::MAX).is_err());
        let header = 0x20 + MARKER_DESCRIPTOR + 0x30;
        let mut wrong_class = bytes.clone();
        write_bytes(&mut wrong_class, header + 8, &0x8080_0000u32.to_le_bytes()).unwrap();
        assert!(read_component(&wrong_class, 0x20).is_err());
        let mut infinite = bytes;
        write_bytes(
            &mut infinite,
            header + 16 + MARKER_POSITION,
            &f32::INFINITY.to_le_bytes(),
        )
        .unwrap();
        assert!(read_component(&infinite, 0x20).is_err());
    }

    #[test]
    fn an_empty_marker_set_is_not_an_error() {
        assert_eq!(read_component(&component(&[]), 0x20), Ok(Vec::new()));
    }

    /// The fixtures above cannot prove the walk reaches a real weapon's markers: the
    /// arrangement, assignment and relation chain only exists in installed packages.
    #[test]
    #[ignore = "requires installed Shadowkeep packages"]
    fn installed_weapons_carry_named_markers() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
        let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
        let mut failures = Vec::new();
        for name in [
            "Better Devils",
            "Age-Old Bond",
            "Chroma Rush",
            "Black Talon",
        ] {
            let Some(summary) = catalog.weapon_donors().into_iter().find(|d| d.name == name) else {
                continue;
            };
            let donor = catalog.weapon_donor(summary.hash).unwrap();
            let arrangement = donor.art_arrangements[0].arrangement;
            let sets = match read_appearance(&packages, arrangement) {
                Ok(sets) => sets,
                Err(error) => {
                    failures.push(format!("{name}: {error}"));
                    continue;
                }
            };
            let markers: Vec<_> = sets.iter().flat_map(|set| &set.markers).collect();
            let labels: Vec<_> = markers.iter().map(|marker| marker.label()).collect();
            println!("{name} arrangement {arrangement}:");
            for (set_index, set) in sets.iter().enumerate() {
                for marker in &set.markers {
                    let [x, y, z] = marker.position;
                    println!(
                        "    obj{set_index} {:<22} {x:>9.4} {y:>9.4} {z:>9.4}",
                        marker.label()
                    );
                }
            }
            if markers.is_empty() {
                failures.push(format!("{name}: no markers"));
            } else if !labels.iter().any(|label| !label.starts_with("0x")) {
                failures.push(format!("{name}: no marker resolved to a name"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The index behind the explorer, against real packages. The fixtures prove the
    /// arithmetic; this proves the whole-game walk produces rows worth showing.
    #[test]
    #[ignore = "requires installed Shadowkeep packages"]
    #[allow(clippy::cognitive_complexity)]
    fn the_marker_index_describes_the_unnamed_ones() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
        let manager = crate::investment::discovery::open_packages(&packages).unwrap();
        let ticks = AtomicUsize::new(0);
        let index = build_index(&manager, |_, _| {
            ticks.fetch_add(1, Ordering::Relaxed);
        });
        let ticks = ticks.into_inner();
        println!(
            "{} objects read, {} carry markers, {} marker sets, {} names, {} recovered",
            index.scanned,
            index.objects.len(),
            index.sets,
            index.entries.len(),
            index.named()
        );
        assert!(ticks > 1, "progress must be reported while reading");
        assert!(!index.entries.is_empty());
        assert!(index.named() > 0);
        // The reverse lookup: every object listed carries markers, and every marker's examples
        // resolve back to an object that really holds it.
        assert!(!index.objects.is_empty() && index.objects.len() <= index.scanned);
        assert!(index.objects.iter().all(|o| !o.markers.is_empty()));
        assert!(
            index.objects.windows(2).all(|w| w[0].entity < w[1].entity),
            "objects must be sorted for lookup"
        );
        for entry in index.entries.iter().take(50) {
            for example in &entry.examples {
                assert!(
                    index.placement(*example, entry.hash).is_some(),
                    "{:08X} is not on its example {example:08X}",
                    entry.hash
                );
            }
        }
        // Sorted by reach, so the first rows are the ones worth naming next.
        assert!(
            index
                .entries
                .windows(2)
                .all(|w| w[0].objects >= w[1].objects)
        );
        let described = index
            .entries
            .iter()
            .filter(|entry| entry.name.is_none() && entry.neighbour.is_some())
            .count();
        let unnamed = index.entries.iter().filter(|e| e.name.is_none()).count();
        println!("{described} of {unnamed} unnamed markers have a named neighbour");
        for entry in index.entries.iter().take(20) {
            println!(
                "  {:<40} {:>5} objects  examples {:08X?}",
                entry.label(),
                entry.objects,
                entry.examples.iter().take(3).collect::<Vec<_>>()
            );
        }
        for entry in &index.entries {
            assert!(!entry.examples.is_empty());
            assert!(entry.examples.len() <= MAX_EXAMPLES);
        }
    }

    fn carrier(entity: u32, sets: usize, markers: &[(u32, [f32; 3])]) -> Carrier {
        Carrier {
            entity,
            sets,
            markers: markers
                .iter()
                .map(|&(name, position)| Marker {
                    name,
                    position,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                })
                .collect(),
        }
    }

    #[test]
    fn marker_scan_schedules_each_package_once() {
        let tags: Vec<u32> = (0..600)
            .map(|index| tiger_pkg::TagHash::new(0x123, index).0)
            .chain((0..300).map(|index| tiger_pkg::TagHash::new(0x456, index).0))
            .collect();
        let jobs = package_jobs(&tags);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0], tags[..600]);
        assert_eq!(jobs[1], tags[600..]);
    }

    /// The index is derived from what the disk keeps, so a scan that went through the cache
    /// file must describe the game exactly as the scan that was written.
    #[test]
    fn a_scan_read_back_from_the_cache_gives_the_same_index() {
        let named = crate::hash::fnv1_name_hash("primary_fire");
        assert!(marker_name(named).is_some());
        let scan = Scan {
            scanned: 40,
            carriers: vec![
                carrier(
                    0x8080_0001,
                    2,
                    &[(named, [0.0, 0.0, 0.0]), (0x1234_5678, [0.0, 0.011, 0.0])],
                ),
                carrier(
                    0x8080_0002,
                    1,
                    &[
                        (0x1234_5678, [0.1, 0.1, 0.1]),
                        (0x1234_5678, [0.2, 0.2, 0.2]),
                    ],
                ),
            ],
        };
        let mut bytes = Vec::new();
        crate::package_runtime::cache_file::write(&mut bytes, &scan).unwrap();
        let read: Scan = crate::package_runtime::cache_file::read(&bytes).unwrap();
        let index = index_from(&scan);
        assert_eq!(index_from(&read), index);

        assert_eq!(index.scanned, 40);
        assert_eq!(index.sets, 3);
        assert_eq!(index.objects.len(), 2);
        assert!(index.object(0x8080_0002).is_some());
        // Counted once per object that carries it, however many rows it has there.
        let unnamed = index
            .entries
            .iter()
            .find(|e| e.hash == 0x1234_5678)
            .unwrap();
        assert_eq!(unnamed.objects, 2);
        assert_eq!(unnamed.examples, vec![0x8080_0001, 0x8080_0002]);
        assert_eq!(unnamed.neighbour.map(|n| n.name), Some("primary_fire"));
        assert_eq!(index.entries[0].hash, 0x1234_5678, "the widest name leads");
    }

    /// The parallel read against real packages: the same objects as reading them one by one,
    /// progress that ends at the total, and a cancel that stops it without a result.
    #[test]
    #[ignore = "requires installed Shadowkeep packages"]
    fn the_parallel_read_matches_a_serial_one_and_cancels() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
        let manager = crate::investment::discovery::open_packages(&packages).unwrap();
        let last = std::sync::Mutex::new((0, 0));
        let started = std::time::Instant::now();
        let parallel = scan(
            &manager,
            &|done, total| {
                let mut last = last.lock().unwrap();
                *last = (last.0.max(done), total);
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        println!("parallel read: {:?}", started.elapsed());
        let (done, total) = *last.lock().unwrap();
        assert!(done > 0);
        assert_eq!(done, total);

        // One object in seven read the old way, one at a time, which is too slow for all.
        let started = std::time::Instant::now();
        let mut serial = Vec::new();
        for (tag, _) in manager.get_all_by_reference(ENTITY).into_iter().step_by(7) {
            if let Ok(sets) = entity_marker_sets(&manager, tag.0)
                && !sets.is_empty()
            {
                let markers: Vec<_> = sets.iter().flat_map(|set| set.markers.clone()).collect();
                serial.push((tag.0, sets.len(), markers));
            }
        }
        println!("serial read of a seventh: {:?}", started.elapsed());
        assert!(!serial.is_empty());
        for (entity, sets, markers) in serial {
            let carrier = parallel
                .carriers
                .binary_search_by_key(&entity, |c| c.entity)
                .map(|at| &parallel.carriers[at])
                .unwrap_or_else(|_| panic!("0x{entity:08X} is missing from the parallel read"));
            assert_eq!(
                (carrier.sets, &carrier.markers),
                (sets, &markers),
                "0x{entity:08X}"
            );
        }
        let sampled: BTreeSet<u32> = manager
            .get_all_by_reference(ENTITY)
            .into_iter()
            .step_by(7)
            .map(|(tag, _)| tag.0)
            .collect();
        for carrier in parallel
            .carriers
            .iter()
            .filter(|c| sampled.contains(&c.entity))
        {
            assert!(
                entity_marker_sets(&manager, carrier.entity).is_ok_and(|sets| !sets.is_empty()),
                "0x{:08X} carries no markers when read alone",
                carrier.entity
            );
        }

        let cancelled = scan(&manager, &|_, _| {}, &AtomicBool::new(true));
        assert_eq!(cancelled.unwrap_err(), CANCELLED);
    }
}
