//! Imported particle assets: converted particle systems with their programs, materials,
//! shaders and textures. They take a group in the asset packages, where each node gets its
//! tag, its package reference and the tags of the nodes it names.
use super::assets::audio;
use super::imports::Inputs;
use super::*;
use crate::tag_payload::read_u16;
use std::collections::{BTreeMap, BTreeSet};
mod ability;
mod hud;
pub(in crate::item) use ability::symbol as ability_symbol;
mod projectile;
use super::assets::replication;
mod shard;

type Node = super::assets::Node<audio::Metadata>;

pub(in crate::item) struct ImportedParticles {
    nodes: Vec<Node>,
    hud: Vec<hud::Binding>,
    projectile: Option<String>,
    /// Further projectile roots the weapon fires only where one of its private perks names one
    /// in place of a native projectile, as a pattern override does. Each flies by the fired
    /// projectile's launch policy.
    perk_projectiles: BTreeSet<String>,
    pub(in crate::item) source_trajectory: bool,
    /// The projectile is a retained native carrier fired as its own weapon fires it, so neither
    /// the imported speed fit nor the source launch adjustment applies.
    retained_trajectory: bool,
    attachments: BTreeSet<String>,
    presentation: Option<Presentation>,
    sword: Option<super::sword_profiles::Imported>,
}

struct Presentation {
    entity: u32,
    root: Option<String>,
    components: Vec<PresentationComponent>,
}

struct PresentationComponent {
    source_owner: u32,
    owner: String,
    class: u32,
}

fn presentation_number(part: &serde_json::Value, field: &str) -> AuthoringResult<u32> {
    part[field]
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .filter(|number| ![0, u32::MAX].contains(number))
        .ok_or_else(|| invalid(format!("Imported runtime presentation lacks {field}")))
}

fn presentation_component(
    part: &serde_json::Value,
    nodes: &[Node],
    required_class: Option<u32>,
) -> AuthoringResult<PresentationComponent> {
    let source_owner = presentation_number(part, "source_owner")?;
    let class = match required_class {
        Some(class) => class,
        None => presentation_number(part, "class")?,
    };
    if class & 0xffff0000 != 0x80800000 {
        return Err(invalid("Imported runtime component lacks its native class"));
    }
    let owner = part["owner"]
        .as_str()
        .filter(|symbol| !symbol.is_empty())
        .ok_or_else(|| invalid("Imported runtime component lacks its owner"))?
        .to_owned();
    let node = nodes
        .iter()
        .find(|node| node.symbol == owner)
        .ok_or_else(|| invalid("Imported runtime component owner is absent"))?;
    if node.template != source_owner || node.reference.is_some() {
        return Err(invalid(
            "Imported runtime component does not match its source template",
        ));
    }
    Ok(PresentationComponent {
        source_owner,
        owner,
        class,
    })
}

fn presentation(part: &serde_json::Value, nodes: &[Node]) -> AuthoringResult<Presentation> {
    let entity = presentation_number(part, "source_entity")?;
    if let Some(root) = part.get("root") {
        let root = root
            .as_str()
            .filter(|root| !root.is_empty())
            .ok_or_else(|| invalid("Imported vehicle runtime lacks its root symbol"))?;
        let node = nodes
            .iter()
            .find(|node| node.symbol == root)
            .ok_or_else(|| invalid("Imported vehicle runtime root is absent"))?;
        if node.template != entity
            || node.reference.is_some()
            || read_u16(&node.payload, 0x96)? != 15
        {
            return Err(invalid(
                "Imported vehicle runtime root is not its declared vehicle",
            ));
        }
        replication::validate(nodes, node)?;
        return Ok(Presentation {
            entity,
            root: Some(root.to_owned()),
            components: Vec::new(),
        });
    }
    let mut components = vec![presentation_component(part, nodes, Some(0x808072bd))?];
    if let Some(entries) = part.get("components") {
        for entry in entries
            .as_array()
            .ok_or_else(|| invalid("Imported runtime components are not an array"))?
        {
            let component = presentation_component(entry, nodes, None)?;
            if components.iter().any(|existing| {
                existing.source_owner == component.source_owner || existing.owner == component.owner
            }) {
                return Err(invalid("Imported runtime component is duplicated"));
            }
            components.push(component);
        }
    }
    Ok(Presentation {
        entity,
        root: None,
        components,
    })
}

