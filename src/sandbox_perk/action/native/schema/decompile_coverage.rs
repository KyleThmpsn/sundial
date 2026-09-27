use crate::sandbox_perk::action::decode;
use crate::sandbox_perk::program::decompile::decompile;

#[test]
#[ignore = "requires PARHELION_PERK_SURVEY with captured stock actions"]
fn captured_stock_actions_open_without_decode_or_conversion_failures() {
    let root = std::env::var_os("PARHELION_PERK_SURVEY")
        .expect("PARHELION_PERK_SURVEY must name the captured survey directory");
    let actions = std::path::Path::new(&root).join("actions");
    let mut checked = 0;
    for entry in std::fs::read_dir(actions).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "bin") {
            continue;
        }
        let tag = u32::from_str_radix(path.file_stem().unwrap().to_str().unwrap(), 16)
            .expect("captured action filename must be its hexadecimal tag");
        // This range contains synthetic fixtures, not captured stock actions.
        if (0x80B7_7947..=0x80B7_79C5).contains(&tag) && (tag - 0x80B7_7947) % 7 == 0 {
            continue;
        }
        let data = std::fs::read(&path).unwrap();
        let decoded = decode(&data).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        decompile(&decoded, &format!("0x{tag:08X}"), |tag| tag)
            .unwrap_or_else(|error| panic!("{}: {}", path.display(), error.0));
        checked += 1;
    }
    assert!(checked > 0, "no captured stock actions were checked");
}
