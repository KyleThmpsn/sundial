use super::*;

pub(super) struct AuthoredTemplate {
    pub(super) weapon: String,
    pub(super) variant: WeaponSocketPlugVariantRecipe,
}

impl Workbench {
    pub(super) fn load_authored_templates(
        &mut self,
        library: Option<&RecipeLibrary>,
        draft: &WeaponRecipe,
        catalog: &InvestmentCatalog,
    ) {
        self.initialize();
        let (mut templates, mut warnings) = load_saved(library);
        if let Some(perks) = &self.library {
            let recipes = templates.iter().filter_map(|entry| {
                let hash = entry.variant.source_plug_hash.parse_u32().unwrap_or_default();
                if !entry.variant.replace_effects && catalog.item_definition_tag(hash).is_none() {
                    warnings.push(format!("Could not import a custom perk from {} because its stock template {hash:08X} is unavailable. The weapon recipe is unchanged.", entry.weapon));
                    None
                } else {
                    Some(from_variant(&entry.variant, catalog))
                }
            }).collect::<Vec<_>>();
            match perks.import_embedded(recipes) {
                Ok(report) => {
                    warnings.extend(report.errors);
                    if report.added > 0 {
                        self.message = Some(format!(
                            "Added {} custom perks from saved weapon recipes to the library.",
                            report.added
                        ));
                    }
                }
                Err(error) => warnings.push(error),
            }
            warnings.extend(self.scan_library());
        }
        append_templates(&mut templates, draft.clone());
        self.authored_templates = Some(templates);
        if !warnings.is_empty() {
            self.error = Some(warnings.join("\n"));
        }
    }

    pub(super) fn draw_templates(&mut self, ui: &mut egui::Ui, catalog: &InvestmentCatalog) {
        let stock = self.templates.get_or_insert_with(|| {
            catalog.perk_template_choices_from(crate::package_profile::is_stock_item_definition)
        });
        let authored = self.authored_templates.as_deref().unwrap_or_default();
        let picked = pickers::popup(
            ui,
            "perk-template",
            "Copy Existing…",
            &mut self.template_query,
            |ui, query, reset, height| {
                let mut rows: Vec<(bool, usize, u32, String, String)> = Vec::new();
                for (index, entry) in authored.iter().enumerate() {
                    let hash = entry
                        .variant
                        .source_plug_hash
                        .parse_u32()
                        .unwrap_or_default();
                    let name = entry
                        .variant
                        .name
                        .clone()
                        .unwrap_or_else(|| catalog.plug_label(hash, false));
                    let detail = format!(
                        "{} · {}",
                        entry.weapon,
                        entry.variant.description.as_deref().unwrap_or_default()
                    );
                    if pickers::matches(query, &format!("{name} {detail}")) {
                        rows.push((true, index, hash, name, detail));
                    }
                }
                for (index, choice) in stock.iter().enumerate() {
                    let hash = choice.representative_hash;
                    let detail = format!(
                        "{} · {}",
                        choice.representative_type_name,
                        catalog.perk_description(hash).unwrap_or_default()
                    );
                    if pickers::matches(
                        query,
                        &format!("{} {detail} {hash:08X}", choice.representative_name),
                    ) {
                        rows.push((
                            false,
                            index,
                            hash,
                            choice.representative_name.clone(),
                            detail,
                        ));
                    }
                }
                rows.sort_by_cached_key(|row| (!row.0, row.3.to_lowercase(), row.2));
                pickers::results(
                    ui,
                    "perk-template-results",
                    rows.len(),
                    height,
                    reset,
                    sundial::investment::authoring_choice_row_height(ui),
                    |ui, index| {
                        let row = &rows[index];
                        catalog
                            .draw_authoring_choice_row(ui, Some(row.2), &row.3, Some(&row.4), false)
                            .clicked()
                            .then_some((row.0, row.1))
                    },
                )
            },
        );
        let Some((is_authored, index)) = picked else {
            return;
        };
        let recipe = if is_authored {
            from_variant(&authored[index].variant, catalog)
        } else {
            let choice = &stock[index];
            let mut recipe = PerkRecipe::new();
            recipe.name = format!("Custom {}", choice.representative_name);
            recipe.template_plug = choice.representative_hash.into();
            recipe.description = catalog
                .perk_description(choice.representative_hash)
                .unwrap_or_default()
                .to_owned();
            recipe.effects = catalog
                .item_sandbox_perk_indices(choice.representative_hash)
                .into_iter()
                .map(PerkRecipe::effect)
                .collect();
            recipe.stats = catalog
                .item_stat_contributions(choice.representative_hash)
                .into_iter()
                .map(|stat| WeaponStatOverride {
                    definition_index: stat.definition_index,
                    value: stat.value,
                })
                .collect();
            recipe
        };
        self.add_document(Document::new(recipe, None));
        self.page = Page::Effects;
    }
}