pub(in crate::item) fn load(graph: &Inputs) -> AuthoringResult<Option<ImportedParticles>> {
    let value = graph.value();
    let mut nodes = Vec::new();
    for section in [
        "particles",
        "projectile",
        "attachments",
        "runtime_presentation",
        "audio_assets",
        "sword",
    ] {
        if let Some(part) = value.get(section) {
            if part["installable"] != true {
                return Err(invalid(format!(
                    "Imported {section} is not marked installable"
                )));
            }
            nodes.extend(
                part["nodes"]
                    .as_array()
                    .ok_or_else(|| invalid(format!("Imported {section} lacks its asset nodes")))?,
            );
        }
    }
    // A translated firing effect's particle closure takes this group. Its rebuilt flow joins the
    // firing presentation instead, which names these systems by symbol.
    if let Some(part) = value
        .get("firing_effect")
        .and_then(|effect| effect.get("assets"))
    {
        if part["installable"] != true {
            return Err(invalid("Imported firing effect is not marked installable"));
        }
        nodes.extend(
            part["nodes"]
                .as_array()
                .ok_or_else(|| invalid("Imported firing effect lacks its asset nodes"))?,
        );
    }
    let projectile = value
        .get("projectile")
        .map(|section| {
            section["root"]
                .as_str()
                .filter(|symbol| !symbol.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| invalid("Imported projectile lacks its root symbol"))
        })
        .transpose()?;
    // A perk projectile is launched by the weapon's own Barrel, which the fired projectile's
    // launch policy prepares, so each one must share that policy.
    let mut perk_projectiles = BTreeSet::new();
    for part in value
        .get("additional_projectiles")
        .map(|sections| {
            sections
                .as_array()
                .ok_or_else(|| invalid("Imported additional projectiles are not an array"))
        })
        .transpose()?
        .into_iter()
        .flatten()
    {
        if part["installable"] != true {
            return Err(invalid(
                "Imported additional projectile is not marked installable",
            ));
        }
        if projectile.is_none()
            || part["conversion"]["launch_policy"]
                != value["projectile"]["conversion"]["launch_policy"]
        {
            return Err(invalid(
                "An imported additional projectile needs the fired projectile's launch policy",
            ));
        }
        let root = part["root"]
            .as_str()
            .filter(|symbol| !symbol.is_empty())
            .ok_or_else(|| invalid("Imported additional projectile lacks its root symbol"))?;
        if projectile.as_deref() == Some(root) || !perk_projectiles.insert(root.to_owned()) {
            return Err(invalid("Imported projectile root is repeated"));
        }
        nodes.extend(
            part["nodes"]
                .as_array()
                .ok_or_else(|| invalid("Imported additional projectile lacks its asset nodes"))?,
        );
    }
    let mut attachments = BTreeSet::new();
    if let Some(section) = value.get("attachments") {
        let roots = section["roots"]
            .as_array()
            .filter(|roots| !roots.is_empty())
            .ok_or_else(|| invalid("Imported attachments lack their root symbols"))?;
        for root in roots {
            let root = root
                .as_str()
                .filter(|root| !root.trim().is_empty())
                .ok_or_else(|| invalid("Imported attachment root is empty"))?;
            if !attachments.insert(root.to_owned()) {
                return Err(invalid("Imported attachment root is repeated"));
            }
        }
    }
    let mut symbols = BTreeSet::new();
    let mut result = Vec::with_capacity(nodes.len());
    for node in nodes {
        let symbol = node["symbol"]
            .as_str()
            .ok_or_else(|| invalid("Imported particle node lacks a symbol"))?;
        if !symbols.insert(symbol.to_owned()) {
            return Err(invalid(format!(
                "Imported particle node {symbol} is repeated"
            )));
        }
        let file = node["file"]
            .as_str()
            .ok_or_else(|| invalid("Imported particle node lacks a file"))?;
        let path = std::path::Path::new(file);
        if !path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return Err(invalid("Imported particle file leaves its graph folder"));
        }
        let payload = graph
            .read(path)
            .map_err(|error| invalid(format!("Imported particle {symbol}: {error}")))?;
        let template = node["template"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| invalid(format!("Imported particle {symbol} lacks a template")))?;
        let mut patches = Vec::new();
        for patch in node["patches"].as_array().into_iter().flatten() {
            let offset = patch["offset"]
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| invalid("Imported particle patch lacks an offset"))?;
            let target = patch["symbol"]
                .as_str()
                .ok_or_else(|| invalid("Imported particle patch lacks a symbol"))?;
            patches.push((offset, target.to_owned()));
        }
        result.push(Node {
            symbol: symbol.to_owned(),
            template,
            payload,
            reference: node["reference"].as_str().map(str::to_owned),
            storage: match node["storage"].as_str() {
                None => crate::NewTagStorageMode::InheritTemplate,
                Some("audio_bank") => crate::NewTagStorageMode::AudioBank,
                Some("audio_media") => crate::NewTagStorageMode::AudioMedia,
                Some(_) => return Err(invalid("Imported asset has an unsupported storage mode")),
            },
            patches,
            metadata: serde_json::from_value(node.clone())
                .map_err(|error| invalid(format!("Imported graph audio: {error}")))?,
        });
    }
    for node in &result {
        for target in node
            .patches
            .iter()
            .map(|(_, target)| target)
            .chain(node.reference.iter())
        {
            if !symbols.contains(target) {
                return Err(invalid(format!(
                    "Imported particle {} names missing node {target}",
                    node.symbol
                )));
            }
        }
    }
    let presentation = value
        .get("runtime_presentation")
        .map(|part| presentation(part, &result))
        .transpose()?;
    let hud = value
        .get("hud_statuses")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(|error| invalid(format!("Imported HUD bindings: {error}")))?
        .unwrap_or_default();
    let assets = ImportedParticles {
        hud,
        nodes: result,
        source_trajectory: value["projectile"]["conversion"]["launch_policy"]
            == "source_player_trajectory",
        retained_trajectory: value["projectile"]["conversion"]["launch_policy"]
            == "retained_native_trajectory",
        projectile,
        perk_projectiles,
        attachments,
        presentation,
        sword: super::sword_profiles::Imported::load(value)?,
    };
    if let Some(root) = &assets.projectile {
        assets.projectile(root)?;
    }
    for root in &assets.perk_projectiles {
        assets.perk_projectile(root)?;
    }
    for root in &assets.attachments {
        assets.attachment(root)?;
    }
    if let Some(sword) = &assets.sword {
        sword.validate(&assets.nodes)?;
    }
    Ok((!assets.nodes.is_empty()).then_some(assets))
}

