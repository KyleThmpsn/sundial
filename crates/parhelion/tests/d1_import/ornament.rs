//! Where a converted weapon's ornaments sit in the staged Collections. An Exotic's ornaments join
//! its weapon type's page under Exotics and Weapon Ornaments, and an ordinary weapon's its type's
//! page under Weapons and Weapon Ornaments. The page must name each ornament among its children,
//! the list Collections shows, and its stock ornaments must be ones a stock weapon of the source's
//! type takes.
use super::*;

/// Default Ornament, the reset every stock weapon ornament socket starts with.
const RESET: u32 = 0xAEBAE371;
const PLUG_BLOCK: u32 = 0x808077E3;

struct Tables {
    root: Payload,
    items: Payload,
    collectibles: Payload,
    nodes: Payload,
    /// Item table row by hash.
    rows: BTreeMap<u32, usize>,
}

impl Tables {
    fn read(staged: &mut Reader) -> Result<Self> {
        let tag = staged
            .manager
            .lookup
            .named_tags
            .iter()
            .find(|n| n.name == "investment_globals")
            .context("Globals")?
            .hash
            .0;
        let globals = staged.tag(tag, None)?;
        let root = (*staged.tag(globals.u32(16)?, None)?).clone();
        let items = (*staged.tag(root.u32(8 + 48 * 16)?, None)?).clone();
        let collectibles = (*staged.tag(root.u32(8 + 19 * 16)?, None)?).clone();
        let nodes = (*staged.tag(root.u32(8 + 63 * 16)?, None)?).clone();
        let mut rows = BTreeMap::new();
        for (index, row) in items
            .array(8, 24, Some(0x80807BE8))?
            .into_iter()
            .enumerate()
        {
            rows.entry(items.u32(row)?).or_insert(index);
        }
        Ok(Self {
            root,
            items,
            collectibles,
            nodes,
            rows,
        })
    }

    fn index(&self, hash: u32) -> Result<usize> {
        self.rows
            .get(&hash)
            .copied()
            .with_context(|| format!("Item {hash:08X} absent"))
    }

    fn definition(&self, staged: &mut Reader, hash: u32) -> Result<Payload> {
        let row = self.items.array(8, 24, Some(0x80807BE8))?[self.index(hash)?];
        Ok((*staged.tag(self.items.u32(row + 16)?, Some(0x80807BEA))?).clone())
    }

    fn hash_at(&self, index: usize) -> Result<u32> {
        let rows = self.items.array(8, 24, Some(0x80807BE8))?;
        self.items.u32(*rows.get(index).context("Item index")?)
    }

    fn node_rows(&self) -> Result<Vec<usize>> {
        self.nodes.array(8, 0xA8, Some(0x80803056))
    }

    fn collectible_rows(&self) -> Result<Vec<usize>> {
        self.collectibles.array(8, 0xB8, Some(0x80803475))
    }

    fn parents(p: &Payload, row: usize) -> Result<Vec<u16>> {
        p.array(row + 0x18, 2, Some(0x80803962))?
            .into_iter()
            .map(|at| p.u16(at))
            .collect()
    }

    /// The collectibles a node names as its children, which Collections lists on its page.
    fn children(&self, node: u16) -> Result<Vec<usize>> {
        let row = *self
            .node_rows()?
            .get(usize::from(node))
            .context("Node row")?;
        if self.nodes.u64(row + 0x78)? == 0 {
            return Ok(Vec::new());
        }
        self.nodes
            .array(row + 0x78, 4, Some(0x80803068))?
            .into_iter()
            .map(|at| Ok(usize::from(self.nodes.u16(at)?)))
            .collect()
    }

    /// The plug categories the ornament socket of a weapon takes, the reset's among them.
    fn ornament_categories(&self, staged: &mut Reader, weapon: u32) -> Result<Vec<u32>> {
        let definition = self.definition(staged, weapon)?;
        if definition.u64(0x68)? == 0 {
            return Ok(Vec::new());
        }
        let socket_types = self.root.u32(8 + 94 * 16)?;
        let table = staged.tag(socket_types, None)?;
        let types = table.array(8, 96, Some(0x80807ABB))?;
        let mut categories = Vec::new();
        for row in definition.array(definition.pointer(0x68)?, 0x50, Some(0x808077C4))? {
            let initial = definition.u16(row + 2)?;
            if initial == u16::MAX || self.hash_at(usize::from(initial))? != RESET {
                continue;
            }
            let at = *types
                .get(usize::from(definition.u16(row)?))
                .context("Socket type row")?;
            for at in table.array(at + 0x28, 8, Some(0x80807AC0))? {
                categories.push(table.u32(at)?);
            }
        }
        Ok(categories)
    }

