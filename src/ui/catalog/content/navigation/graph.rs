//! Bounded, cycle-safe relationship diagram using the same links as the list.
use super::*;
mod layout;

pub(super) struct View {
    pub graph: bool,
    depth: usize,
    cached: Option<(usize, Neighborhood)>,
    center: bool,
    metrics: Option<(u32, u32)>,
}
impl Default for View {
    fn default() -> Self {
        Self {
            graph: false,
            depth: 1,
            cached: None,
            center: true,
            metrics: None,
        }
    }
}

struct Neighborhood {
    nodes: BTreeMap<u32, usize>,
    edges: BTreeSet<(u32, u32)>,
    hidden: usize,
    types: BTreeMap<u32, u32>,
}

fn neighborhood(index: &tft::Index, root: u32, depth: usize) -> Neighborhood {
    let mut nodes = BTreeMap::from([(root, 0)]);
    let mut omitted = BTreeSet::new();
    for level in 0..depth {
        let frontier = nodes
            .iter()
            .filter_map(|(&tag, &distance)| (distance == level).then_some(tag))
            .collect::<BTreeSet<_>>();
        for link in &index.references {
            let neighbor = if frontier.contains(&link.source) {
                Some(link.target)
            } else if frontier.contains(&link.target) {
                Some(link.source)
            } else {
                None
            };
            if let Some(tag) = neighbor
                && !nodes.contains_key(&tag)
            {
                if nodes.len() < 40 {
                    nodes.insert(tag, level + 1);
                } else {
                    omitted.insert(tag);
                }
            }
        }
    }
    let mut edges = BTreeSet::new();
    for link in &index.references {
        let source = nodes.contains_key(&link.source);
        let target = nodes.contains_key(&link.target);
        if source && target {
            edges.insert((link.source, link.target));
        } else if source {
            omitted.insert(link.target);
        } else if target {
            omitted.insert(link.source);
        }
    }
    let mut types = BTreeMap::new();
    for link in &index.references {
        for (tag, class) in [
            (link.source, link.source_class),
            (link.target, link.target_class),
        ] {
            if nodes.contains_key(&tag) && crate::weapon_runtime::native_type_name(class).is_some()
            {
                types.entry(tag).or_insert(class);
            }
        }
    }
    Neighborhood {
        types,
        nodes,
        edges,
        hidden: omitted.len(),
    }
}

