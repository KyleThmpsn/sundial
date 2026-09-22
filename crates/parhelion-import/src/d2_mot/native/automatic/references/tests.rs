use super::*;

#[test]
#[ignore = "Requires local source packages and a prepared model, never installs"]
fn source_conversion_uses_discovered_contracts() {
    let prepared = PathBuf::from(std::env::var_os("PARHELION_PREPARED").unwrap());
    let previous = prepared.parent().unwrap().join("assembled");
    let scratch = tempfile::tempdir().unwrap();
    let refs = scratch.path().join("references");
    let bindings = scratch.path().join("bindings");
    let out = scratch.path().join("rendered");
    let mut progress = |message| println!("{message}");
    export(&prepared, &refs, &mut progress).unwrap();
    let source = load(&prepared.join("source/source-manifest.json")).unwrap();
    let native = load(&prepared.join("native/source-manifest.json")).unwrap();
    let tool = super::super::decompiler::prepare(&mut progress).unwrap();
    crate::d2_mot::support::export_with_progress(
        &prepared,
        Path::new(source["packages"].as_str().unwrap()),
        Path::new(native["packages"].as_str().unwrap()),
        &refs,
        &tool,
        &bindings,
        &mut progress,
    )
    .unwrap();
    let result = super::super::super::effects::build_with_progress(
        &prepared,
        &previous,
        &refs,
        &bindings,
        &out,
        "source-all",
        &mut progress,
    )
    .unwrap();
    println!("{result}");
    let manifest = load(&out.join("graph/asset-graph.json")).unwrap();
    let nodes = manifest["nodes"].as_array().unwrap();
    let symbols: BTreeSet<_> = nodes
        .iter()
        .map(|n| n["symbol"].as_str().unwrap())
        .collect();
    assert_eq!(symbols.len(), nodes.len());
    for node in nodes {
        let bytes = fs::read(out.join("graph").join(node["file"].as_str().unwrap())).unwrap();
        if let Some(reference) = node["reference"].as_str() {
            assert!(symbols.contains(reference));
        }
        for patch in node["patches"].as_array().unwrap() {
            assert!(symbols.contains(patch["symbol"].as_str().unwrap()));
            assert!(patch["offset"].as_u64().unwrap() as usize + 4 <= bytes.len());
        }
    }
    assert_eq!(
        manifest["source_shader_adapter"]["source_shader_equations_retained"],
        true
    );
}
