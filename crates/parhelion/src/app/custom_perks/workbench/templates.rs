use super::*;

pub(super) struct AuthoredTemplate {
    weapon: String,
    variant: WeaponSocketPlugVariantRecipe,
}

impl Workbench {
    pub(super) fn load_authored_templates(
        &mut self,
        library: Option<&RecipeLibrary>,
        draft: &WeaponRecipe,
    ) {
        let mut recipes = vec![draft.clone()];
        if let Some(library) = library {
            match library.scan() {
                Ok(scan) => {
                    for entry in scan.entries {
                        match WeaponRecipe::load_json(&entry.path) {
                            Ok(recipe) => recipes.push(recipe),
                            Err(error) => self.error = Some(error.to_string()),
                        }
                    }
                }
                Err(error) => self.error = Some(error),
            }
        }
        let mut templates: Vec<AuthoredTemplate> = Vec::new();
        for recipe in recipes {
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
        self.authored_templates = Some(templates);
    }

    pub(super) fn draw_templates(&mut self, ui: &mut egui::Ui, catalog: &InvestmentCatalog) {
        let stock = self.templates.get_or_insert_with(|| {
            catalog.perk_template_choices_from(crate::package_profile::is_stock_item_definition)
        });
        let authored = self.authored_templates.as_deref().unwrap_or_default();
        let picked = pickers::popup(
            ui,
            "perk-template",
            "Copy Existing Perk…",
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