impl View {
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        index: &tft::Index,
        names: &BTreeMap<u32, Vec<String>>,
        root: u32,
    ) -> Option<Destination> {
        ui.horizontal_wrapped(|ui| {
            ui.label("Connection Depth");
            if ui.add(egui::Slider::new(&mut self.depth, 1..=3)).changed() {
                self.center = true;
            }
            if ui.button("Center on Selection").clicked() {
                self.center = true;
            }
        });
        if self
            .cached
            .as_ref()
            .is_none_or(|(depth, _)| *depth != self.depth)
        {
            self.cached = Some((self.depth, neighborhood(index, root, self.depth)));
        }
        let Neighborhood {
            nodes,
            edges,
            hidden,
            types,
        } = &self.cached.as_ref().expect("graph cache populated").1;
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{} {} · {} {}", nodes.len(), if nodes.len() == 1 { "resource" } else { "resources" }, edges.len(), if edges.len() == 1 { "link" } else { "links" }));
            if *hidden > 0 { ui.label(format!("{hidden} neighboring resources hidden")).on_hover_text("Increase the connection depth to see more. Each graph shows at most 40 resources."); }
        });
        ui.label("Resolved TFT links. Arrows point to the referenced asset.");
        let font_height = ui.text_style_height(&egui::TextStyle::Body);
        let metrics = (
            font_height.to_bits(),
            ui.available_width().round().to_bits(),
        );
        if self.metrics != Some(metrics) {
            self.center = true;
            self.metrics = Some(metrics);
        }
        let (positions, size) = layout::arrange(nodes, edges, root, font_height);
        let mut scroll = egui::ScrollArea::both()
            .id_salt("relationship-graph")
            .max_height(430.0);
        if self.center {
            let target = positions[&root].center();
            scroll = scroll
                .horizontal_scroll_offset((target.x - ui.available_width() * 0.5).max(0.0))
                .vertical_scroll_offset((target.y - 150.0).max(0.0));
            self.center = false;
        }
        let mut destination = None;
        scroll.show(ui, |ui| {
            let (canvas, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let rects = positions
                .iter()
                .map(|(&tag, rect)| (tag, rect.translate(canvas.min.to_vec2())))
                .collect::<BTreeMap<_, _>>();
            for &(source, target) in edges {
                let a = rects[&source];
                let b = rects[&target];
                let stroke = egui::Stroke::new(1.5, ui.visuals().text_color());
                if source == target {
                    let start = a.right_center();
                    let bend = start + egui::vec2(18.0, -40.0);
                    ui.painter().line_segment([start, bend], stroke);
                    ui.painter().arrow(bend, a.center_top() - bend, stroke);
                } else {
                    let (start, end) = if a.center().x < b.center().x {
                        (a.right_center(), b.left_center())
                    } else if a.center().x > b.center().x {
                        (a.left_center(), b.right_center())
                    } else {
                        (a.center_bottom(), b.center_top())
                    };
                    ui.painter().arrow(start, end - start, stroke);
                }
            }
            for (&tag, rect) in &rects {
                let name = reference::resource_name(names, tag)
                    .map(tft::asset_label)
                    .unwrap_or_else(|| "Unnamed Resource".into());
                let role = types
                    .get(&tag)
                    .map_or("Type Not Identified", |class| reference::type_name(*class));
                let title = format!(
                    "{}\n{}\n0x{tag:08X}",
                    node_line(ui, &name, rect.width() - 20.0),
                    node_line(ui, role, rect.width() - 20.0)
                );
                if tag == root {
                    ui.painter().text(
                        rect.center_top() - egui::vec2(0.0, 6.0),
                        egui::Align2::CENTER_BOTTOM,
                        "Selected Resource",
                        egui::TextStyle::Body.resolve(ui.style()),
                        ui.visuals().text_color(),
                    );
                }
                let response = ui
                    .push_id(tag, |ui| {
                        ui.put(
                            *rect,
                            egui::Button::new(
                                egui::RichText::new(&title)
                                    .font(egui::TextStyle::Body.resolve(ui.style())),
                            )
                            .wrap()
                            .selected(tag == root),
                        )
                    })
                    .inner;
                if response
                    .on_hover_text(format!("Open Resource\n{name}\n{role}\n0x{tag:08X}"))
                    .clicked()
                {
                    destination = Some(Destination::Resource(tag));
                }
            }
        });
        destination
    }
}

fn node_line(ui: &egui::Ui, text: &str, width: f32) -> String {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ui.fonts(|fonts| fonts.layout_job(job))
        .rows
        .first()
        .map_or_else(String::new, |row| row.text())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn link(source: u32, target: u32) -> tft::Reference {
        tft::Reference {
            source,
            target,
            source_class: 0,
            target_class: 0,
            offset: 0,
            path: String::new(),
        }
    }
    #[test]
    fn cycles_shared_nodes_and_direction_are_preserved() {
        let index = tft::Index {
            references: vec![link(1, 2), link(2, 3), link(3, 1), link(4, 2), link(1, 2)],
            ..Default::default()
        };
        let Neighborhood {
            nodes,
            edges,
            hidden,
            ..
        } = neighborhood(&index, 1, 2);
        assert_eq!(nodes.len(), 4);
        assert_eq!(edges.len(), 4);
        assert!(edges.contains(&(3, 1)));
        assert_eq!(hidden, 0);
    }
    #[test]
    fn expansion_is_bounded_and_reports_hidden_neighbors() {
        let index = tft::Index {
            references: (2..102).map(|tag| link(1, tag)).collect(),
            ..Default::default()
        };
        let Neighborhood { nodes, hidden, .. } = neighborhood(&index, 1, 3);
        assert_eq!(nodes.len(), 40);
        assert_eq!(hidden, 61);
    }
}
