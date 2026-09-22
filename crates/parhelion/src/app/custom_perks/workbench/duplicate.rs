//! Copy one effect after recovering its effective behavior, never by substituting a donor.
use super::*;
use sundial::package_authoring::sandbox_perk::program::Program;

pub(super) struct Pending {
    document: String,
    source: WeaponSandboxPerkRuntimeRecipe,
    worker: thread::JoinHandle<Result<Program, String>>,
}

fn insert(
    recipe: &mut PerkRecipe,
    source: &WeaponSandboxPerkRuntimeRecipe,
    mut program: Program,
    index: u16,
) -> Result<(), String> {
    let position = recipe
        .effects
        .iter()
        .position(|effect| effect == source)
        .ok_or(
            "The source effect changed while its copy was being prepared. Duplicate it again.",
        )?;
    if recipe
        .effects
        .iter()
        .any(|effect| effect.source_perk_index == index)
    {
        return Err("There is no available identity for this effect.".into());
    }
    program.name = format!("{} Copy", program.name);
    let mut effect = PerkRecipe::effect(index);
    effect.program = Some(program);
    recipe.effects.insert(position + 1, effect);
    Ok(())
}

impl Workbench {
    pub(super) fn copying_selected_effect(&self) -> bool {
        self.duplicating.as_ref().is_some_and(|pending| {
            self.documents
                .get(self.selected)
                .is_some_and(|document| document.recipe.id == pending.document)
        })
    }

    pub(super) fn duplicate_effect(
        &mut self,
        packages: &Path,
        ctx: &egui::Context,
        recipe: &mut PerkRecipe,
        choices: &[WeaponSandboxPerkChoice],
        index: u16,
    ) {
        if self.duplicating.is_some() {
            return;
        }
        let Some(source) = recipe
            .effects
            .iter()
            .find(|effect| effect.source_perk_index == index)
            .cloned()
        else {
            return;
        };
        if let Some(program) = source.program.clone() {
            let result = self
                .free_metadata_index(recipe, choices)
                .ok_or_else(|| "There is no available identity for another effect.".to_owned())
                .and_then(|index| insert(recipe, &source, program, index));
            if let Err(error) = result {
                self.error = Some(error);
            }
            return;
        }
        let name = self.stock_effect_name(choices, index);
        let packages = packages.to_owned();
        let snapshot = source.clone();
        let repaint = ctx.clone();
        self.duplicating = Some(Pending {
            document: recipe.id.clone(),
            source,
            worker: thread::spawn(move || {
                let result = editor::conversion::copy_effect(&packages, &snapshot, name);
                repaint.request_repaint();
                result
            }),
        });
    }

    pub(super) fn poll_duplicate(
        &mut self,
        ctx: &egui::Context,
        choices: &[WeaponSandboxPerkChoice],
    ) {
        if !self
            .duplicating
            .as_ref()
            .is_some_and(|pending| pending.worker.is_finished())
        {
            if self.duplicating.is_some() {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            return;
        }
        let pending = self.duplicating.take().unwrap();
        let result = pending
            .worker
            .join()
            .unwrap_or_else(|_| Err("The effect copy could not be prepared.".into()));
        let Some(position) = self
            .documents
            .iter()
            .position(|doc| doc.recipe.id == pending.document)
        else {
            return;
        };
        let index = self.free_metadata_index(&self.documents[position].recipe, choices);
        let document = &mut self.documents[position];
        let before = document.recipe.clone();
        let result = result.and_then(|program| {
            if document.pending_effect.is_some()
                || (position == self.selected && self.editor.is_some())
            {
                return Err("Finish editing the source effect, then duplicate it again.".into());
            }
            let index = index.ok_or("There is no available identity for another effect.")?;
            insert(&mut document.recipe, &pending.source, program, index)
        });
        match result {
            Ok(()) => {
                document.history.record_step(before);
                document.modified = Some(SystemTime::now());
                self.persist_drafts();
            }
            Err(error) => self.error = Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::sandbox_perk::program::{Action, NativeNode};

    #[test]
    fn pending_copy_blocks_committing_its_document_but_not_other_documents() {
        let recipe = PerkRecipe::new();
        let id = recipe.id.clone();
        let mut workbench = Workbench {
            documents: vec![
                Document::new(recipe, None),
                Document::new(PerkRecipe::new(), None),
            ],
            ..Default::default()
        };
        let (send, receive) = std::sync::mpsc::channel();
        workbench.duplicating = Some(Pending {
            document: id,
            source: PerkRecipe::effect(421),
            worker: thread::spawn(move || {
                receive.recv().unwrap();
                Ok(Program::default())
            }),
        });
        assert_eq!(
            workbench.edit_issue(),
            Some("Wait for the effect copy to finish.")
        );
        assert_eq!(workbench.save_issue(), workbench.edit_issue());
        workbench.selected = 1;
        assert_eq!(workbench.edit_issue(), None);
        send.send(()).unwrap();
        workbench
            .duplicating
            .take()
            .unwrap()
            .worker
            .join()
            .unwrap()
            .unwrap();
    }

    #[test]
    fn duplicate_keeps_native_bytes_uses_a_distinct_identity_and_checks_the_source() {
        let mut recipe = PerkRecipe::new();
        let mut source = program::new_effect(1178);
        let mut node = NativeNode::effect(47).unwrap();
        node.bytes[4..8].copy_from_slice(&0xFEDCBA98_u32.to_le_bytes());
        source
            .program
            .as_mut()
            .unwrap()
            .actions
            .push(Action::Native { node });
        recipe.effects.push(source.clone());
        let original = recipe.clone();
        insert(&mut recipe, &source, source.program.clone().unwrap(), 421).unwrap();
        assert_eq!(recipe.effects[0], source);
        let copy = recipe.effects[1].program.as_ref().unwrap().clone();
        assert_eq!(copy.actions, source.program.as_ref().unwrap().actions);
        assert_eq!(recipe.effects[1].source_perk_index, 421);
        let unchanged = recipe.clone();
        assert!(insert(&mut recipe, &source, copy.clone(), 421).is_err());
        assert_eq!(recipe, unchanged);
        let mut changed = original;
        changed.effects[0].program.as_mut().unwrap().duration_ms += 1;
        let unchanged = changed.clone();
        assert!(insert(&mut changed, &source, source.program.clone().unwrap(), 421).is_err());
        assert_eq!(changed, unchanged);
    }
}