    fn category(&self, staged: &mut Reader, item: usize) -> Result<Option<u32>> {
        let definition = self.definition(staged, self.hash_at(item)?)?;
        let Ok(block) = definition.pointer(0x40) else {
            return Ok(None);
        };
        (block >= 4 && definition.u32(block - 4)? == PLUG_BLOCK)
            .then(|| definition.u32(block))
            .transpose()
    }
}

/// Checks each converted weapon ornament of `recipe` and returns the evidence. `kin` lists the
/// stock weapons of the source's type.
pub(super) fn check(staged: &mut Reader, recipe: &WeaponRecipe, kin: &[u32]) -> Result<Value> {
    let mut ornaments = Vec::new();
    for attachment in recipe
        .overrides
        .imported_graph
        .iter()
        .flat_map(|reference| &reference.attachments)
    {
        let graph: Value =
            serde_json::from_slice(&fs::read(attachment.directory.join("asset-graph.json"))?)?;
        if graph["ornament"].get("target_weapon").is_some() {
            ornaments.push(graph);
        }
    }
    if ornaments.is_empty() {
        return Ok(json!([]));
    }
    let tables = Tables::read(staged)?;
    let owner = recipe.identity.item_hash.parse_u32()?;
    let exotic = tables
        .definition(staged, owner)?
        .u8(sundial::package_authoring::investment_schema::ITEM_RARITY_OFFSET)?
        == 5;
    let mut type_categories = BTreeSet::new();
    for &weapon in kin {
        // A stock weapon whose sockets fall outside the weapon layout offers no ornament socket,
        // as the importer's layout treats it.
        if let Ok(categories) = tables.ornament_categories(staged, weapon) {
            type_categories.extend(categories);
        }
    }
    type_categories.remove(&tables.category(staged, tables.index(RESET)?)?.unwrap_or(0));
    ensure!(
        !type_categories.is_empty(),
        "No stock weapon of this type has an ornament socket"
    );
    let node_rows = tables.node_rows()?;
    let collectible_rows = tables.collectible_rows()?;
    let mut evidence = Vec::new();
    for graph in &ornaments {
        ensure!(
            number(&graph["ornament"]["target_weapon"])? == owner,
            "Ornament names another owner"
        );
        let plug = number(&graph["item_hash"])?;
        let item = u16::try_from(tables.index(plug)?)?;
        let found = collectible_rows
            .iter()
            .enumerate()
            .filter(|&(_, &row)| tables.collectibles.u16(row + 0x2C).ok() == Some(item))
            .map(|(index, &row)| (index, row))
            .collect::<Vec<_>>();
        let [(collectible, row)] = found[..] else {
            anyhow::bail!(
                "Ornament {} has {} collectibles, not one",
                graph["ornament"]["name"],
                found.len()
            );
        };
        let parents = Tables::parents(&tables.collectibles, row)?;
        let [page] = parents[..] else {
            anyhow::bail!(
                "Ornament {} is on {} pages",
                graph["ornament"]["name"],
                parents.len()
            );
        };
        let children = tables.children(page)?;
        ensure!(
            children.contains(&collectible),
            "Ornament {}'s page does not list it",
            graph["ornament"]["name"]
        );
        let mut path = Vec::new();
        let mut current = vec![page];
        while let [node] = current[..] {
            let row = *node_rows.get(usize::from(node)).context("Node row")?;
            path.push(tables.nodes.u32(row + 0x28)?);
            current = Tables::parents(&tables.nodes, row)?;
        }
        let expected: &[u32] = if exotic {
            &[0x47D9E328, 0x3FB0E331]
        } else {
            &[0xBC18ECA7, 0x5B219F74]
        };
        ensure!(
            path.get(1..3) == Some(expected),
            "Ornament {} is on {path:08X?}, not under {expected:08X?}",
            graph["ornament"]["name"]
        );
        // The page's stock ornaments fit some stock weapon of the source's type. A page the build
        // added for a type without one has no stock ornaments.
        let mut stock = Vec::new();
        for &child in children.iter().filter(|&&child| child != collectible) {
            let row = *collectible_rows.get(child).context("Collectible row")?;
            let item = usize::from(tables.collectibles.u16(row + 0x2C)?);
            if let Some(category) = tables.category(staged, item)? {
                stock.push((tables.hash_at(item)?, category));
            }
        }
        let fitting = stock
            .iter()
            .filter(|(_, category)| type_categories.contains(category))
            .count();
        ensure!(
            fitting > 0 || (!exotic && stock.is_empty()),
            "Ornament {}'s page holds no ornament of its weapon type",
            graph["ornament"]["name"]
        );
        evidence.push(
            json!({"name":graph["ornament"]["name"],"plug":format!("{plug:08X}"),
            "layout_plug":format!("{:08X}", number(&graph["ornament"]["native_plug"])?),
            "collectible":collectible,"page":page,
            "path":path.iter().map(|h| format!("{h:08X}")).collect::<Vec<_>>(),
            "page_ornaments":stock.len(),"page_ornaments_of_type":fitting}),
        );
    }
    Ok(json!(evidence))
}
