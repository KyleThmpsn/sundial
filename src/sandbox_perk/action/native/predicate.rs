//! Names and values read from compiled comparisons, checked against their native bindings.
use super::{Graph, value};
#[cfg(test)]
use crate::sandbox_perk::action::native::NodeKind as NativeNodeKind;

pub const COMPARISON_CLASS: u32 = 0x80804D7D;

pub struct Comparison {
    pub name: String,
    pub operation: &'static str,
    pub threshold: u32,
    /// The first scalar lane of this native constant block holds the threshold.
    pub constant_block: usize,
}

pub fn read(graph: &Graph, index: usize) -> Option<Comparison> {
    let block = graph.blocks.get(index)?;
    if block.class != COMPARISON_CLASS {
        return None;
    }
    let bindings = graph.blocks.get(*block.links.get(&16)?)?;
    if bindings.class != 0x8080941B || bindings.count != Some(1) {
        return None;
    }
    let key = u32::from_le_bytes(bindings.bytes.get(4..8)?.try_into().ok()?);
    let source = graph.blocks.get(*block.links.get(&0)?)?;
    if source.class != 0 {
        return None;
    }
    let text = std::str::from_utf8(&source.bytes)
        .ok()?
        .trim_end_matches('\0');
    let variable = text.split_whitespace().next()?;
    if !variable
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    let hash = variable.bytes().fold(0x811C9DC5u32, |hash, byte| {
        hash.wrapping_mul(0x01000193) ^ u32::from(byte)
    });
    if hash != key {
        return None;
    }
    let left = value::Program::read(graph, index, 24).ok()?;
    let right = value::Program::read(graph, index, 88).ok()?;
    let input = [
        value::Instruction {
            opcode: 60,
            operand: Some(0),
        },
        value::Instruction {
            opcode: 62,
            operand: Some(0),
        },
    ];
    let literal = [
        value::Instruction {
            opcode: 52,
            operand: Some(0),
        },
        value::Instruction {
            opcode: 62,
            operand: Some(0),
        },
    ];
    if left.fast_path != 0
        || right.fast_path != 0
        || left.instructions != input
        || right.instructions != literal
        || right.constants.len() != 1
    {
        return None;
    }
    let operation = match block.bytes.get(136)? {
        0 => "=",
        3 => ">=",
        4 => "<",
        5 => ">",
        _ => return None,
    };
    let name = title(variable);
    Some(Comparison {
        name,
        operation,
        threshold: right.constants[0][0],
        constant_block: *block.links.get(&112)?,
    })
}

