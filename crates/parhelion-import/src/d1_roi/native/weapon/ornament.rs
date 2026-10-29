//! A weapon's talent grid ornaments. Each converts its own art on the weapon's native runtime, as
//! the weapon's art does, and becomes a private plug of the weapon's ornament socket, laid out as
//! [`crate::tiger::ornament::layout`] lays out imported appearances. An ornament carries art and
//! an icon, so the weapon keeps its animation, audio and gameplay.
use super::*;
use crate::tiger::ornament::{self, Appearance, Layout};

/// The weapon whose ornaments import.
pub(super) struct Owner<'a> {
    pub weapon: &'a Weapon,
    /// The weapon's source export, with its runtime and dyes.
    pub source: &'a Value,
    pub native: &'a Path,
    pub donor: u32,
    /// Other native weapons of the donor's type, which can lend a donor without an ornament
    /// socket their socket type.
    pub kin: &'a [u32],
    pub namespace: &'a str,
    /// The authored weapon.
    pub item: u32,
    /// The donor's art template, which every art row takes.
    pub template: u16,
    pub recipe: &'a Path,
    /// Each ornament converts into `<HASH>/graph` here, named by its source item, and the package
    /// reads that lay them out go in [`ornament::reads`] beside it.
    pub output: &'a Path,
}

/// One ornament's place in the weapon's ornament socket.
struct Choice<'a> {
    layout: &'a Layout,
    choice: usize,
    plug: u32,
}

impl Source {
    /// Convert `owner`'s ornaments and offer them in its recipe's ornament socket. An ornament
    /// that does not convert is left out with its reason and the next takes its choice. The
    /// weapon keeps no ornament column when none converts, and the report says why. Only a
    /// cancellation is an error.
    pub(super) fn import_weapon_ornaments(
        &self,
        owner: &Owner,
        options: &NativeOptions,
        progress: &mut dyn FnMut(String),
    ) -> Result<Value> {
        let mut report = json!({"omitted":[]});
        match self.convert_weapon_ornaments(owner, options, &mut report, progress) {
            Ok(true) => {}
            Ok(false) => {
                remove(owner.output);
                remove(&ornament::reads(owner.output));
            }
            Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
            Err(error) => {
                report["skipped"] = json!(format!("{error:#}"));
                remove(owner.output);
                remove(&ornament::reads(owner.output));
            }
        }
        Ok(report)
    }

    /// Convert and bind the ornaments, returning whether any was bound.
    fn convert_weapon_ornaments(
        &self,
        owner: &Owner,
        options: &NativeOptions,
        report: &mut Value,
        progress: &mut dyn FnMut(String),
    ) -> Result<bool> {
        let mut assets = BTreeMap::new();
        let ornaments = self.weapon_ornaments(owner.weapon, &mut assets)?;
        report["ornaments"] = json!(ornaments.len());
        if ornaments.is_empty() {
            return Ok(false);
        }
        let mut reader = Reader::discovery(
            owner.native,
            &ornament::reads(owner.output).join("native"),
            false,
        )?;
        let mut layout = ornament::layout(&mut reader, owner.donor, owner.kin, ornaments.len())?;
        let sockets = crate::tiger::shadowkeep::sockets(&mut reader, owner.donor)?;
        drop(reader);
        let recipe: Value = serde_json::from_slice(&fs::read(owner.recipe)?)?;
        layout.append_after(
            recipe["overrides"]["socket_columns"]
                .as_array()
                .map_or(0, Vec::len),
        )?;
        report["layout"] = layout.report.clone();
        let mut converted = Vec::new();
        let total = ornaments.len();
        for (index, (item, provenance)) in ornaments.iter().enumerate() {
            crate::cancellation::check()?;
            progress(format!(
                "Converting ornament {} of {total}: {}",
                index + 1,
                item.name
            ));
            let choice = Choice {
                layout: &layout,
                choice: converted.len() + 1,
                plug: layout.plugs[converted.len()],
            };
            let folder = owner.output.join(format!("{:08X}", item.hash));
            let graph = folder.join(crate::graph::GRAPH);
            match self.convert_weapon_ornament(
                owner,
                options,
                (item, provenance),
                &choice,
                &graph,
                &mut assets,
            ) {
                Ok(name) => converted.push((choice.plug, name, graph)),
                Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
                Err(error) => {
                    if let Some(omitted) = report["omitted"].as_array_mut() {
                        omitted.push(json!({"ornament":format!("{:08X}", item.hash),
                            "name":item.name,"reason":format!("{error:#}")}));
                    }
                    remove(&folder);
                }
            }
        }
        report["converted"] = converted
            .iter()
            .enumerate()
            .map(|(index, (_, name, _))| json!({"choice":index + 1,"name":name}))
            .collect();
        if converted.is_empty() {
            return Ok(false);
        }
        let appearances = converted
            .iter()
            .map(|(plug, name, graph)| Appearance {
                plug: *plug,
                name,
                graph,
            })
            .collect::<Vec<_>>();
        ornament::attach(owner.recipe, &layout, &sockets, &appearances)?;
        Ok(true)
    }

