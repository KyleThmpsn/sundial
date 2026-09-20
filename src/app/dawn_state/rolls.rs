use super::*;
use crate::persistence::dawn_account::SavedRoll;

impl SundialApp {
    pub(super) fn draw_saved_rolls(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = self.document.dawn_account() else {
            return;
        };
        let Some(character) = doc.characters().characters().get(self.selected_character) else {
            return;
        };
        let mut action = None;
        ui.set_max_width(760.0);
        let query_id = ui.make_persistent_id("roll-search");
        let mut query = ui
            .data(|d| d.get_temp::<String>(query_id))
            .unwrap_or_default();
        ui.add(
            egui::TextEdit::singleline(&mut query)
                .hint_text("Search items…")
                .desired_width(320.0),
        );
        ui.data_mut(|d| d.insert_temp(query_id, query.clone()));
        let mut count = 0;
        for item in character
            .equipment
            .values()
            .flatten()
            .chain(character.inventory.iter())
        {
            let hash = item.definition_hash.get();
            let soid = item.instance_soid.get();
            let name = self
                .manifest
                .display_name(u64::from(hash))
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Item {hash:08X}"));
            if !name.to_lowercase().contains(&query.to_lowercase()) {
                continue;
            }
            let saved = match doc.saved_roll(soid) {
                Ok(roll) => roll,
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    continue;
                }
            };
            let definition = self.manifest.item(u64::from(hash));
            if saved == SavedRoll::default()
                && definition.is_none_or(|d| {
                    d.sockets
                        .iter()
                        .all(|s| s.ordered_randomized_choices().is_empty())
                })
            {
                continue;
            }
            count += 1;
            egui::CollapsingHeader::new(name).id_salt(soid).show(ui,|ui| {
                item_label(ui,&self.manifest,hash);
                let Some(definition)=definition else {ui.weak("The installed socket definition is unavailable.");return;};
                let mut roll=saved.clone();
                if ui.button("Clear Saved Roll").on_hover_text("Clears ownership and roll bytes. Authored socket choices are unchanged.").clicked() { roll=SavedRoll::default(); }
                for (lane,socket) in definition.sockets.iter().take(12).enumerate() {
                    let pool=socket.ordered_randomized_choices();
                    if pool.is_empty() && roll.lanes&(1<<lane)==0 {continue;}
                    egui::CollapsingHeader::new(socket.display_label(lane)).id_salt((soid,lane)).show(ui,|ui| {
                        let authored=matches!(&item.plugs,sundial_account::ItemPlugs::Authored(p) if lane<p.len());
                        let mut rolled=roll.lanes&(1<<lane)!=0;
                        if ui.add_enabled(authored,egui::Checkbox::new(&mut rolled,"Rolled Socket")).on_disabled_hover_text("Author this socket in Items before enabling saved roll ownership.").changed() {
                            if rolled {roll.lanes|=1<<lane;} else {roll.lanes&=!(1<<lane);roll.owned[lane]=0;}
                        }
                        ui.add_enabled_ui(rolled && pool.len()<=64,|ui| {
                            for (row,hash) in pool.iter().take(64).enumerate() {
                                let mut owned=roll.owned[lane]&(1<<row)!=0;
                                let label=self.manifest.display_name(*hash).map(str::to_owned).unwrap_or_else(||format!("Plug {hash:08X}"));
                                if ui.checkbox(&mut owned,label).changed() {
                                    if owned {roll.owned[lane]|=1<<row;} else {roll.owned[lane]&=!(1<<row);}
                                }
                            }
                        });
                        if pool.is_empty() {ui.weak("The randomized pool is unavailable. Existing ownership is preserved.");}
                    });
                }
                egui::CollapsingHeader::new("Roll Bytes").id_salt((soid,"entropy")).show(ui,|ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (i,byte) in roll.entropy.iter_mut().enumerate() {
                            ui.push_id(i,|ui| {ui.add(egui::DragValue::new(byte).range(0..=255)).on_hover_text(format!("Entropy byte {}. Used by the client to select randomized socket rows.",i+1));});
                        }
                    });
                });
                if roll!=saved {action=Some((soid,hash,roll));}
            });
        }
        if count == 0 {
            ui.weak("No matching items with saved or randomized rolls.");
        }
        if ui.is_enabled()
            && let Some((soid, hash, roll)) = action
        {
            let result = self.document.dawn_account_mut().unwrap().set_saved_roll(
                self.selected_character,
                soid,
                roll,
                self.manifest.item(u64::from(hash)).unwrap(),
            );
            self.finish_dawn_edit(result, "Saved Roll Updated");
        }
    }
}