/// An engine variable in title case, small words kept lowercase inside the title, so
/// bloom_catalyst_on_cooldown reads "Bloom Catalyst on Cooldown".
fn title(variable: &str) -> String {
    let words = variable.split('_').collect::<Vec<_>>();
    words
        .iter()
        .enumerate()
        .map(|(index, word)| crate::sandbox_perk::nodes::title_word(word, index, words.len()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The plain name of a compared engine variable, where the stock perks that compare it
/// establish one. The variable names themselves come from the compiled source string the
/// client keeps, so they are engine names; this only puts them in the words a player uses.
#[must_use]
pub fn plain_variable(name: &str) -> &str {
    match name {
        "Nearby Ally Count" => "Nearby Allies",
        // Osmosis ("changes this weapon's damage type to match your subclass") and
        // Elemental Capacitor ("based on the currently equipped subclass") gate their Arc,
        // Solar and Void effects on these three, so each is the equipped subclass element.
        // Thermal is the engine's word for Solar, the one element the other two leave.
        "Is Arc" => "Subclass Is Arc",
        "Is Thermal" => "Subclass Is Solar",
        "Is Void" => "Subclass Is Void",
        "Equipped Item Magazine Fraction" => "Magazine Fraction",
        "Is Guarding with Sword" => "Guarding with a Sword",
        // "Bauble" is the engine's word for a Warmind Cell: Blessing of Rasputin reads
        // "collecting a Warmind Cell increases the chances that your next final blow with a
        // Seraph weapon will create a Warmind Cell", which is warmind_cells_increase_bauble_
        // chance word for word, and the Seraph weapon perks check all three of these before
        // spawning a cell.
        "Rasputin Weapon Equipped" => "Seraph Weapon Equipped",
        "Solar Splash Spawn Baubles" => "Solar Splash Spawns Warmind Cells",
        "Warmind Cells Increase Bauble Chance" => "Warmind Cells Increase Cell Chance",
        // Ionic Return pairs each of these with the state it mirrors, so each reads as
        // that state with a recency the traced code does not pin down.
        "Super Active Recent" => "Super Recently Active",
        "Is Arc Recent" => "Subclass Recently Arc",
        "Is Thermal Recent" => "Subclass Recently Solar",
        "Is Void Recent" => "Subclass Recently Void",
        // Judgment ("envelops the target in a field that weakens and disrupts them") is
        // the only perk comparing this, and cage is the engine's word for that field.
        "Siphon Gun Overload No Cage" => "Weakening Field Not Formed",
        // Surrounded compares this on both sides of 1 beside its enemy count, so it picks
        // between two paths rather than counting anything.
        "Support Nearby Enemy" => "Nearby Enemy Count Supported",
        other => other,
    }
}

pub fn describe(graph: &Graph) -> Option<String> {
    let matches = comparisons(graph, 0);
    if matches.len() != 1 {
        return None;
    }
    let comparison = &matches[0];
    let value = f32::from_bits(comparison.threshold);
    if !value.is_finite() {
        return None;
    }
    let name = plain_variable(&comparison.name);
    let description = format!("{name} {} {value}", comparison.operation);
    Some(if graph.blocks.first()?.bytes.get(0xF8) == Some(&1) {
        format!("Not ({description})")
    } else {
        description
    })
}

/// An engine variable the stock perks compare, with the comparison they use as a default.
/// The variable names come from the compiled source string the client keeps, so authoring
/// one of these reproduces a comparison the game itself makes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Variable {
    pub name: &'static str,
    pub plain: &'static str,
    pub evidence: &'static str,
    pub operation: &'static str,
    pub threshold: f32,
}

pub const VARIABLES: &[Variable] = &[
    Variable {
        name: "nearby_enemy_count",
        plain: "Nearby Enemy Count",
        evidence: "Compared by Surrounded, Heavy Handed and Reactive Pulse, which all read \"surrounded\" or \"three or more enemies in close proximity\", each on > 2.",
        operation: ">",
        threshold: 2.0,
    },
    Variable {
        name: "nearby_ally_count",
        plain: "Nearby Allies",
        evidence: "Compared by Firing Line (\"near two or more allies\") on >= 1.5.",
        operation: ">=",
        threshold: 1.5,
    },
    Variable {
        name: "is_arc",
        plain: "Subclass Is Arc",
        evidence: "Osmosis (\"match your subclass\") and Elemental Capacitor (\"the currently equipped subclass\") gate their Arc effects on this = 1.",
        operation: "=",
        threshold: 1.0,
    },
    Variable {
        name: "is_thermal",
        plain: "Subclass Is Solar",
        evidence: "Osmosis and Elemental Capacitor gate their Solar effects on this = 1. Thermal is the engine's word for the one element the Arc and Void variables leave.",
        operation: "=",
        threshold: 1.0,
    },
    Variable {
        name: "is_void",
        plain: "Subclass Is Void",
        evidence: "Osmosis and Elemental Capacitor gate their Void effects on this = 1.",
        operation: "=",
        threshold: 1.0,
    },
    Variable {
        name: "equipped_item_magazine_fraction",
        plain: "Magazine Fraction",
        evidence: "Compared by one stock perk, an Overload Rounds variant, on > 0. The name is the engine's own.",
        operation: ">",
        threshold: 0.0,
    },
    Variable {
        name: "is_guarding_with_sword",
        plain: "Guarding with a Sword",
        evidence: "Compared by Energy Transfer (\"guarding while receiving damage\") on > 0.",
        operation: ">",
        threshold: 0.0,
    },
    Variable {
        name: "melee_energy",
        plain: "Melee Energy",
        evidence: "Compared by Heavy Handed (\"regain half of your melee energy when you use a charged melee\") on = 1.",
        operation: "=",
        threshold: 1.0,
    },
    Variable {
        name: "melee_overcharge_state",
        plain: "Melee Overcharge State",
        evidence: "Compared by Heavy Handed beside its melee energy check, on > 0.",
        operation: ">",
        threshold: 0.0,
    },
    Variable {
        name: "rasputin_weapon_equipped",
        plain: "Seraph Weapon Equipped",
        evidence: "The engine name rasputin_weapon_equipped, compared on > 0 by the Seraph weapon perks that spawn Warmind Cells. Blessing of Rasputin calls them \"Seraph weapons\".",
        operation: ">",
        threshold: 0.0,
    },
    Variable {
        name: "solar_splash_spawn_baubles",
        plain: "Solar Splash Spawns Warmind Cells",
        evidence: "The engine name solar_splash_spawn_baubles, compared on > 0 by the Seraph weapon perks. Bauble is the engine's word for a Warmind Cell, and this is Wrath of Rasputin's \"Solar splash damage final blows have a chance to spawn Warmind Cells\".",
        operation: ">",
        threshold: 0.0,
    },
    Variable {
        name: "super_active",
        plain: "Super Active",
        evidence: "Compared by Vorpal Weapon (\"Guardians with their Super active\") on = 1, and by Ionic Return on > 0.1.",
        operation: "=",
        threshold: 1.0,
    },
    // Ionic Return compares each of these beside the plain state it mirrors, so each is
    // that state again with a recency the traced code does not pin down.
    Variable {
        name: "super_active_recent",
        plain: "Super Recently Active",
        evidence: "The engine name super_active_recent, compared by Ionic Return on > 0.1 beside its super_active check. How recent is not established.",
        operation: ">",
        threshold: 0.1,
    },
    Variable {
        name: "is_arc_recent",
        plain: "Subclass Recently Arc",
        evidence: "The engine name is_arc_recent, compared by Ionic Return on > 0.1 beside its is_arc check. How recent is not established.",
        operation: ">",
        threshold: 0.1,
    },
    Variable {
        name: "is_thermal_recent",
        plain: "Subclass Recently Solar",
        evidence: "The engine name is_thermal_recent, compared by Ionic Return on > 0.1 beside its is_thermal check. Thermal is the engine's word for Solar.",
        operation: ">",
        threshold: 0.1,
    },
    Variable {
        name: "is_void_recent",
        plain: "Subclass Recently Void",
        evidence: "The engine name is_void_recent, compared by Ionic Return on > 0.1 beside its is_void check. How recent is not established.",
        operation: ">",
        threshold: 0.1,
    },
    Variable {
        name: "siphon_gun_overload_no_cage",
        plain: "Weakening Field Not Formed",
        evidence: "The engine name siphon_gun_overload_no_cage, compared only by Judgment (\"envelops the target in a field that weakens and disrupts them. Strong against Overload Champions\"), on >= 1 for one path and < 0.5 for the other. Cage is the engine's word for that field.",
        operation: ">=",
        threshold: 1.0,
    },
    Variable {
        name: "support_nearby_enemy",
        plain: "Nearby Enemy Count Supported",
        evidence: "The engine name support_nearby_enemy, compared by Surrounded on >= 1 and on < 1 for its other path, beside its nearby_enemy_count check. It selects between two paths rather than counting enemies.",
        operation: ">=",
        threshold: 1.0,
    },
    // Bloom is an engine codename. The one perk comparing these carries no description, so
    // the rows keep the engine's own words rather than guessing at a catalyst.
    Variable {
        name: "bloom_catalyst_active",
        plain: "Bloom Catalyst Active",
        evidence: "The engine name bloom_catalyst_active, compared on = 0 by one stock perk that carries no description. Which catalyst bloom names is not established.",
        operation: "=",
        threshold: 0.0,
    },
    Variable {
        name: "bloom_catalyst_on_cooldown",
        plain: "Bloom Catalyst on Cooldown",
        evidence: "The engine name bloom_catalyst_on_cooldown, compared on = 0 by the same undescribed stock perk, beside its bloom_catalyst_active check.",
        operation: "=",
        threshold: 0.0,
    },
    Variable {
        name: "warmind_cells_increase_bauble_chance",
        plain: "Warmind Cells Increase Cell Chance",
        evidence: "The engine name warmind_cells_increase_bauble_chance, compared on > 0 by the Seraph weapon perks. It is Blessing of Rasputin word for word: \"collecting a Warmind Cell increases the chances that your next final blow with a Seraph weapon will create a Warmind Cell\".",
        operation: ">",
        threshold: 0.0,
    },
];

/// The comparison operations the stock predicates compile, with the byte each stores.
pub const OPERATIONS: [(&str, u8); 4] = [("=", 0), (">=", 3), ("<", 4), (">", 5)];

/// The stock variable with this engine name.
#[must_use]
pub fn variable(name: &str) -> Option<&'static Variable> {
    VARIABLES.iter().find(|variable| variable.name == name)
}

/// The FNV-1 key the binding stores for a variable name, exactly as `read` checks it.
#[must_use]
pub fn binding_key(variable: &str) -> u32 {
    variable.bytes().fold(0x811C9DC5u32, |hash, byte| {
        hash.wrapping_mul(0x01000193) ^ u32::from(byte)
    })
}

/// A fresh general predicate (kind 20) comparing one engine variable, built from the
/// stock Surrounded node by rewriting the four things that differ between stock
/// comparisons: the source string, the binding key, the operation and the threshold.
pub fn compose(variable: &str, operation: &str, threshold: f32) -> Result<Vec<u8>, String> {
    let mut graph = Graph::read(include_bytes!("predicate/nearby_enemy.bin"), 0, 0x80803DCE)?;
    let index = graph
        .blocks
        .iter()
        .position(|block| block.class == COMPARISON_CLASS)
        .ok_or("The comparison template has no comparison.")?;
    rewrite(&mut graph, index, variable, operation, threshold)?;
    graph.emit()
}

/// Rewrite one compiled comparison in place. Every other block keeps its bytes.
pub fn rewrite(
    graph: &mut Graph,
    index: usize,
    variable: &str,
    operation: &str,
    threshold: f32,
) -> Result<(), String> {
    if variable.is_empty()
        || !variable
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(
            "A compared variable is one engine name of letters, digits and underscores.".into(),
        );
    }
    if !threshold.is_finite() {
        return Err("Enter a finite threshold.".into());
    }
    let code = OPERATIONS
        .iter()
        .find(|(name, _)| *name == operation)
        .map(|(_, code)| *code)
        .ok_or("The comparison operation must be =, >=, < or >.")?;
    let mut changed = graph.clone();
    let block = changed
        .blocks
        .get(index)
        .filter(|block| block.class == COMPARISON_CLASS)
        .ok_or("The selected block is not a compiled comparison.")?;
    let source = *block
        .links
        .get(&0)
        .ok_or("The comparison has no source text.")?;
    let binding = *block
        .links
        .get(&16)
        .ok_or("The comparison has no binding.")?;
    let constant = *block
        .links
        .get(&112)
        .ok_or("The comparison has no threshold.")?;
    let mut text = format!("{variable} {operation} {threshold}").into_bytes();
    text.push(0);
    changed.blocks[source].bytes = text;
    changed.blocks[binding]
        .bytes
        .get_mut(4..8)
        .ok_or("The binding is truncated.")?
        .copy_from_slice(&binding_key(variable).to_le_bytes());
    *changed.blocks[index]
        .bytes
        .get_mut(136)
        .ok_or("The comparison is truncated.")? = code;
    changed.blocks[constant]
        .bytes
        .get_mut(..4)
        .ok_or("The threshold constant is truncated.")?
        .copy_from_slice(&threshold.to_le_bytes());
    changed.validate()?;
    if read(&changed, index).is_none() {
        return Err("The rewritten comparison does not read back.".into());
    }
    *graph = changed;
    Ok(())
}