    /// Convert one ornament into its graph folder `folder`, returning the name its choice shows.
    fn convert_weapon_ornament(
        &self,
        owner: &Owner,
        options: &NativeOptions,
        (item, provenance): (&Weapon, &Value),
        choice: &Choice,
        folder: &Path,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<String> {
        let art = self.weapon_art(item, assets)?;
        // An ornament may override only some of the weapon's dye slots. The others and their
        // layer precedence stay.
        let mut dyes = owner.source["dyes"]
            .as_array()
            .context("Source weapon dyes")?
            .clone();
        dyes.extend(self.dyes(item.definition, assets)?);
        dyes.sort_by_key(|dye| match dye["layer"].as_str() {
            Some("default") => 0,
            Some("custom") => 1,
            Some("fixed") => 2,
            _ => 3,
        });
        let roles = ornament::roles(owner.namespace, choice.layout.lane, choice.choice);
        let target = ornament::fnv(&format!("{roles}/private-plug"));
        let mut assembly = Assembly::new(
            self,
            owner.native,
            folder,
            options,
            Some((&owner.source["runtime"], owner.donor)),
        )?;
        let (display, container) =
            assembly.ornament_display(owner.weapon.definition, provenance)?;
        // The talent grid names an ornament whose own strings leave it unnamed.
        let name = if super::super::super::localization::Labels::new(self)?
            .text(self, &self.read(item.strings)?, 0x78)?
            .is_empty()
            && !display.is_empty()
        {
            display
        } else {
            item.name.clone()
        };
        let mut parts = Vec::new();
        for (index, part) in art["parts"]
            .as_array()
            .context("Ornament art parts")?
            .iter()
            .enumerate()
        {
            let symbol = assembly.body(part, &dyes, &format!("part-{index}"))?;
            parts.push(json!({"source_assignment":part["assignment"],
                "key":key(&format!("parhelion/imported-gear/{target:08X}/art-{index}")),
                "parent":symbol}));
        }
        ensure!(!parts.is_empty(), "The ornament has no converted art");
        let mut rows = art["rows"].as_array().context("Ornament art rows")?.clone();
        for row in &mut rows {
            row["template_index"] = json!(owner.template);
        }
        let icon = match assembly.item_icon(item.strings)? {
            Some(icon) => Some(icon),
            None => assembly.container_icon(container)?,
        };
        let mut limitations = vec![
            "The ornament changes the weapon's art. Its animation, audio and gameplay stay the weapon's.".to_owned(),
        ];
        limitations.extend(assembly.approximations.iter().cloned());
        let graph = json!({"kind":"weapon","source":"d1_roi","source_item":item.hash,
            "source_name":name,"source_icon_png":icon,"ornament_icon_png":icon,
            "source_builds":self.report.builds,"source_stamp":self.report.stamp,
            "item_hash":target,"native_item":owner.donor,
            "converter_revision":crate::graph::CONVERTER_REVISION,
            "nodes":assembly.nodes,"parent":parts[0]["parent"],"dyes":[],"dye_rows":[[],[],[]],
            "gear_art":{"selector_format":"shadowkeep","rows":rows,"parts":parts},
            "source_ornament":provenance,"rendering":assembly.evidence,
            "ornament":{"target_weapon":owner.item,"target_socket":choice.layout.lane,
                "choice_index":choice.choice,"native_plug":choice.plug,
                "native_socket_type":choice.layout.socket_type,
                "plug_category":choice.layout.category,"name":name,"source_item":item.hash},
            "installable":true,"gameplay_verified":false,"limitations":limitations});
        crate::io::write_json(&folder.join("asset-graph.json"), &graph)?;
        assembly.native.finish()?;
        Ok(name)
    }
}

/// Removing unused conversions only saves space, so a failure is not an error.
fn remove(folder: &Path) {
    if folder.exists()
        && let Err(error) = fs::remove_dir_all(folder)
    {
        eprintln!(
            "Unused ornament conversion {} kept: {error}",
            folder.display()
        );
    }
}
