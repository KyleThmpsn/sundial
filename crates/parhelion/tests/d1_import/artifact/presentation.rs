//! Native inspection keys and the actual Collections ancestry are independent UI contracts.
use super::*;

pub(super) fn check(
    native: &mut Reader,
    globals: &Payload,
    nodes: &Payload,
    (item_hash, donor): (u32, u32),
    pages: &[u16],
    source_class: u8,
) -> Result<Value> {
    // Stock helmets of the source's class hold these keys (FNV-1 of `armor_titan`,
    // `armor_hunter`, `armor_warlock` and `armor`) and item type indexes. The tooltip names a
    // required class from the key.
    let (class_key, item_type) = match source_class {
        0 => (0xD60E6BA5u32, 0x19u16),
        1 => (0xD74EBFB3, 0x18),
        2 => (0x82E15A90, 0x1A),
        _ => (0x27785BD2, 0x18),
    };
    let table = native.tag(globals.u32(16 + 33 * 16)?, None)?;
    // The seasonal template shows a localized line while its season's unlock flag is set. The
    // import must drop its copy and leave the template's own row in place.
    let template = table
        .array(8, 24, Some(0x80805CDF))?
        .into_iter()
        .find(|&row| table.u32(row).ok() == Some(donor))
        .context("Artifact template presentation entry")?;
    let template = native.tag(table.u32(template + 16)?, None)?;
    let template_notices = template.array(0xD0, 0x28, Some(0x80802B22))?.len();
    let row = table
        .array(8, 24, Some(0x80805CDF))?
        .into_iter()
        .find(|&row| table.u32(row).ok() == Some(item_hash))
        .context("Artifact presentation entry")?;
    let strings = native.tag(table.u32(row + 16)?, None)?;
    let action = strings.pointer(0x38)?;
    ensure!(
        strings.u32(action - 4)? == 0x80805D10,
        "Inspect action class"
    );
    // The original UI router maps this key to the ordinary item screen. Its seasonal route
    // has another key and a vendor index, independent of the item's equipment bucket.
    ensure!(
        strings.u32(action)? == 0x66E18DD8 && strings.u16(action + 4)? == u16::MAX,
        "Artifact inspection still routes to seasonal presentation"
    );
    ensure!(
        strings.u64(action + 0x10)? == 0,
        "Inspect action retained a donor condition"
    );
    ensure!(
        strings.u64(0xD0)? == 0,
        "Artifact retained the seasonal template's conditional notice"
    );
    // The template is classified `ghost`, for which the ordinary screen stages a Ghost and
    // lists Ghost sockets. The import's talents are armor perk sockets, so it must hold its
    // class's armor key with `head` and leave the rest of the tuple empty.
    ensure!(
        template.u32(0xB8)? == 0xEFCD14BE,
        "Artifact template is not classified as a Ghost item"
    );
    ensure!(
        strings.u32(0xB8)? == class_key && strings.u32(0xBC)? == 0xCD887E07,
        "Artifact is not classified as its class's armor"
    );
    ensure!(
        strings.u32(0xC0)? == 0x811C9DC5
            && strings.u32(0xC4)? == 0x811C9DC5
            && strings.u32(0xC8)? == 0x811C9DC5,
        "Artifact classification tuple kept other keys"
    );
    // The template names no delete action, a seasonal display style and no item type. The
    // import must offer Dismantle and carry ordinary gear's style and the armor type.
    let records = |strings: &Payload| -> Result<(u32, u32, u16, u16, u16)> {
        let actions = strings.pointer(0x08)?;
        let display = strings.pointer(0x10)?;
        let kind = strings.pointer(0x18)?;
        ensure!(
            strings.u32(actions - 4)? == 0x80805D25
                && strings.u32(display - 4)? == 0x80805D1C
                && strings.u32(kind - 4)? == 0x80805D1A,
            "Item display record classes"
        );
        Ok((
            strings.u32(actions)?,
            strings.u32(actions + 4)?,
            strings.u16(actions + 0x14)?,
            strings.u16(display)?,
            strings.u16(kind + 2)?,
        ))
    };
    ensure!(
        records(&template)? == (0xFFFF, 0x811C9DC5, u16::MAX, 0x6B, u16::MAX),
        "Artifact template display records differ"
    );
    ensure!(
        records(&strings)? == (5, 0x7B9E5F5F, 19, 0x0A, item_type),
        "Artifact lacks the Dismantle action, gear display style or armor type"
    );
    // Item strings `+CC` hold the display style. The template holds the empty name, and the
    // import must hold FNV-1 of `ui_display_style_authored_artifact`, which the character
    // screen's decision below compares.
    ensure!(
        template.u32(0xCC)? == 0x811C9DC5 && strings.u32(0xCC)? == 0x9C2E2F90,
        "Artifact display style differs"
    );
    // The client reads an item's stat group from the display block at strings `+70`, which the
    // template lacks, and finds no stats without one. A stock Legendary helmet's block holds
    // two empty names, no damage type, 5, the armor kind `5E813014` and group 4, whose stats
    // are Mobility to Strength. The strings size word must cover the added block.
    ensure!(
        template.u64(0x70)? == 0,
        "Artifact template holds a stat display block"
    );
    let block = strings.pointer(0x70)?;
    let mut words = [0u32; 6];
    for (index, word) in words.iter_mut().enumerate() {
        *word = strings.u32(block + index * 4)?;
    }
    ensure!(
        strings.u32(block - 4)? == 0x80805CF1
            && block % 4 == 0
            && words == [0x811C9DC5, 0x811C9DC5, 0, 5, 0x5E813014, 4],
        "Artifact stat display block {words:X?} is not a helmet's"
    );
    ensure!(
        usize::try_from(strings.u64(0)?)? == strings.0.len(),
        "Artifact strings size word differs from its payload"
    );
    // The inspection screen's socket component lists an item's sockets only when the item
    // strings hold the block at `+58` beside the definition's sockets. Every stock item with
    // sockets holds it, an eight-byte label reference, and no stock item without sockets does.
    // The template has no sockets and no block. The import has sockets, so it must hold a
    // helmet's reference, entry `3AC0DC75` of string table 5.
    ensure!(
        template.u64(0x58)? == 0,
        "Artifact template holds a socket label block"
    );
    let label = strings.pointer(0x58)?;
    ensure!(
        strings.u32(label - 4)? == 0x80805D0B
            && label % 4 == 0
            && strings.u32(label)? == 5
            && strings.u32(label + 4)? == 0x3AC0DC75,
        "Artifact socket label block is not a helmet's"
    );
    artifact_buttons(native)?;
    tooltip_decisions(native)?;
    let hashes = ancestry(nodes, pages)?;
    ensure!(
        hashes.contains(&0x5CDCBF0A) && hashes.contains(&0xCA512551),
        "Artifact is outside Equipment's native Artifacts branch"
    );
    ensure!(
        !hashes.contains(&0xA3633733) && !hashes.contains(&0x3E5AF28B),
        "Artifact still belongs to Ghost Shells"
    );
    Ok(json!({"inspect_key":format!("{:08X}",strings.u32(action)?),
        "vendor_index":strings.u16(action+4)?,
        "conditional_notices":0,"template_conditional_notices":template_notices,
        "ancestor_hashes":hashes.iter().map(|hash|format!("{hash:08X}")).collect::<Vec<_>>(),
        "live_inspection_verified":false}))
}

