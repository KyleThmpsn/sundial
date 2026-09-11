use super::*;
use crate::app::custom_perks::editor::tests::set_test_speed;

#[test]
fn switching_documents_preserves_pending_edits_under_their_original_owner() {
    for existing in [false, true] {
        let mut parameter_editor = editor(fixture());
        let graph = parameter_editor.graph.clone().unwrap();
        set_test_speed(&graph, &mut parameter_editor.draft, 1.5);
        let expected = parameter_editor.draft.clone();
        let mut workbench = Workbench::default();
        workbench.set_test_editor(parameter_editor);
        let original = workbench.documents[0].recipe.clone();
        workbench.open = false;
        let mut second = PerkRecipe::new();
        second.name = "Another Perk".into();
        second.effects.push(PerkRecipe::effect(1178));
        if existing {
            workbench
                .documents
                .push(Document::new(second.clone(), None));
            workbench.select_document(1);
        } else {
            workbench.add_document(Document::new(second.clone(), None));
        }
        assert_eq!(workbench.selected, 1);
        assert!(workbench.editor.is_none());
        assert!(workbench.editing_effect.is_none());
        assert!(workbench.editing_program_action.is_none());
        assert_eq!(workbench.documents[0].recipe, original);
        assert_eq!(
            workbench.documents[0]
                .pending_effect
                .as_ref()
                .unwrap()
                .values,
            expected
        );
        assert_eq!(workbench.documents[1].recipe, second);
        assert!(workbench.documents[1].pending_effect.is_none());
        workbench.select_document(0);
        assert_eq!(
            workbench.documents[0]
                .pending_effect
                .as_ref()
                .unwrap()
                .values,
            expected
        );
    }
}

#[test]
fn reselecting_the_current_document_keeps_its_active_editor() {
    let mut workbench = Workbench::default();
    workbench.set_test_editor(editor(fixture()));
    let recipe = workbench.documents[0].recipe.clone();
    workbench.add_document(Document::new(recipe, None));
    assert_eq!(workbench.documents.len(), 1);
    assert!(workbench.editor.is_some());
    assert_eq!(workbench.editing_effect, Some(1178));
}

#[test]
fn switching_documents_keeps_an_unfinished_worker_owned_until_it_finishes() {
    let mut parameter_editor = editor(fixture());
    let (sender, receiver) = mpsc::channel();
    parameter_editor.receiver = Some(receiver);
    let mut workbench = Workbench::default();
    workbench.set_test_editor(parameter_editor);
    workbench.editing_program_action = Some(0);
    workbench.add_document(Document::new(PerkRecipe::new(), None));
    assert!(workbench.editor.is_none());
    assert!(workbench.editing_program_action.is_none());
    assert_eq!(
        workbench.documents[0]
            .pending_effect
            .as_ref()
            .unwrap()
            .action,
        Some(0)
    );
    assert_eq!(workbench.retired_editors.len(), 1);
    assert!(workbench.busy());
    drop(sender);
    workbench.open = false;
    workbench.show(
        &egui::Context::default(),
        Path::new(""),
        None,
        &[],
        false,
        (&WeaponRecipe::every_end(), None),
    );
    assert!(workbench.retired_editors.is_empty());
    assert!(!workbench.busy());
}
