use super::*;

mod activation;
mod ammo;
mod appearance_sweep;
mod bank_overlays;
mod behavior_firing;
mod collection_conditions;
mod companions;
mod component_rewire;
mod donors;
#[cfg(feature = "d2-model-importer")]
mod eager_private_dependencies;
mod effect_length;
mod glow;
mod hud;
mod native_objectives;
mod ornaments;
mod parts;
mod personalization;
mod presentation;
mod programs;
mod projectiles;
mod projects;
mod socket_expansion;

/// Whether a stock view must carry the Oodle runtime, or links it only when the install has one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Oodle {
    Required,
    IfPresent,
}

/// A temporary install on the stock packages' volume holding hard links of every clean stock
/// package, and of the Oodle runtime as `oodle` asks. Returns the view and its packages folder,
/// which takes a bundle create-new without writing through to the stock files.
fn stock_view(
    packages: &Path,
    prefix: &str,
    oodle: Oodle,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let source_root = packages
        .parent()
        .expect("configured package directory should have a parent");
    let view = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(source_root)
        .expect("temporary package view should be created on the package volume");
    let staged = view.path().join("packages");
    fs::create_dir(&staged).expect("temporary packages directory should be created");
    for entry in fs::read_dir(packages).expect("clean-stock packages should be listable") {
        let entry = entry.expect("clean-stock package entry should be readable");
        if entry.path().extension().and_then(|value| value.to_str()) == Some("pkg") {
            fs::hard_link(entry.path(), staged.join(entry.file_name()))
                .expect("clean-stock package should hard-link into the temporary view");
        }
    }
    let runtime = source_root.join("bin/x64/oo2core_3_win64.dll");
    if oodle == Oodle::Required || runtime.is_file() {
        let bin = view.path().join("bin/x64");
        fs::create_dir_all(&bin).expect("temporary Oodle directory should be created");
        fs::hard_link(&runtime, bin.join("oo2core_3_win64.dll"))
            .expect("Oodle runtime should hard-link into the temporary view");
    }
    (view, staged)
}

/// Stages a built bundle beside hard links of the clean stock packages and returns that view.
///
/// Reading a tag back through a real package manager is the only proof that an authored payload
/// survives packing, so every staged check shares this temporary install.
fn staged_view(
    packages: &Path,
    prefix: &str,
    bundle: &NewWeaponProjectBundle,
) -> tempfile::TempDir {
    let (view, staged) = stock_view(packages, prefix, Oodle::Required);
    assert_eq!(
        bundle.write_new(&staged).unwrap().len(),
        bundle.artifacts.len()
    );
    view
}
