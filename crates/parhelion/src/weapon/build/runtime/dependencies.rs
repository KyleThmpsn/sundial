//! Add native component prerequisites for authored graph and action references.
use super::*;
use sundial::package_authoring::sandbox_perk::{
    action, program::NativeProgram, projectile::residency,
};

pub(super) fn native_prerequisites<'a>(
    manager: &PackageManager,
    groups: impl IntoIterator<Item = (AppendedTagAllocator, &'a [NewTagSpec])>,
) -> AuthoringResult<Vec<TagHash>> {
    let groups = groups.into_iter().collect::<Vec<_>>();
    let mut authored = BTreeSet::new();
    for (allocator, tags) in &groups {
        for index in 0..tags.len() {
            authored.insert(
                allocator
                    .assigned_tag(index, "Runtime dependency", "authored resource")?
                    .0,
            );
        }
    }
    let mut graphs = BTreeSet::new();
    for (_, tags) in groups {
        for tag in tags {
            let source = manager
                .get_entry(tag.template_tag)
                .ok_or_else(|| invalid("Runtime dependency template is missing"))?;
            if source.reference == WEAPON_ENTITY_CLASS {
                graphs.insert(tag.template_tag.0);
            } else if source.reference == action::ACTION_ROOT_CLASS {
                // Walk all declared resource lanes, including auxiliary records, policies
                // and nested nodes. A summary's primary effect asset is not a closure.
                for graph in NativeProgram::read(&tag.payload)
                    .map_err(|error| {
                        invalid(format!(
                            "Authored action from {}: {error}",
                            tag.template_tag
                        ))
                    })?
                    .resources()
                    .map_err(invalid)?
                {
                    if authored.contains(&graph) || matches!(graph, 0 | u32::MAX | 0x811C_9DC5) {
                        continue;
                    }
                    let entry = manager.get_entry(TagHash(graph)).ok_or_else(|| {
                        invalid(format!(
                            "Authored action references missing asset 0x{graph:08X}"
                        ))
                    })?;
                    if entry.reference == WEAPON_ENTITY_CLASS {
                        graphs.insert(graph);
                    }
                }
            }
        }
    }
    let mut additions = BTreeSet::new();
    for graph in graphs {
        let report = residency::inspect(manager, graph).map_err(invalid)?;
        additions.extend(report.additions());
    }
    Ok(additions.into_iter().map(TagHash).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::sandbox_perk::{
        self,
        program::{Action, Asset, Position, Program, Trigger},
    };

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn activity_assets_and_orbs_enroll_their_checked_dependencies() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let manager = open_shadowkeep_package_manager(&packages).unwrap();
        let globals = manager
            .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
            .unwrap();
        let source =
            sandbox_perk::load_sandbox_perk_runtime_action(&manager, &globals, 421).unwrap();
        let allocator = AppendedTagAllocator::new(PARHELION_ASSET_PACKAGE_ID, 0);
        for (asset, action) in [
            (
                0x80C107D8,
                Action::Spawn {
                    asset: Asset {
                        graph: 0x80C107D8,
                        ..Asset::default()
                    },
                    position: Position::Event,
                },
            ),
            (0x80EFAE02, Action::generate_orb(Position::Event)),
        ] {
            let program = Program {
                trigger: Trigger::WeaponKill,
                actions: vec![action],
                ..Program::default()
            };
            let compiled = sandbox_perk::program::compile(&manager, &program).unwrap();
            let tags = vec![NewTagSpec {
                template_tag: source.action_tag,
                payload: compiled.payload,
                storage: crate::NewTagStorageMode::InheritTemplate,
            }];
            let additions = native_prerequisites(&manager, [(allocator, tags.as_slice())])
                .unwrap()
                .into_iter()
                .map(|tag| tag.0)
                .collect::<BTreeSet<_>>();
            let expected = residency::inspect(&manager, asset).unwrap().additions();
            assert!(expected.is_subset(&additions), "0x{asset:08X}");
            if asset == 0x80C107D8 {
                assert!(!expected.is_empty());
                for tag in [asset, 0x80BFD097, 0x80BFD098, 0x80C10790] {
                    assert!(additions.contains(&tag), "0x{tag:08X}");
                }
            }
        }
        assert_eq!(
            manager.read_tag(source.action_tag).unwrap(),
            source.action_payload
        );
    }
}
