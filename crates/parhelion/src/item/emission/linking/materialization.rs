//! Configured package artifact oracle, authored before the linker boundary.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};

fn digest(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn read_i64(data: &[u8], offset: usize) -> AuthoringResult<i64> {
    let bytes = data
        .get(offset..offset + 8)
        .ok_or_else(|| invalid("Fixture pointer is truncated"))?;
    Ok(i64::from_le_bytes(bytes.try_into().unwrap()))
}

#[derive(Deserialize)]
struct Fixture {
    packages: PathBuf,
    owner_template: u32,
    allocation_template: u32,
    buffer_header: u32,
    buffer_data: u32,
    parent_template: u32,
}

type PackageState = (
    u16,
    Vec<(u32, Vec<u8>)>,
    Vec<crate::NewTagReferenceOverride>,
);

fn state(assets: &crate::asset_packages::AssetPackages) -> Vec<PackageState> {
    assets
        .packages
        .iter()
        .map(|p| {
            (
                p.id,
                p.tags
                    .iter()
                    .map(|t| (t.template_tag.0, t.payload.clone()))
                    .collect(),
                p.references.clone(),
            )
        })
        .collect()
}

#[test]
#[ignore = "requires PARHELION_LINK_FIXTURE with Native owner, allocation, buffer and enrollment templates"]
fn private_materialization_package_readback() {
    let output = crate::test_support::artifact_dir("linking");
    assert!(!output.exists(), "linker output must be fresh");
    use crate::package_profile::{Host, Window};
    for (label, window, host) in [
        ("legacy", Window::FULL, Host::APPENDED),
        (
            "managed",
            Window {
                base: Window::FULL.base,
                top: Window::FULL.top - 1,
                descending: true,
            },
            Host::standalone(Window::FULL.top),
        ),
    ] {
        let _profile = crate::package_profile::hold(window, host);
        materialization_readback(&output.join(label));
    }
}

#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn materialization_readback(output: &Path) {
    match fs::symlink_metadata(output) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("Cannot inspect linker output directory: {error}"),
        Ok(_) => panic!("Linker output path already exists: {}", output.display()),
    }
    let fixture: Fixture = serde_json::from_slice(
        &fs::read(std::env::var_os("PARHELION_LINK_FIXTURE").expect("configured linker fixture"))
            .unwrap(),
    )
    .unwrap();
    let manager =
        sundial::package_authoring::open_shadowkeep_package_manager(&fixture.packages).unwrap();
    assert_eq!(
        manager
            .get_entry(TagHash(fixture.buffer_header))
            .unwrap()
            .reference,
        fixture.buffer_data
    );
    let owner = manager.read_tag(TagHash(fixture.owner_template)).unwrap();
    let allocation = manager
        .read_tag(TagHash(fixture.allocation_template))
        .unwrap();
    let header = manager.read_tag(TagHash(fixture.buffer_header)).unwrap();
    let data = manager.read_tag(TagHash(fixture.buffer_data)).unwrap();
    let parent = manager.read_tag(TagHash(fixture.parent_template)).unwrap();
    assert_eq!(
        manager
            .get_entry(TagHash(fixture.parent_template))
            .unwrap()
            .file_type,
        16
    );
    let entity_template = read_u32(&parent, 16).unwrap();
    let entity = manager.read_tag(TagHash(entity_template)).unwrap();
    let instance = 16usize
        .checked_add_signed(read_i64(&owner, 16).unwrap() as isize)
        .unwrap();
    let definition = 24usize
        .checked_add_signed(read_i64(&owner, 24).unwrap() as isize)
        .unwrap();
    assert_eq!(read_u32(&owner, instance).unwrap(), fixture.owner_template);
    assert_eq!(
        read_u32(&owner, definition).unwrap(),
        fixture.owner_template
    );
    let seed = NewTagSpec {
        template_tag: TagHash(fixture.allocation_template),
        payload: allocation.clone(),
        storage: crate::NewTagStorageMode::InheritTemplate,
    };
    let mut assets = crate::asset_packages::AssetPackages::primary(vec![seed], vec![]).unwrap();
    let mut companions = Companions::new();
    let companion = native_companion(
        &fixture.packages,
        &manager,
        fixture.parent_template,
        &mut companions,
    )
    .unwrap()
    .clone();
    let declarations = || {
        let mut companion_decl = Declaration::new(
            "companion",
            companion.0.0,
            companion.1.len() + crate::format::BLOCK_SIZE,
        );
        companion_decl.companion = Some(Companion::new("parent", fixture.parent_template));
        let mut declarations = vec![
            Declaration::new("owner", fixture.owner_template, owner.len()),
            Declaration::new("allocation", fixture.allocation_template, allocation.len()),
            Declaration::new("header", fixture.buffer_header, header.len()),
            Declaration::new("data", fixture.buffer_data, data.len()),
            Declaration::new("parent", fixture.parent_template, parent.len()),
            Declaration::new("entity", entity_template, entity.len()),
            companion_decl,
        ];
        declarations.swap(3, 4);
        declarations.splice(
            4..4,
            (0..8192).map(|index| {
                Declaration::new(
                    format!("extra-{index}"),
                    fixture.allocation_template,
                    allocation.len(),
                )
            }),
        );
        declarations
    };
    let make = |symbols: &BTreeMap<String, TagHash>| -> AuthoringResult<Vec<Node>> {
        let mut private = owner.clone();
        write_u32(&mut private, instance, symbols["owner"].0)?;
        write_u32(&mut private, definition, symbols["owner"].0)?;
        let mut owner_node = Node::new("owner", fixture.owner_template, private);
        owner_node.patch(0x44, "allocation")?;
        let mut header_node = Node::new("header", fixture.buffer_header, header.clone());
        header_node.reference = Some("data".into());
        let mut companion_node = Node::new("companion", companion.0.0, companion.1.clone());
        companion_node.companion = Some(Companion::new("parent", fixture.parent_template));
        let mut parent_node = Node::new("parent", fixture.parent_template, parent.clone());
        parent_node.patch(16, "entity")?;
        let mut nodes = vec![
            owner_node,
            Node::new(
                "allocation",
                fixture.allocation_template,
                allocation.clone(),
            ),
            header_node,
            Node::new("data", fixture.buffer_data, data.clone()),
            parent_node,
            Node::new("entity", entity_template, entity.clone()),
            companion_node,
        ];
        nodes.swap(3, 4);
        nodes.splice(
            4..4,
            (0..8192).map(|index| {
                Node::new(
                    format!("extra-{index}"),
                    fixture.allocation_template,
                    allocation.clone(),
                )
            }),
        );
        Ok(nodes)
    };
    let before = state(&assets);
    let mut failures = Vec::new();
    for fault in [
        "materializer",
        "order",
        "template",
        "bound",
        "patch",
        "overlap",
        "target",
        "groups",
    ] {
        let result = materialize(
            &fixture.packages,
            &mut assets,
            &manager,
            (declarations(), &External::default()),
            "parent",
            &mut companions,
            |symbols| {
                if fault == "materializer" {
                    return Err(invalid("Configured materializer failure"));
                }
                let mut nodes = make(symbols)?;
                match fault {
                    "order" => nodes.swap(0, 1),
                    "template" => nodes[0].template = TagHash(u32::MAX),
                    "bound" => nodes[0].payload.push(0),
                    "patch" => write_u32(&mut nodes[0].payload, 0x44, 7)?,
                    "overlap" => nodes[0].patches.push((0x45, "allocation".into())),
                    "target" => nodes[0].patches[0].1 = "missing".into(),
                    _ => {}
                }
                Ok(nodes)
            },
            |_, _| {
                if fault == "groups" {
                    Err(invalid("Configured group failure"))
                } else {
                    Ok(())
                }
            },
            None,
        );
        assert!(result.is_err(), "{fault} must refuse");
        assert_eq!(state(&assets), before, "{fault} mutated packages");
        failures.push(fault);
    }
    let streamed = {
        #[cfg(feature = "d2-model-importer")]
        {
            let mut stream = crate::asset_packages::stream::Stream::new(&assets).unwrap();
            for _ in 0..8193 {
                let (allocator, tags) = stream.target();
                allocator
                    .reserve_payload(tags.len(), allocation.len())
                    .unwrap();
                allocator
                    .assigned_tag(tags.len(), "Streamed fixture", "allocation")
                    .unwrap();
                tags.push(NewTagSpec {
                    template_tag: TagHash(fixture.allocation_template),
                    payload: allocation.clone(),
                    storage: crate::NewTagStorageMode::InheritTemplate,
                });
            }
            stream.finish(&mut assets).unwrap()
        }
        #[cfg(not(feature = "d2-model-importer"))]
        {
            Vec::<TagHash>::new()
        }
    };
    let linked = materialize(
        &fixture.packages,
        &mut assets,
        &manager,
        (declarations(), &External::default()),
        "parent",
        &mut companions,
        make,
        |symbols, groups| {
            groups
                .get_mut(&symbols["parent"])
                .unwrap()
                .extend(&streamed);
            Ok(())
        },
        None,
    )
    .unwrap();
    assets.validate().unwrap();
    assert!(assets.packages.len() > 1);
    assert_eq!(assets.packages[0].tags[0].payload, allocation);
    assert_ne!(linked.symbols["owner"].entry_index(), 0);
    assert_ne!(
        linked.symbols["header"].pkg_id(),
        linked.symbols["data"].pkg_id()
    );
    assert_eq!(
        linked.symbols["parent"].pkg_id(),
        linked.symbols["companion"].pkg_id()
    );
    let temporary = tempfile::tempdir().unwrap();
    use tiger_pkg::Package;
    let mut reopened = BTreeMap::new();
    let mut emitted = BTreeMap::new();
    let mut enrolled = false;
    for package in &assets.packages {
        let profile = crate::package_profile::authored_package(package.id).unwrap();
        let artifact = crate::extend::build_standalone_package_with_references(
            &fixture.packages,
            package.id,
            profile.file_name,
            &package.tags,
            &package.references,
        )
        .unwrap();
        let written = artifact.write_new(temporary.path()).unwrap();
        let bytes = fs::read(&written).unwrap();
        let layout = crate::format::PackageLayout::parse(&bytes).unwrap();
        let enrollment = layout.shared_tag_enrollment_rows(&bytes).unwrap();
        enrolled |= enrollment.chunks_exact(8).any(|row| {
            read_u32(row, 0).unwrap() == linked.symbols["parent"].0
                && read_u32(row, 4).unwrap() == linked.symbols["companion"].0
        });
        emitted.insert(
            profile.file_name.to_owned(),
            serde_json::json!({
                "bytes":bytes.len(),"sha256":digest(&bytes),"entries":package.tags.len()
            }),
        );
        reopened.insert(
            package.id,
            tiger_pkg::PackageD2PreBL::open(written.to_str().unwrap()).unwrap(),
        );
    }
    assert!(enrolled);
    let read = |tag: TagHash| reopened[&tag.pkg_id()].read_tag(tag).unwrap();
    let mut readback = BTreeMap::new();
    for (symbol, tag) in &linked.symbols {
        let package = assets
            .packages
            .iter()
            .find(|package| package.id == tag.pkg_id())
            .unwrap();
        let expected = &package.tags[tag.entry_index() as usize].payload;
        let payload = read(*tag);
        assert_eq!(payload, *expected);
        readback.insert(
            symbol.clone(),
            serde_json::json!({
                "tag": tag.0, "bytes": payload.len(), "sha256": digest(&payload)
            }),
        );
    }
    let private = read(linked.symbols["owner"]);
    assert_eq!(
        read_u32(&private, instance).unwrap(),
        linked.symbols["owner"].0
    );
    assert_eq!(
        read_u32(&private, definition).unwrap(),
        linked.symbols["owner"].0
    );
    assert_eq!(
        read_u32(&private, 0x44).unwrap(),
        linked.symbols["allocation"].0
    );
    let entries = reopened[&linked.symbols["header"].pkg_id()].entries();
    assert_eq!(
        entries[linked.symbols["header"].entry_index() as usize].reference,
        linked.symbols["data"].0
    );
    let loaded = crate::shared_tag_dependency_index::dependency_entries(
        &read(linked.symbols["companion"]),
        linked.symbols["companion"],
        linked.symbols["parent"],
    )
    .unwrap();
    for (symbol, tag) in &linked.symbols {
        if symbol.starts_with("extra-") {
            assert!(loaded.contains(&(tag.pkg_id(), tag.entry_index())));
        }
    }
    for tag in &streamed {
        assert!(loaded.contains(&(tag.pkg_id(), tag.entry_index())));
        assert_eq!(read(*tag), allocation);
    }
    let inputs: BTreeMap<_, _> = [
        ("owner", fixture.owner_template, owner.as_slice()),
        (
            "allocation",
            fixture.allocation_template,
            allocation.as_slice(),
        ),
        ("header", fixture.buffer_header, header.as_slice()),
        ("data", fixture.buffer_data, data.as_slice()),
        ("parent", fixture.parent_template, parent.as_slice()),
        ("entity", entity_template, entity.as_slice()),
        ("companion", companion.0.0, companion.1.as_slice()),
    ]
    .into_iter()
    .map(|(symbol, tag, payload)| {
        (
            symbol,
            serde_json::json!({
                "tag": tag, "bytes": payload.len(), "sha256": digest(payload)
            }),
        )
    })
    .collect();
    let receipt = serde_json::json!({"symbols": linked.symbols.iter().map(|(s,t)| (s.clone(), t.0)).collect::<BTreeMap<_,_>>(),
        "packages": emitted, "streamed":streamed.iter().map(|tag|tag.0).collect::<Vec<_>>(),
        "native_build":"86657.20.08.23.1800.d2_rc___release", "version":env!("CARGO_PKG_VERSION"),
        "test_executable_sha256":crate::artifact::digest_file(&std::env::current_exe().unwrap()).unwrap().sha256,
        "inputs": inputs, "private_readback": readback,
        "failure_cases": failures, "readback_verified": true});
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent).unwrap();
    }
    fs::create_dir(output).expect("linker output must still be absent");
    for name in emitted.keys() {
        let written = temporary.path().join(name);
        fs::copy(&written, output.join(name)).unwrap();
        assert_eq!(
            fs::read(output.join(name)).unwrap(),
            fs::read(written).unwrap()
        );
    }
    fs::write(
        output.join("link-materialization.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    eprintln!("{receipt}");
}
