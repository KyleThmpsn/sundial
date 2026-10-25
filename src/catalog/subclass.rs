//! Native Shadowkeep selection contract shared by catalog choices and validation.

pub(crate) struct Rules {
    pub name: &'static str,
    pub middle_super: u8,
}

impl Rules {
    pub fn supports_pair(&self, super_ability: u64, melee: u64) -> bool {
        [(10, 11), (10, 15), (u64::from(self.middle_super), 21)].contains(&(super_ability, melee))
    }

    pub fn repair_pair(&self, super_ability: u8, melee: u8) -> (u8, u8) {
        // The melee entry identifies the tree for every Shadowkeep subclass.
        // Prefer it, then a distinctive middle-tree super, then the top tree.
        match melee {
            11 => (10, 11),
            15 => (10, 15),
            21 => (self.middle_super, 21),
            _ if super_ability == 20 => (self.middle_super, 21),
            _ => (10, 11),
        }
    }
}

pub(crate) const fn rules(subclass_hash: u64) -> Option<Rules> {
    let (name, middle_super) = match subclass_hash {
        // Their middle-tree entry 20 contributes a hash, not a selectable
        // bucket kind. Keep super entry 10; melee entry 21 selects the tree.
        0x4F91_DC97 => ("Arcstrider", 10),
        0xC99B_33E9 => ("Sentinel", 10),
        0xB055_4739 => ("Striker", 20),
        0xB920_CE9A => ("Sunbreaker", 20),
        0xD8B8_D1FC => ("Gunslinger", 20),
        0xC048_3D8B => ("Nightstalker", 20),
        0xCF88_FEA5 => ("Dawnblade", 20),
        0x686A_154A => ("Stormcaller", 20),
        0xE7BC_88B0 => ("Voidwalker", 20),
        _ => return None,
    };
    Some(Rules { name, middle_super })
}
