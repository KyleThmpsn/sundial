//! Keep direct users before the selection and referenced assets after it.
use super::*;

pub(super) fn arrange(
    nodes: &BTreeMap<u32, usize>,
    edges: &BTreeSet<(u32, u32)>,
    root: u32,
    font: f32,
) -> (BTreeMap<u32, egui::Rect>, egui::Vec2) {
    let mut columns = BTreeMap::from([(root, 0_i32)]);
    for level in 1..=nodes.values().copied().max().unwrap_or(0) {
        for (&tag, &distance) in nodes {
            if distance != level {
                continue;
            }
            let side = edges
                .iter()
                .find_map(|&(source, target)| {
                    let (parent, direction) = if target == tag {
                        (source, 1)
                    } else if source == tag {
                        (target, -1)
                    } else {
                        return None;
                    };
                    if nodes[&parent] >= level {
                        return None;
                    }
                    columns.get(&parent).map(|&column| {
                        if column == 0 {
                            direction
                        } else {
                            column.signum()
                        }
                    })
                })
                .unwrap_or(1);
            columns.insert(tag, side * level as i32);
        }
    }
    let first = columns.values().copied().min().unwrap_or(0);
    let last = columns.values().copied().max().unwrap_or(0);
    let mut counts = BTreeMap::<i32, usize>::new();
    for column in columns.values() {
        *counts.entry(*column).or_default() += 1;
    }
    let rows = counts.values().copied().max().unwrap_or(1);
    let width = (font * 17.0).max(225.0);
    let height = font * 4.0 + 18.0;
    let stride = egui::vec2(width + 64.0, height + 48.0);
    let mut placed = BTreeMap::<i32, usize>::new();
    let rects = columns
        .iter()
        .map(|(&tag, &column)| {
            let row = placed.entry(column).or_default();
            let y = *row as f32 + (rows - counts[&column]) as f32 * 0.5;
            *row += 1;
            (
                tag,
                egui::Rect::from_min_size(
                    egui::pos2(
                        (column - first) as f32 * stride.x + 16.0,
                        y * stride.y + 36.0,
                    ),
                    egui::vec2(width, height),
                ),
            )
        })
        .collect();
    (
        rects,
        egui::vec2(
            (last - first + 1) as f32 * stride.x,
            rows as f32 * stride.y + 36.0,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_places_users_before_selection_and_scales_with_text() {
        let nodes = BTreeMap::from([(1, 1), (2, 0), (3, 1), (4, 2)]);
        let edges = BTreeSet::from([(1, 2), (2, 3), (3, 4), (4, 2)]);
        let (normal, _) = arrange(&nodes, &edges, 2, 14.0);
        let (large, _) = arrange(&nodes, &edges, 2, 22.0);
        assert!(normal[&1].right() < normal[&2].left());
        assert!(normal[&2].right() < normal[&3].left());
        assert!(large[&2].height() > normal[&2].height());
        for (tag, a) in &large {
            for (other, b) in &large {
                if tag != other {
                    assert!(!a.intersects(*b));
                }
            }
        }
    }
}