impl ImportedParticles {
    pub(in crate::item) fn sword_patches(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        symbols: &BTreeMap<String, TagHash>,
        keyed_override: bool,
    ) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
        let Some(sword) = &self.sword else {
            return Ok(Vec::new());
        };
        if keyed_override {
            return Err(invalid(
                "Imported sword mechanics already own the attack profiles",
            ));
        }
        sword.patches(manager, entity, symbols)
    }

    /// Include older graphs' mutable arrays in the native loader's instance copy.
    fn repair_instances(&mut self, manager: &PackageManager) -> AuthoringResult<()> {
        for node in &mut self.nodes {
            if manager
                .get_entry(TagHash(node.template))
                .is_none_or(|entry| entry.reference != 0x80809c36)
            {
                continue;
            }
            let patches = node
                .patches
                .iter()
                .map(|(offset, symbol)| serde_json::json!({"offset":offset,"symbol":symbol}))
                .collect::<Vec<_>>();
            if let Some(fixed) =
                parhelion_import::tiger::instance::upgrade_owner(&node.payload, &patches).map_err(
                    |error| invalid(format!("Imported component {}: {error:#}", node.symbol)),
                )?
            {
                node.payload = fixed.payload;
                node.patches = fixed
                    .patches
                    .into_iter()
                    .map(|patch| {
                        Ok((
                            usize::try_from(
                                patch["offset"]
                                    .as_u64()
                                    .ok_or_else(|| invalid("Repaired component offset missing"))?,
                            )
                            .map_err(|_| invalid("Repaired component offset overflow"))?,
                            patch["symbol"]
                                .as_str()
                                .ok_or_else(|| invalid("Repaired component symbol missing"))?
                                .to_owned(),
                        ))
                    })
                    .collect::<AuthoringResult<_>>()?;
            }
        }
        Ok(())
    }
    /// Apply prepared components only to the exact native vehicle they were retargeted for.
    pub(in crate::item) fn apply_presentation(
        &self,
        manager: &PackageManager,
        source_entity: TagHash,
        entity: &mut Vec<u8>,
        symbols: &BTreeMap<String, TagHash>,
    ) -> AuthoringResult<Option<TagHash>> {
        let Some(presentation) = &self.presentation else {
            return Ok(None);
        };
        if source_entity.0 != presentation.entity {
            return Err(invalid(
                "Imported appearance was prepared for a different summoned vehicle. Reimport it for this vehicle.",
            ));
        }
        if let Some(root) = &presentation.root {
            let node = self
                .nodes
                .iter()
                .find(|node| node.symbol == *root)
                .ok_or_else(|| invalid("Imported vehicle runtime root is absent"))?;
            *entity = node.payload.clone();
            for (offset, target) in &node.patches {
                let tag = symbols
                    .get(target)
                    .ok_or_else(|| invalid("Imported vehicle dependency was not allocated"))?;
                entity
                    .get_mut(*offset..offset + 4)
                    .ok_or_else(|| invalid("Imported vehicle reference exceeds its root"))?
                    .copy_from_slice(&tag.0.to_le_bytes());
            }
            return symbols
                .get(root)
                .copied()
                .map(Some)
                .ok_or_else(|| invalid("Imported vehicle root was not allocated"));
        }
        for component in &presentation.components {
            self.apply_component(manager, entity, symbols, component)?;
        }
        Ok(None)
    }

    fn apply_component(
        &self,
        manager: &PackageManager,
        entity: &mut [u8],
        symbols: &BTreeMap<String, TagHash>,
        component: &PresentationComponent,
    ) -> AuthoringResult<()> {
        let original = manager
            .read_tag(TagHash(component.source_owner))
            .map_err(|e| invalid(e.to_string()))?;
        let original = parhelion_import::tiger::payload::Payload(original);
        let resource = original.pointer(24).map_err(|e| invalid(e.to_string()))?;
        if manager
            .get_entry(TagHash(component.source_owner))
            .is_none_or(|e| e.reference != 0x80809c36)
            || original
                .u32(
                    resource
                        .checked_sub(4)
                        .ok_or_else(|| invalid("Invalid runtime component resource"))?,
                )
                .map_err(|e| invalid(e.to_string()))?
                != component.class
        {
            return Err(invalid(
                "Imported runtime owner does not match its native component class",
            ));
        }
        let node = self
            .nodes
            .iter()
            .find(|node| node.symbol == component.owner)
            .ok_or_else(|| invalid("Imported runtime component payload is absent"))?;
        let replacement = parhelion_import::tiger::payload::Payload(node.payload.clone());
        let resource = replacement
            .pointer(24)
            .map_err(|e| invalid(e.to_string()))?;
        if replacement
            .u32(
                resource
                    .checked_sub(4)
                    .ok_or_else(|| invalid("Invalid imported runtime resource"))?,
            )
            .map_err(|e| invalid(e.to_string()))?
            != component.class
        {
            return Err(invalid(
                "Imported runtime payload changes its component class",
            ));
        }
        let before = parhelion_import::tiger::payload::Payload(entity.to_vec());
        parhelion_import::tiger::entity::owner_slots(&before, &original, component.source_owner)
            .map_err(|e| invalid(e.to_string()))?;
        let target = symbols
            .get(&component.owner)
            .ok_or_else(|| invalid("Imported runtime owner was not allocated"))?;
        sundial::package_authoring::entity::retarget_weapon_component_owner(
            entity,
            component.source_owner,
            target.0,
        )
        .map_err(invalid)?;
        parhelion_import::tiger::entity::reject_stale_owner(
            &parhelion_import::tiger::payload::Payload(entity.to_vec()),
            component.source_owner,
        )
        .map_err(|e| invalid(e.to_string()))?;
        Ok(())
    }

    /// A perk may bind only a declared attachment root from this group.
    pub(in crate::item) fn attachment(&self, symbol: &str) -> AuthoringResult<()> {
        let node = self
            .nodes
            .iter()
            .find(|node| node.symbol == symbol)
            .filter(|_| self.attachments.contains(symbol))
            .ok_or_else(|| {
                invalid(format!(
                    "Imported attachment {symbol} is not a declared root"
                ))
            })?;
        if node.reference.is_some() {
            return Err(invalid(
                "An imported attachment root cannot replace its entity class",
            ));
        }
        // A direct damage effect binds a group whose root is its private outgoing profile.
        if profile_root(&node.payload) {
            return Ok(());
        }
        let (count, _, _, class) = array_at(&node.payload, 16)?;
        if count == 0
            || class != 0x80809C04
            || read_u64(&node.payload, 0)? != node.payload.len() as u64
        {
            return Err(invalid(
                "Imported attachment root is not a complete native entity",
            ));
        }
        Ok(())
    }

    /// An equipped import must own a native ability bank in the donor's selection bucket.
    /// The node's storage template is only a format witness. Its prepared bytes carry behavior.
    pub(in crate::item) fn ability(
        &self,
        manager: &PackageManager,
        symbol: &str,
        source: TagHash,
        retained_keys: &[u32],
    ) -> AuthoringResult<()> {
        use sundial::package_authoring::{ability_bank, ability_modifier};
        self.attachment(symbol)?;
        let source = manager
            .read_tag(source)
            .map_err(|error| invalid(error.to_string()))?;
        let slot = ability_modifier::entity_bank(&source)
            .map_err(invalid)?
            .and_then(ability_modifier::bank_slot)
            .ok_or_else(|| invalid("The native ability donor has no qualified selection bucket"))?;
        let root = self
            .nodes
            .iter()
            .find(|node| node.symbol == symbol)
            .ok_or_else(|| invalid("Imported ability root is missing"))?;
        let (count, _, first, _) = array_at(&root.payload, 16)?;
        let mut banks = BTreeSet::new();
        for index in 0..count {
            let at = first + index * 12;
            let owner = read_u32(&root.payload, at)?;
            let target = root
                .patches
                .iter()
                .find(|(offset, _)| *offset == at)
                .and_then(|(_, symbol)| self.nodes.iter().find(|node| node.symbol == *symbol));
            let template = target.map_or(owner, |node| node.template);
            if !ability_modifier::is_bank(template) {
                continue;
            }
            let bank =
                target.ok_or_else(|| invalid("An imported ability must own its prepared bank"))?;
            if bank.reference.is_some() || ability_modifier::bank_slot(template) != Some(slot) {
                return Err(invalid(
                    "The imported ability bank does not match its selection bucket",
                ));
            }
            ability_bank::validate_native_owner(&bank.payload).map_err(invalid)?;
            banks.insert(bank.symbol.as_str());
            if !retained_keys.is_empty() {
                let keys = ability_bank::property_rows(&bank.payload)
                    .map_err(invalid)?
                    .into_iter()
                    .map(|row| row.key)
                    .collect::<BTreeSet<_>>();
                if retained_keys.iter().any(|key| !keys.contains(key)) {
                    return Err(invalid(
                        "The imported ability bank lacks a selected property",
                    ));
                }
            }
        }
        if banks.len() != 1 {
            return Err(invalid(
                "An imported ability needs exactly one private bank in its selection bucket",
            ));
        }
        Ok(())
    }

    pub(in crate::item) fn merge(&mut self, other: Self) -> AuthoringResult<()> {
        if self.presentation.is_some() && other.presentation.is_some() {
            return Err(invalid(
                "More than one imported group supplies the vehicle appearance",
            ));
        }
        if self.projectile.is_some() && other.projectile.is_some() {
            return Err(invalid(
                "More than one imported group supplies the fired projectile",
            ));
        }
        if self.sword.is_some() && other.sword.is_some() {
            return Err(invalid(
                "More than one imported group supplies sword mechanics",
            ));
        }
        // Groups may carry the same converted node, as a weapon's audio and a perk explosion's
        // audio share banks. Identical nodes become one. A symbol with other contents refuses.
        let existing = self
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.symbol.clone(), index))
            .collect::<BTreeMap<_, _>>();
        let mut added = Vec::new();
        for node in other.nodes {
            match existing.get(&node.symbol).map(|&index| &self.nodes[index]) {
                Some(kept)
                    if kept.template == node.template
                        && kept.payload == node.payload
                        && kept.reference == node.reference
                        && kept.storage == node.storage
                        && kept.patches == node.patches => {}
                Some(_) => {
                    return Err(invalid("Imported asset groups contain duplicate symbols"));
                }
                None => added.push(node),
            }
        }
        self.nodes.extend(added);
        self.hud.extend(other.hud);
        self.attachments.extend(other.attachments);
        self.source_trajectory |= other.source_trajectory;
        self.retained_trajectory |= other.retained_trajectory;
        self.projectile = self.projectile.take().or(other.projectile);
        self.perk_projectiles.extend(other.perk_projectiles);
        self.presentation = self.presentation.take().or(other.presentation);
        self.sword = self.sword.take().or(other.sword);
        Ok(())
    }

    /// Validate the declared root independently of its eventual package address.
    pub(in crate::item) fn projectile(&self, symbol: &str) -> AuthoringResult<&[u8]> {
        if self.projectile.as_deref() != Some(symbol) {
            return Err(invalid(format!(
                "Imported projectile {symbol} is not the graph's declared projectile root"
            )));
        }
        self.projectile_root(symbol)
    }

    /// A projectile root a private perk of the weapon may name in place of a native projectile:
    /// the fired root or one of the graph's additional projectile roots.
    pub(in crate::item) fn perk_projectile(&self, symbol: &str) -> AuthoringResult<&[u8]> {
        if self.projectile.as_deref() != Some(symbol) && !self.perk_projectiles.contains(symbol) {
            return Err(invalid(format!(
                "Imported projectile {symbol} is not one of the graph's declared projectile roots"
            )));
        }
        self.projectile_root(symbol)
    }

    /// The group's fired projectile root, whose launch policy its perk projectiles share.
    pub(in crate::item) fn fired_projectile(&self) -> Option<&str> {
        self.projectile.as_deref()
    }

    fn projectile_root(&self, symbol: &str) -> AuthoringResult<&[u8]> {
        let node = self
            .nodes
            .iter()
            .find(|node| node.symbol == symbol)
            .ok_or_else(|| invalid("Imported projectile root is absent from its asset group"))?;
        if node.reference.is_some()
            || sundial::package_authoring::sandbox_perk::entity::kind(&node.payload)
                .map_err(invalid)?
                != Some(sundial::package_authoring::sandbox_perk::entity::Kind::Projectile)
        {
            return Err(invalid(
                "Imported projectile root is not a native projectile entity",
            ));
        }
        // An unreplicated root needs an unreplicated template or no network component, checked
        // when authored.
        if !replication::absent(node)? {
            replication::validate(&self.nodes, node)?;
        }
        Ok(&node.payload)
    }

    /// Rows in the projectile root's one trajectory pool, or `None` when its movement is absent,
    /// repeated or of another layout. Typed self-references are still symbolic here.
    pub(in crate::item) fn trajectory_capacity(&self, symbol: &str) -> Option<usize> {
        let root = self.nodes.iter().find(|node| node.symbol == symbol)?;
        let bindings = sundial::package_authoring::entity::weapon_component_bindings(
            &root.payload,
            0x0437_756D,
        )
        .ok()?;
        let [binding] = bindings.as_slice() else {
            return None;
        };
        let (_, _, rows, class) = array_at(&root.payload, 16).ok()?;
        if binding.concrete_class != 0x8080_3B73 || class != 0x8080_9C04 {
            return None;
        }
        let owner =
            replication::target(&self.nodes, root, rows + binding.component_index * 12).ok()?;
        let instance = usize::try_from(binding.resource_offset).ok()?;
        sundial::package_authoring::entity::projectile_trajectory_capacity(
            &owner.payload,
            u32::MAX,
            instance,
        )
        .ok()
    }

    /// Reserve and append the group, including its replication loading companion.
    /// Return the node symbols and the resources needed by the global runtime root.
    pub(in crate::item) fn author(
        &self,
        manager: &PackageManager,
        (packages, shared): (
            &mut crate::asset_packages::AssetPackages,
            &mut super::assets::Shared,
        ),
        item: u32,
    ) -> AuthoringResult<(BTreeMap<String, TagHash>, Vec<TagHash>)> {
        if !self.hud.is_empty() {
            return Err(invalid(
                "Imported HUD displays must be bound before asset allocation",
            ));
        }
        let runtime = self
            .presentation
            .as_ref()
            .and_then(|presentation| presentation.root.as_ref());
        let roots = self
            .projectile
            .iter()
            .chain(&self.perk_projectiles)
            .chain(runtime)
            .map(String::as_str)
            .chain(
                self.sword
                    .as_ref()
                    .map(super::sword_profiles::Imported::root),
            )
            .collect::<Vec<_>>();
        // A profile root is a component owner, not an entity.
        let entities = roots
            .iter()
            .map(|root| (*root).to_owned())
            .chain(
                self.attachments
                    .iter()
                    .filter(|root| {
                        self.nodes
                            .iter()
                            .find(|node| node.symbol == **root)
                            .is_none_or(|node| !profile_root(&node.payload))
                    })
                    .cloned(),
            )
            .collect();
        super::assets::author(
            &self.nodes,
            &roots,
            &entities,
            manager,
            packages,
            // Audio binding names each bank, medium and event after its own node.
            (|node: &Node| node.metadata.is_empty(), shared),
            |symbols| {
                let mut bound = audio::bind(
                    &self.nodes.iter().map(audio::Node::from).collect::<Vec<_>>(),
                    symbols,
                    &format!("parhelion/{item:08x}/graph-audio"),
                )?;
                shard::bind(manager, &self.nodes, item, &mut bound)?;
                Ok(bound)
            },
            |node, payload, entity| {
                if entity {
                    parhelion_import::tiger::entity::links::Graph::read_native(
                        &parhelion_import::tiger::payload::Payload(payload.to_vec()),
                    )
                    .map_err(|error| {
                        invalid(format!("Imported entity {}: {error}", node.symbol))
                    })?;
                }
                Ok(())
            },
        )
    }
}

