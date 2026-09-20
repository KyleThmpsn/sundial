//! Callable object-behavior scripts, resolved from installed native path/tag pairs.
use super::tft;
use crate::sandbox_perk::action::native::fields::scripts;
use std::collections::BTreeMap;

const SCRIPT_CLASS: u32 = 0x8080_941E;

#[derive(Clone, Debug)]
pub struct Choice {
    pub tag: u32,
    pub path: String,
    pub title: String,
    pub stock_perks: String,
}

/// A path alone is not callable. Keep only live references to the script resource class,
/// across all packages rather than only the scripts reached by stock perk actions.
pub fn choices(index: &tft::Index) -> Vec<Choice> {
    let mut found = BTreeMap::new();
    for reference in &index.references {
        if reference.target_class != SCRIPT_CLASS
            || !reference.path.ends_with(".object_behaviors.tft")
        {
            continue;
        }
        found.entry(reference.target).or_insert_with(|| Choice {
            tag: reference.target,
            path: reference.path.clone(),
            title: scripts::title(&reference.path),
            stock_perks: scripts::by_tag(reference.target)
                .filter(|stock| stock.path == reference.path)
                .map_or_else(String::new, |stock| stock.perks.to_owned()),
        });
    }
    let mut choices = found.into_values().collect::<Vec<_>>();
    choices.sort_by_cached_key(|choice| (choice.title.to_lowercase(), choice.tag));
    choices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn installed_scripts_extend_the_stock_perk_examples() {
        let path =
            std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let manager = super::super::open_packages(&path).unwrap();
        let index = tft::cached(&path, &manager, |_, _| {}).unwrap();
        let found = choices(&index);
        assert!(found.len() > scripts::SCRIPTS.len());
        println!(
            "{} installed object-behavior scripts, compared with {} stock-perk examples",
            found.len(),
            scripts::SCRIPTS.len()
        );
        for entry in found
            .iter()
            .filter(|entry| scripts::by_tag(entry.tag).is_none())
            .take(12)
        {
            println!("{:08X}: {}", entry.tag, entry.path);
        }
    }

    #[test]
    fn package_wide_scripts_include_non_perk_sources_and_require_valid_targets() {
        let sample = |tag, class, path: &str| tft::Reference {
            source: 1,
            source_class: 2,
            offset: 16,
            target: tag,
            target_class: class,
            path: path.into(),
        };
        let mut index = tft::Index {
            references: vec![
                sample(
                    5,
                    SCRIPT_CLASS,
                    r"content\other\new_script.object_behaviors.tft",
                ),
                sample(
                    5,
                    SCRIPT_CLASS,
                    r"content\other\new_script.object_behaviors.tft",
                ),
                sample(6, 123, r"content\other\wrong_class.object_behaviors.tft"),
                sample(7, SCRIPT_CLASS, r"content\other\not_a_script.tft"),
            ],
            ..Default::default()
        };
        index.paths.push(tft::ContentPath {
            source: 8,
            offset: 0,
            path: r"content\orphan.object_behaviors.tft".into(),
        });
        let found = choices(&index);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tag, 5);
        assert_eq!(found[0].title, "New Script");
        assert!(found[0].stock_perks.is_empty());
    }
}