pub(super) fn from_variant(
    variant: &WeaponSocketPlugVariantRecipe,
    catalog: &InvestmentCatalog,
) -> PerkRecipe {
    let mut recipe = PerkRecipe::new();
    let hash = variant.source_plug_hash.parse_u32().unwrap_or_default();
    recipe.template_plug = variant.source_plug_hash.clone();
    recipe.icon = variant.icon.clone();
    recipe.name = variant
        .name
        .clone()
        .unwrap_or_else(|| catalog.plug_label(hash, false));
    recipe.description = variant.description.clone().unwrap_or_else(|| {
        catalog
            .perk_description(hash)
            .unwrap_or_default()
            .to_owned()
    });
    recipe.classification = variant.classification_donor_hash.clone();
    if !variant.replace_effects {
        recipe.effects = catalog
            .item_sandbox_perk_indices(hash)
            .into_iter()
            .map(PerkRecipe::effect)
            .collect();
        recipe.stats = catalog
            .item_stat_contributions(hash)
            .into_iter()
            .map(|stat| WeaponStatOverride {
                definition_index: stat.definition_index,
                value: stat.value,
            })
            .collect();
    }
    for &index in &variant.additional_sandbox_perks {
        if !recipe
            .effects
            .iter()
            .any(|effect| effect.source_perk_index == index)
        {
            recipe.effects.push(PerkRecipe::effect(index));
        }
    }
    for effect in &variant.sandbox_perks {
        if let Some(existing) = recipe
            .effects
            .iter_mut()
            .find(|existing| existing.source_perk_index == effect.source_perk_index)
        {
            *existing = effect.clone();
        } else {
            recipe.effects.push(effect.clone());
        }
    }
    for stat in &variant.investment_stats {
        if let Some(existing) = recipe
            .stats
            .iter_mut()
            .find(|existing| existing.definition_index == stat.definition_index)
        {
            *existing = stat.clone();
        } else {
            recipe.stats.push(stat.clone());
        }
    }
    recipe
}

/// Source loading returns its own diagnostics so callers can display them in context.
pub(super) fn load(
    library: Option<&RecipeLibrary>,
    draft: &WeaponRecipe,
) -> (Vec<AuthoredTemplate>, Vec<String>) {
    let (mut templates, warnings) = load_saved(library);
    append_templates(&mut templates, draft.clone());
    (templates, warnings)
}

fn load_saved(library: Option<&RecipeLibrary>) -> (Vec<AuthoredTemplate>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut recipes = Vec::new();
    if let Some(library) = library {
        match library.scan() {
            Ok(scan) => {
                warnings.extend(scan.errors);
                for entry in scan.entries {
                    match WeaponRecipe::load_json(&entry.path) {
                        Ok(recipe) => recipes.push(recipe),
                        Err(error) => warnings.push(error.to_string()),
                    }
                }
            }
            Err(error) => warnings.push(error),
        }
    }
    let mut templates: Vec<AuthoredTemplate> = Vec::new();
    for recipe in recipes {
        append_templates(&mut templates, recipe);
    }
    (templates, warnings)
}

fn append_templates(templates: &mut Vec<AuthoredTemplate>, recipe: WeaponRecipe) {
    for mut variant in recipe.overrides.socket_plug_variants {
        variant.socket_index = 0;
        variant.choice_index = 0;
        if !templates.iter().any(|template| template.variant == variant) {
            templates.push(AuthoredTemplate {
                weapon: recipe.name.clone(),
                variant,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_LIBRARY_ROOT"]
    fn saved_weapon_perks_migrate_to_files_without_changing_weapons_or_importing_drafts() {
        let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let root = PathBuf::from(std::env::var_os("PARHELION_LIBRARY_ROOT").unwrap());
        let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let weapons = RecipeLibrary::open(temp.path().join("recipes")).unwrap();
        let mut originals = Vec::new();
        for entry in std::fs::read_dir(root.join("recipes")).unwrap() {
            let path = entry.unwrap().path();
            if path.to_string_lossy().ends_with(".parhelion.json") {
                let bytes = std::fs::read(&path).unwrap();
                let copied = weapons.root().join(path.file_name().unwrap());
                std::fs::write(&copied, &bytes).unwrap();
                originals.push((copied, bytes));
            }
        }
        let perks = Library::open(temp.path().join("perks")).unwrap();
        // Seed existing standalone files to exercise duplicate detection across formats.
        for entry in std::fs::read_dir(root.join("perks")).unwrap() {
            let path = entry.unwrap().path();
            if path.to_string_lossy().ends_with(".perk.json") {
                Library::read(&path).unwrap();
                std::fs::copy(&path, perks.root().join(path.file_name().unwrap())).unwrap();
            }
        }
        let initial = perks.scan().unwrap().entries.len();
        let mut workbench = Workbench {
            initialized: true,
            library: Some(perks),
            ..Default::default()
        };
        let mut draft = WeaponRecipe::every_end();
        let mut unsaved = PerkRecipe::new();
        unsaved.name = "Unsaved Draft Must Not Be Imported".into();
        draft.overrides.socket_plug_variants = vec![unsaved.at_socket(0, 0)];
        workbench.load_authored_templates(Some(&weapons), &draft, &catalog);
        assert!(workbench.error.is_none(), "{:?}", workbench.error);
        assert!(
            !workbench
                .entries
                .iter()
                .any(|entry| entry.recipe.name == unsaved.name)
        );
        let count = workbench.entries.len();
        assert!(count >= initial);
        workbench.load_authored_templates(Some(&weapons), &draft, &catalog);
        assert!(workbench.error.is_none(), "{:?}", workbench.error);
        assert_eq!(workbench.entries.len(), count);
        for (path, bytes) in originals {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        println!(
            "{initial} existing standalone perks, {} new embedded perks, {count} total. Refresh is idempotent and saved weapons are unchanged.",
            count - initial
        );
    }
}