pub fn comparisons(graph: &Graph, root: usize) -> Vec<Comparison> {
    let mut pending = vec![root];
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    while let Some(index) = pending.pop() {
        if !seen.insert(index) {
            continue;
        }
        let Some(block) = graph.blocks.get(index) else {
            continue;
        };
        if let Some(comparison) = read(graph, index) {
            result.push(comparison);
        }
        pending.extend(block.links.values().copied());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_reads_the_compiled_threshold_instead_of_stale_source_text() {
        let source = include_bytes!("predicate/nearby_enemy.bin");
        let mut graph = Graph::read(source, 0, 0x80803DCE).unwrap();
        assert_eq!(graph.emit().unwrap(), source);
        assert_eq!(describe(&graph).as_deref(), Some("Nearby Enemy Count > 2"));
        let comparison = comparisons(&graph, 0).remove(0);
        graph.blocks[comparison.constant_block].bytes[..4].copy_from_slice(&4.0f32.to_le_bytes());
        assert_eq!(describe(&graph).as_deref(), Some("Nearby Enemy Count > 4"));
        let reloaded = Graph::read(&graph.emit().unwrap(), 0, 0x80803DCE).unwrap();
        assert_eq!(describe(&reloaded), describe(&graph));

        let binding = graph
            .blocks
            .iter_mut()
            .find(|block| block.class == 0x8080941B)
            .unwrap();
        binding.bytes[4] ^= 1;
        assert!(
            describe(&graph).is_none(),
            "A source string cannot name a different runtime binding"
        );
    }

    #[test]
    fn every_stock_variable_composes_a_predicate_that_reads_back_and_reloads() {
        let template = include_bytes!("predicate/nearby_enemy.bin");
        let mut names = std::collections::BTreeSet::new();
        for variable in VARIABLES {
            assert!(
                names.insert(variable.name),
                "{} listed twice",
                variable.name
            );
            assert!(!variable.plain.is_empty() && !variable.evidence.is_empty());
            let bytes = compose(variable.name, variable.operation, variable.threshold).unwrap();
            let graph = Graph::read(&bytes, 0, 0x80803DCE).unwrap();
            graph.validate_node(NativeNodeKind::Condition(20)).unwrap();
            assert_eq!(
                describe(&graph).unwrap(),
                format!(
                    "{} {} {}",
                    variable.plain, variable.operation, variable.threshold
                )
            );
            let comparison = comparisons(&graph, 0).remove(0);
            assert_eq!(comparison.operation, variable.operation);
            assert_eq!(f32::from_bits(comparison.threshold), variable.threshold);
            let reloaded = Graph::read(&graph.emit().unwrap(), 0, 0x80803DCE).unwrap();
            assert_eq!(reloaded, graph);
            // The template's own comparison composes back to the template byte for byte.
            if variable.name == "nearby_enemy_count" {
                assert_eq!(bytes, template);
            }
        }
    }

    #[test]
    fn composing_preserves_native_bindings_and_refuses_bad_input() {
        // The binding key is the FNV-1 of the raw variable, as the stock nodes store it.
        assert_eq!(binding_key("nearby_enemy_count"), 0x58A9_CB99);
        assert_eq!(binding_key("nearby_ally_count"), 0xB762_7DF5);
        let a = Graph::read(&compose("is_arc", "=", 1.0).unwrap(), 0, 0x80803DCE).unwrap();
        let b = Graph::read(&compose("is_void", ">", 0.5).unwrap(), 0, 0x80803DCE).unwrap();
        assert_eq!(describe(&a).as_deref(), Some("Subclass Is Arc = 1"));
        assert_eq!(describe(&b).as_deref(), Some("Subclass Is Void > 0.5"));
        assert!(compose("bad name", "=", 1.0).is_err());
        assert!(compose("is_arc", "!=", 1.0).is_err());
        assert!(compose("is_arc", "=", f32::NAN).is_err());
    }
}
