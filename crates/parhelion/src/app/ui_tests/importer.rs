use super::*;

#[test]
fn appearance_model_picker_requires_the_importer_preference() {
    let mut app = PackageAuthoringApp::default();
    let before = app.recipe.clone();
    app.importer.enabled = false;
    let (output, _) = render(900.0, |ui| app.draw_appearance_workspace(ui));
    assert!(!text(&output).contains("Choose Donor Model"));
    app.importer.enabled = true;
    let (output, _) = render(900.0, |ui| app.draw_appearance_workspace(ui));
    assert!(text(&output).contains("Choose Donor Model"));
    assert_eq!(app.recipe, before);
}

#[test]
fn importer_window_is_gated_by_its_own_preference() {
    let mut app = PackageAuthoringApp::default();
    app.importer.open = true;
    app.importer.enabled = false;
    let (output, _) = render(900.0, |ui| app.draw_importer(ui.ctx()));
    assert!(!text(&output).contains("D2 Importer"));
    app.importer.enabled = true;
    let (output, _) = render(900.0, |ui| app.draw_importer(ui.ctx()));
    assert!(text(&output).contains("D2 Importer"));
    assert!(text(&output).contains("Refresh"));
}
