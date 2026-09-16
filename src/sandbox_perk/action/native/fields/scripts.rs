//! The behavior scripts effect kind 48 runs, as the stock perks ship them.
//!
//! Kind 48 references a runtime resource by path and tag. The path names the script in the
//! game's own words (apply_tiered_charge_of_light, remove_one_stack_of_charged_with_light),
//! so a script is chosen by that name rather than typed as a path. Every entry is one the
//! installed stock perks reference, with how many of their nodes do and which perks.
//!
//! Generated from the scratch census `taginfo/src/bin/keys.rs` over the 1,800 stock perk
//! actions from the clean Shadowkeep packages. Regenerate it when the corpus changes.

/// One behavior script a stock perk runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Script {
    pub path: &'static str,
    pub tag: u32,
    /// How many stock kind 48 nodes reference it.
    pub uses: u32,
    /// Named stock perks that reference it, up to six.
    pub perks: &'static str,
}

impl Script {
    /// The script's file name in title case, the name a reader picks it by.
    #[must_use]
    pub fn title(&self) -> String {
        let file = self.path.rsplit('\\').next().unwrap_or(self.path);
        let stem = file.split('.').next().unwrap_or(file);
        stem.split('_')
            .filter(|word| !word.is_empty())
            .map(|word| {
                let mut chars = word.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                })
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub const SCRIPTS: &[Script] = &[
    Script {
        path: r"content\sandbox\talents\armor_d2\mods\seasonal\season9\components\apply_tiered_charge_of_light.object_behaviors.tft",
        tag: 0x8157_8792,
        uses: 17,
        perks: "Taking Charge, Shield Break Charge, Empowered Finish, Quick Charge, Blast Radius, Precisely Charged",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\mods\seasonal\season9\components\remove_one_stack_of_charged_with_light.object_behaviors.tft",
        tag: 0x8161_E68C,
        uses: 12,
        perks: "High-Energy Fire, Striking Light, Heal Thyself",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\mods\seasonal\season9\components\remove_all_stacks_of_charged_with_light.object_behaviors.tft",
        tag: 0x8157_89E4,
        uses: 3,
        perks: "Extra Reserves, Surprise Attack, Energy Converter",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\titan\exotic\chest\empower_abilities\hopon\empower_grenade.object_behaviors.tft",
        tag: 0x80BB_C7C3,
        uses: 2,
        perks: "Overflowing Light",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\titan\exotic\chest\empower_abilities\hopon\grenade_thrown_not_shield.object_behaviors.tft",
        tag: 0x80BB_CB28,
        uses: 1,
        perks: "Overflowing Light",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\mods\seasonal\season9\damage_reduction_when_shields_gone.object_behaviors.tft",
        tag: 0x8157_8A3F,
        uses: 1,
        perks: "Protective Light",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\mods\seasonal\season11\void\bonus_damage\components\set_initial_bonus_damage_channel.object_behaviors.tft",
        tag: 0x8162_C855,
        uses: 1,
        perks: "Surprise Attack",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\mods\activity\nightmare_hunts\nightmare_mod3\components\nightmare_mod3.object_behaviors.tft",
        tag: 0x80BC_2979,
        uses: 1,
        perks: "Dreambane Mod",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\hunter\exotic\arms\arc_counter_punch\arc_counter_punch.object_behaviors.tft",
        tag: 0x80BC_298C,
        uses: 1,
        perks: "Cross Counter",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\hunter\exotic\arms\super_energy_on_knife_super_kill\super_energy_on_knife_super.object_behaviors.tft",
        tag: 0x80BC_2B4F,
        uses: 1,
        perks: "Sharp Edges",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\warlock\exotic\chest\elemental_matching_buff\multi_bloom.object_behaviors.tft",
        tag: 0x80BB_CC00,
        uses: 1,
        perks: "Crystalline Transistor",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\warlock\exotic\head\arc_overcharge\hop_on\arc_overcharge_super_sustain.object_behaviors.tft",
        tag: 0x80BC_2952,
        uses: 1,
        perks: "Conduction Tines",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\warlock\exotic\legs\orbs_grant_extended_dawnblade\extended_dawnblade\components\remove_one_stack_of_extended_dawnblade.object_behaviors.tft",
        tag: 0x8157_9413,
        uses: 1,
        perks: "Embers of Light",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\warlock\exotic\legs\orbs_grant_extended_dawnblade\extended_dawnblade\remove_all_stacks_of_extended_dawnblade.object_behaviors.tft",
        tag: 0x815B_3C71,
        uses: 1,
        perks: "",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\mamba\bonus_near_bank\bonus_near_bank.object_behaviors.tft",
        tag: 0x80BC_2AE0,
        uses: 1,
        perks: "",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\mamba\mark_hvt_on_damage\mamba_mark_hvt.object_behaviors.tft",
        tag: 0x80BC_2B16,
        uses: 1,
        perks: "",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\mamba\mark_hvt_on_damage\mamba_mark_invader.object_behaviors.tft",
        tag: 0x80BC_2991,
        uses: 1,
        perks: "",
    },
    Script {
        path: r"content\sandbox\talents\armor_d2\mamba\taken_buff_for_killing_mobs\taken_buff_for_killing_mobs.object_behaviors.tft",
        tag: 0x80BC_2BC1,
        uses: 1,
        perks: "",
    },
    Script {
        path: r"content\sandbox\v410\talents\weapons\support_increased_splash_damage\components\support_increased_splash_damage.object_behaviors.tft",
        tag: 0x80BC_2A7E,
        uses: 1,
        perks: "Dragonfly",
    },
    Script {
        path: r"content\sandbox\v420\weapons\outbreak_prime\attach_variant_perk\outbreak_attach_counter.object_behaviors.tft",
        tag: 0x80BC_2B2B,
        uses: 1,
        perks: "Parasitism",
    },
    Script {
        path: r"content\sandbox\v420\weapons\pinnacle\firefly_buildup_rapid\v2\firefly_buildup_applicator.object_behaviors.tft",
        tag: 0x80BC_2A16,
        uses: 1,
        perks: "Meganeura",
    },
];

/// The stock script with this resource tag, if any.
#[must_use]
pub fn by_tag(tag: u32) -> Option<&'static Script> {
    SCRIPTS.iter().find(|script| script.tag == tag)
}
