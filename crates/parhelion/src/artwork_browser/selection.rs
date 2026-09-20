use super::*;
use crate::presentation::Artwork;

impl Picker {
    pub fn artwork(
        &mut self,
        selection: Selection,
        packages: &Path,
        catalog: Option<&InvestmentCatalog>,
        ctx: &egui::Context,
    ) -> Receiver<Result<Option<Artwork>, String>> {
        let (sender, receiver) = mpsc::channel();
        let container = if let Selection::Perk(hash) = &selection {
            catalog.and_then(|c| c.weapon_icon_container(*hash))
        } else {
            None
        };
        let packages = packages.to_owned();
        let purpose = self.purpose;
        let repaint = ctx.clone();
        self.workers.push(thread::spawn(move || {
            let result = (|| {
                let pixels = match selection {
                    Selection::Local(path) => library::load(&path, purpose)?,
                    Selection::Icon(Icon::Image { image, .. }) => {
                        image.fit_to(purpose.svg_edge(), purpose.svg_edge())
                    }
                    other => {
                        let manager =
                            sundial::package_authoring::open_shadowkeep_package_manager(&packages)?;
                        let tag = match other {
                            Selection::Icon(Icon::Texture { tag }) => {
                                tiger_pkg::TagHash(tag.parse_u32().map_err(|e| e.to_string())?)
                            }
                            Selection::Perk(_) => {
                                let container = tiger_pkg::TagHash(
                                    container.ok_or("This perk has no readable icon")?,
                                );
                                package_icons::primary_texture(&manager, container)?
                            }
                            _ => unreachable!(),
                        };
                        let image = package_icons::load_for(&manager, tag, purpose)?;
                        let bytes = image
                            .pixels
                            .iter()
                            .flat_map(|p| p.to_srgba_unmultiplied())
                            .collect();
                        image::RgbaImage::from_raw(
                            image.size[0] as u32,
                            image.size[1] as u32,
                            bytes,
                        )
                        .ok_or("Invalid icon pixels")?
                    }
                };
                Artwork::from_source(pixels).map(Some)
            })();
            let _ = sender.send(result);
            repaint.request_repaint();
        }));
        receiver
    }
}
