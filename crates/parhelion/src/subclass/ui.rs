//! Native CUI color decisions, scoped by each private icon's presence flag. Stock icons keep
//! their existing theme. Root-only additions leave imported component indexes untouched.
use sundial::package_authoring::{
    PackageManager,
    ui::{self, Binding, ColorSwitch, Endpoint},
};
use tiger_pkg::TagHash;

use crate::{AuthoringResult, ReplacementSpec, error::invalid};

fn endpoint(component: u16, path: &[u32], property: u32) -> Endpoint {
    Endpoint {
        component,
        path: path.to_vec(),
        property,
    }
}

fn icon(component: u16, lane: u32, property: u32) -> Endpoint {
    endpoint(component, &[0x600, 0], property | (lane << 16))
}

fn read(manager: &PackageManager, tag: u32, class: u32) -> AuthoringResult<Vec<u8>> {
    let tag = TagHash(tag);
    if manager
        .get_entry(tag)
        .is_none_or(|entry| entry.reference != class)
    {
        return Err(invalid(format!(
            "UI color source {tag} has an unsupported class"
        )));
    }
    manager
        .read_tag(tag)
        .map_err(|error| invalid(format!("UI color source {tag}: {error}")))
}

fn pair(
    manager: &PackageManager,
    widget: u32,
    hierarchy: u32,
) -> AuthoringResult<(Vec<u8>, Vec<u8>)> {
    let data = read(manager, widget, ui::widget_class())?;
    let tree = read(manager, hierarchy, ui::hierarchy_class())?;
    if sundial::package_authoring::native_payload::bytes_at::<4>(&tree, 28).map_err(invalid)?
        != widget.to_le_bytes()
    {
        return Err(invalid("The UI hierarchy names a different widget table"));
    }
    Ok((data, tree))
}

fn push_pair(output: &mut Vec<ReplacementSpec>, tags: (u32, u32), data: (Vec<u8>, Vec<u8>)) {
    output.push(ReplacementSpec {
        tag: TagHash(tags.0),
        payload: data.0,
    });
    output.push(ReplacementSpec {
        tag: TagHash(tags.1),
        payload: data.1,
    });
}

pub(crate) fn build(manager: &PackageManager) -> AuthoringResult<Vec<ReplacementSpec>> {
    let mut output = Vec::new();
    // Both Shadowkeep subclass-screen roots use the same native component layout. Each
    // decision sits beside the colored bitmap inside its dynamically repeated node branch.
    for tags in [(0x80BC7482, 0x80BC7483), (0x80B47381, 0x80EFC14F)] {
        let (data, tree) = pair(manager, tags.0, tags.1)?;
        let mut removed = Vec::new();
        let mut switches = Vec::new();
        for (index, (glyph, target)) in [(0xA5, 0xA1), (0x10A, 0x106), (0x16F, 0x16B)]
            .into_iter()
            .enumerate()
        {
            let fallback = endpoint(0x1C3, &[0x80C], 0x20C);
            let target = endpoint(target, &[], 0x20C);
            removed.push(Binding {
                source: fallback.clone(),
                target: target.clone(),
            });
            switches.push(ColorSwitch {
                name: crate::presentation::text_hash("ability-color", &format!("node-{index}")),
                sibling: target.component,
                condition: icon(glyph, 0, 7),
                when_true: icon(glyph, 0, 6),
                when_false: fallback,
                outputs: vec![target],
            });
        }
        let data = ui::remove_bindings(&data, &removed).map_err(invalid)?;
        push_pair(
            &mut output,
            tags,
            ui::color_switches(&data, &tree, &switches).map_err(invalid)?,
        );
    }
    // These are the two original theme inputs to the ability tile's charge-state decisions.
    // Their other inputs and outputs, including readiness and cooldown, remain stock.
    let tile = 0x80BC7142;
    let data = read(manager, tile, ui::widget_class())?;
    let removed = [9, 10].map(|target| Binding {
        source: icon(0x24, 0, 6),
        target: endpoint(target, &[], 0x203),
    });
    output.push(ReplacementSpec {
        tag: TagHash(tile),
        payload: ui::remove_bindings(&data, &removed).map_err(invalid)?,
    });
    // All three roots which import that tile. The nested import starts are 0 + 0x22C + 0x65.
    for tags in [
        (0x80BC6F57, 0x80BC6F5A),
        (0x80BC6FB5, 0x80BC6FB6),
        (0x80BC7261, 0x80BC7262),
    ] {
        let (data, tree) = pair(manager, tags.0, tags.1)?;
        let first = ui::component_count(&tree).map_err(invalid)?;
        let switches = [
            ColorSwitch {
                name: crate::presentation::text_hash("ability-color", "inherited-theme"),
                sibling: 0x29A,
                condition: icon(0x2B5, 1, 7),
                when_true: icon(0x2B5, 1, 6),
                when_false: icon(0x2B5, 0, 6),
                outputs: Vec::new(),
            },
            ColorSwitch {
                name: crate::presentation::text_hash("ability-color", "tile"),
                sibling: 0x29A,
                condition: icon(0x2A1, 2, 7),
                when_true: icon(0x2A1, 2, 6),
                when_false: endpoint(first, &[], 0x205),
                outputs: vec![endpoint(0x29A, &[], 0x203), endpoint(0x29B, &[], 0x203)],
            },
        ];
        push_pair(
            &mut output,
            tags,
            ui::color_switches(&data, &tree, &switches).map_err(invalid)?,
        );
    }
    Ok(output)
}
