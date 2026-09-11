use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RestoreRecipe {
    pub path: PathBuf,
    pub name: String,
    root: PathBuf,
    original: Vec<u8>,
    template: &'static str,
}

impl RecipeLibrary {
    pub(crate) fn prepare_restore_recipe(&self, path: &Path) -> Result<RestoreRecipe, String> {
        let target = self.confined_existing_path(path)?;
        let (_, template) = BUNDLED_RECIPES
            .iter()
            .find(|(name, _)| target.file_name().is_some_and(|file| file == *name))
            .ok_or("This recipe has no bundled default")?;
        let recipe = WeaponRecipe::from_json_str(template).map_err(|error| error.to_string())?;
        self.ensure_unique_identity(&recipe, Some(&target))?;
        Ok(RestoreRecipe {
            path: path.to_path_buf(),
            name: recipe.name,
            root: self.canonical_root.clone(),
            original: fs::read(&target).map_err(|error| error.to_string())?,
            template,
        })
    }

    pub(crate) fn restore_recipe(
        &self,
        preview: &RestoreRecipe,
    ) -> Result<Option<PathBuf>, String> {
        if &self.prepare_restore_recipe(&preview.path)? != preview {
            return Err("This recipe changed after the preview. Review the restore again.".into());
        }
        if preview.original == preview.template.as_bytes() {
            return Ok(None);
        }
        let backup = self.create_restore_backup()?;
        let name = preview
            .path
            .file_name()
            .ok_or("Recipe filename is missing")?;
        atomic_write_create_new(&backup.join(name), &preview.original).map_err(
            |error| match error {
                WriteNewError::AlreadyExists => "Recipe backup already exists".into(),
                WriteNewError::Other(error) => error,
            },
        )?;
        self.prepare_restore_target(&preview.path, &Some(preview.original.clone()))?;
        atomic_write_replace(&preview.path, preview.template.as_bytes())?;
        Ok(Some(backup))
    }
}
