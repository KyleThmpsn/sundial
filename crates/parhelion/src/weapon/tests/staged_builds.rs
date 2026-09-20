use super::*;

mod activation;
mod ammo;
mod collection_conditions;
mod companions;
mod donors;
mod hud;
mod ornaments;
mod personalization;
mod presentation;
mod programs;
mod projectiles;
mod projects;
mod socket_expansion;

/// Stages a built bundle beside hard links of the clean stock packages and returns that view.
///
/// Reading a tag back through a real package manager is the only proof that an authored payload
/// survives packing, so every staged check shares this temporary install.
fn staged_view(
    packages: &Path,
    prefix: &str,
    bundle: &NewWeaponProjectBundle,
) -> tempfile::TempDir {
    let source_root = packages.parent().unwrap();
    let view = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(source_root)
        .unwrap();
    let staged = view.path().join("packages");
    fs::create_dir(&staged).unwrap();
    for entry in fs::read_dir(packages).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().and_then(|value| value.to_str()) == Some("pkg") {
            fs::hard_link(entry.path(), staged.join(entry.file_name())).unwrap();
        }
    }
    let bin = view.path().join("bin/x64");
    fs::create_dir_all(&bin).unwrap();
    fs::hard_link(
        source_root.join("bin/x64/oo2core_3_win64.dll"),
        bin.join("oo2core_3_win64.dll"),
    )
    .unwrap();
    assert_eq!(
        bundle.write_new(&staged).unwrap().len(),
        bundle.artifacts.len()
    );
    view
}
