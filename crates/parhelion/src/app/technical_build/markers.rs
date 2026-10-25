//! Marker report formatting and asynchronous native appearance reads.

use super::*;
use std::fmt::Write as _;

/// The named points the appearance's gear art carries: where the weapon is held, where it
/// fires, where a case leaves it. The runtime resolves these by name, so an imported model that
/// kept a donor's marker set aims at the donor's sight rather than its own. Seeing the names and
/// positions side by side is what makes that visible before a build reaches the game.
///
/// Read from the packages rather than the recipe, so it is rendered separately and only while
/// the window is open.
///
/// A marker the recipe moves is listed where the build puts it, with how far it moved.
pub(super) fn marker_section(markers: Markers<'_>, moved: &[MarkerOffsetRecipe]) -> String {
    let mut text = String::new();
    let out = &mut text;
    let sets = match markers {
        None => {
            let _ = writeln!(out, "\nMARKERS  not read");
            return text;
        }
        Some(Err(error)) => {
            let _ = writeln!(out, "\nMARKERS  unavailable: {error}");
            return text;
        }
        Some(Ok(sets)) => sets,
    };
    let all = || sets.iter().flat_map(|set| &set.markers);
    let total = all().count();
    let named = all()
        .filter(|marker| marker_name(marker.name).is_some())
        .count();
    let offset = |name: u32| {
        moved
            .iter()
            .find(|offset| offset.marker.parse_u32().ok() == Some(name))
            .map(|offset| offset.offset_um.map(|um| um as f32 / 1_000_000.0))
    };
    let shifted = all().filter(|marker| offset(marker.name).is_some()).count();
    let _ = writeln!(
        out,
        "\nMARKERS  {total} on {} objects, {named} named, {shifted} moved",
        sets.len()
    );
    if sets.is_empty() {
        field(out, "markers", "this appearance carries none");
        return text;
    }
    for set in sets {
        let _ = writeln!(
            out,
            "  object {}  set {}  ({})",
            hex(set.entity),
            hex(set.component),
            set.markers.len()
        );
        for marker in &set.markers {
            let moved = offset(marker.name);
            let [x, y, z] = moved.map_or(marker.position, |delta| {
                std::array::from_fn(|axis| marker.position[axis] + delta[axis])
            });
            let moved = moved.map_or_else(String::new, |[dx, dy, dz]| {
                format!("  moved {dx:+.5} {dy:+.5} {dz:+.5}")
            });
            let label = column(&marker.label(), 24);
            // A weapon really does carry two markers of one name at one point, differing only
            // in which way they face, so the rotation has to be shown or they read as a bug.
            let facing = if marker.is_aligned() {
                String::new()
            } else {
                let [i, j, k, w] = marker.orientation;
                format!("  facing {i:>7.4} {j:>7.4} {k:>7.4} {w:>7.4}")
            };
            // A marker whose name is unrecovered is still somewhere meaningful. Naming what it
            // sits next to says more than the hash alone, without guessing at the name.
            let near = gear_markers::nearest_named(sets, marker)
                .map_or_else(String::new, |near| format!("  {near}"));
            let _ = writeln!(
                out,
                "    {label:<24}  {x:>10.5} {y:>10.5} {z:>10.5}{moved}{facing}{near}"
            );
        }
    }
    text
}

/// The packages read that backs the MARKERS section. Held so its package handles can be
/// released before an installation, the same rule the runtime-graph job follows.
pub(in super::super) struct MarkerJob {
    arrangements: Vec<u16>,
    receiver: std::sync::mpsc::Receiver<Result<Vec<MarkerSet>, String>>,
    worker: std::thread::JoinHandle<()>,
}

impl crate::app::PackageAuthoringApp {
    pub(in super::super) fn technical_markers_busy(&self) -> bool {
        self.technical_marker_job.is_some()
    }

    /// The arrangements the report describes: the recipe's override when it has one, else the
    /// geometry donor's. `None` while neither is known. Markers belong to the art, so this is the only
    /// thing the read depends on.
    pub(in super::super) fn technical_marker_arrangements(
        &self,
        donor: Option<&WeaponDonor>,
    ) -> Option<Vec<u16>> {
        let mut rows: Vec<u16> = match self.recipe.overrides.art_arrangements.as_ref() {
            Some(rows) => rows.iter().map(|row| row.arrangement).collect(),
            None => donor?
                .art_arrangements
                .iter()
                .map(|row| row.arrangement)
                .collect(),
        };
        rows.sort_unstable();
        rows.dedup();
        Some(rows)
    }

    /// Starts the read when the arrangement on screen has no result yet. Only called while the
    /// window is open, so a closed window costs nothing. One read runs at a time: a replaced
    /// job would leave its thread holding package handles. The poll keeps a stale result and
    /// the next frame starts the read for the new arrangement.
    pub(in super::super) fn ensure_technical_markers(
        &mut self,
        ctx: &egui::Context,
        arrangements: &[u16],
    ) {
        if self.technical_marker_job.is_some()
            || self
                .technical_markers
                .as_ref()
                .is_some_and(|(read, _)| read == arrangements)
        {
            return;
        }
        if arrangements.is_empty() {
            self.technical_markers = Some((Vec::new(), Ok(Vec::new())));
            self.technical_marker_revision = self.technical_marker_revision.wrapping_add(1);
            return;
        }
        let packages = self.packages.clone();
        let wanted = arrangements.to_vec();
        let (sender, receiver) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let worker = {
            let wanted = wanted.clone();
            std::thread::spawn(move || {
                let mut sets = Vec::new();
                let mut result = Ok(());
                for arrangement in wanted {
                    match sundial::package_authoring::gear_markers::read_appearance(
                        &packages,
                        arrangement,
                    ) {
                        Ok(read) => sets.extend(read),
                        Err(error) => {
                            result = Err(format!("arrangement {arrangement}: {error}"));
                            break;
                        }
                    }
                }
                let _ = sender.send(result.map(|()| sets));
                ctx.request_repaint();
            })
        };
        self.technical_marker_job = Some(MarkerJob {
            arrangements: wanted,
            receiver,
            worker,
        });
    }

    pub(in super::super) fn poll_technical_markers(&mut self) {
        let Some(job) = &self.technical_marker_job else {
            return;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("The marker reader stopped without a result".to_owned())
            }
        };
        let MarkerJob {
            arrangements,
            worker,
            ..
        } = self.technical_marker_job.take().expect("job was checked");
        // Join even a stale read: its package handles must be released before installation.
        let completion = worker.join();
        self.technical_markers = Some((
            arrangements,
            if completion.is_err() {
                Err("The marker reader panicked while reading packages".to_owned())
            } else {
                result
            },
        ));
        self.technical_marker_revision = self.technical_marker_revision.wrapping_add(1);
    }
}