fn artifact_buttons(native: &mut Reader) -> Result<()> {
    // The character screen tables set the artifact tooltip (`50211623`) on the artifact slot's
    // two item buttons, components 154 and 185, through property 218. The staged tables must
    // keep each button's other property, path `[400]` field 8 with its stock value, and drop
    // that one.
    for screen in [0x80BC6F93u32, 0x80BC6F94] {
        let table = native.tag(screen, Some(0x80804825))?;
        let mut buttons = Vec::new();
        for object in table.array(0x48, 64, Some(0x8080462A))? {
            for component in table.array(object + 0x20, 24, Some(0x808046D4))? {
                let index = table.u64(component)?;
                if index != 0x154 && index != 0x185 {
                    continue;
                }
                let mut properties = Vec::new();
                for property in table.array(component + 8, 24, Some(0x80804858))? {
                    properties.push((
                        table.u64(property + 8)?,
                        table.u32(table.pointer(property + 0x10)?)?,
                    ));
                }
                buttons.push((index, properties));
            }
        }
        ensure!(
            buttons == [(0x154, vec![(8, 1)]), (0x185, vec![(8, 0x12)])],
            "Character screen {screen:08X} artifact buttons hold {buttons:X?}"
        );
    }
    Ok(())
}

fn tooltip_decisions(native: &mut Reader) -> Result<()> {
    // Each root table holding the screen must gain one name decision after its stock
    // components: value type 6, the gear tooltip `3BFD3F80` when the equipped button's display
    // style equals the import's and the artifact tooltip `50211623` otherwise, bound to the
    // equipped button's tooltip name. The hierarchy must place it under the root.
    for (root, hierarchy) in [
        (0x80BC6F93u32, 0x80B46E13u32),
        (0x80BC6882, 0x80BC630C),
        (0x80BC6F65, 0x80B46DE5),
    ] {
        let table = native.tag(root, Some(0x80804825))?;
        let tree = native.tag(hierarchy, Some(0x8080496A))?;
        ensure!(
            tree.u32(28)? == root,
            "Screen hierarchy names another table"
        );
        let rows = tree.array(8, 4, Some(0x80804616))?;
        let decision = u16::try_from(rows.len() - 1)?;
        ensure!(
            tree.u32(24)? == rows.len() as u32 * 0x10001
                && rows
                    .iter()
                    .filter(|&&row| tree.u16(row + 2).ok() == Some(decision))
                    .map(|&row| tree.u16(row))
                    .collect::<Result<Vec<_>, _>>()?
                    == [0],
            "Screen {root:08X} hierarchy does not place the decision under its root"
        );
        let classes = table.array(0x38, 16, Some(0x80804622))?;
        let last = *classes.last().context("Screen class rows")?;
        ensure!(
            table.u64(last)? == 0xFBB44B29 && table.u64(last + 8)? == u64::from(decision) << 16,
            "Screen {root:08X} has no decision class row"
        );
        let mut stored = Vec::new();
        for object in table.array(0x48, 64, Some(0x8080462A))? {
            for component in table.array(object + 0x20, 24, Some(0x808046D4))? {
                if table.u64(component)? != u64::from(decision) {
                    continue;
                }
                for property in table.array(component + 8, 24, Some(0x80804858))? {
                    stored.push((
                        table.u64(property + 8)?,
                        table.u32(table.pointer(property + 0x10)?)?,
                    ));
                }
            }
        }
        ensure!(
            stored == [(0x201, 6), (0x203, 0x3BFD3F80), (0x204, 0x50211623)],
            "Screen {root:08X} decision stores {stored:X?}"
        );
        let path = |field: usize| -> Result<Vec<u32>> {
            let descriptor = table.pointer(field)?;
            table
                .array(descriptor, 4, Some(0x80804949))?
                .into_iter()
                .map(|at| table.u32(at))
                .collect()
        };
        let (mut condition, mut output) = (0, 0);
        for binding in table.array(0x58, 56, Some(0x808046D8))? {
            let (source, target) = (table.u64(binding)?, table.u64(binding + 24)?);
            let own = u64::from(decision) * 0x10001;
            if target == own && table.u64(binding + 40)? == 0x202 {
                let conversion = table.pointer(binding + 48)?;
                ensure!(
                    source == 0x154 * 0x10001
                        && table.u64(binding + 16)? == 0x21
                        && path(binding + 8)? == [0x400, 0x17, 2]
                        && table.u32(conversion - 4)? == 0x808046DA
                        && table.u32(conversion)? == 3
                        && table.u32(conversion + 4)? == 0x9C2E2F90,
                    "Screen {root:08X} decision condition differs"
                );
                condition += 1;
            }
            if source == own {
                ensure!(
                    table.u64(binding + 16)? == 0x205
                        && target == 0x154 * 0x10001
                        && table.u64(binding + 40)? == 0x218
                        && path(binding + 32)?.is_empty(),
                    "Screen {root:08X} decision output differs"
                );
                output += 1;
            }
        }
        ensure!(
            (condition, output) == (1, 1),
            "Screen {root:08X} decision has {condition} conditions and {output} outputs"
        );
    }
    Ok(())
}

/// The hashes of every Collections node above `pages`.
fn ancestry(nodes: &Payload, pages: &[u16]) -> Result<BTreeSet<u32>> {
    let rows = nodes.array(8, 0xA8, Some(0x80803056))?;
    let mut pending = pages.to_vec();
    let mut visited = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    while let Some(index) = pending.pop() {
        if !visited.insert(index) {
            continue;
        }
        let row = *rows
            .get(usize::from(index))
            .context("Collections parent index")?;
        hashes.insert(nodes.u32(row + 0x28)?);
        for parent in nodes.array(row + 0x18, 2, Some(0x80803962))? {
            pending.push(nodes.u16(parent)?);
        }
    }
    Ok(hashes)
}