/// Whether a payload is a complete native outgoing profile owner, whose definition is class
/// `80804B02`. An imported group translated for a direct damage effect has one as its root, and
/// the effect's profile word names it in place of a stock profile.
fn profile_root(payload: &[u8]) -> bool {
    let check = || -> AuthoringResult<bool> {
        let definition = crate::tag_payload::relative_target(payload, 24)?;
        Ok(read_u64(payload, 0)? == payload.len() as u64
            && definition >= 4
            && read_u32(payload, definition - 4)? == 0x8080_4B02)
    };
    check().unwrap_or(false)
}

/// The fired graph's particle slots as checked owner patches naming the authored systems.
pub(in crate::item) fn fired_graph_patches(
    fired: &crate::weapon::behavior::FiredGraph,
    symbols: &BTreeMap<String, TagHash>,
) -> AuthoringResult<Vec<sundial::package_authoring::sandbox_perk::program::NativeAssetResourcePatch>>
{
    fired
        .particles
        .iter()
        .map(|slot| {
            let tag = symbols.get(&slot.system).ok_or_else(|| {
                invalid(format!(
                    "Fired graph particle {} is not among the imported particles",
                    slot.system
                ))
            })?;
            Ok(
                sundial::package_authoring::sandbox_perk::program::NativeAssetResourcePatch {
                    binding_hash: slot.binding_hash,
                    resource_index: slot.resource_index,
                    offset: slot.offset,
                    expected: slot.expected.to_le_bytes().to_vec(),
                    bytes: tag.0.to_le_bytes().to_vec(),
                    imported_particle: None,
                },
            )
        })
        .collect()
}
