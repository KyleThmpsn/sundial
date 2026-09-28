//! A stock effect carrying overrides edits in place once the checked conversion has folded
//! them into a program. The conversion reads packages, so it runs off the frame, one at a time.
use super::*;
use sundial::package_authoring::sandbox_perk::program::Program;

pub(super) struct Prepared {
    document: String,
    source: WeaponSandboxPerkRuntimeRecipe,
    state: State,
}

enum State {
    Reading(thread::JoinHandle<Result<Program, String>>),
    Ready(Result<Box<Program>, String>),
}

pub(super) enum Preparation {
    Reading,
    Ready(Box<Program>),
    /// No exact program carries these overrides, so the card stays a reading.
    Unavailable,
}

impl Workbench {
    /// Collects finished conversions. It runs every frame, whether or not the card that asked
    /// is drawn: a conversion that finished while its document was not selected used to stay
    /// Reading, which kept the workbench busy and the window from closing.
    pub(super) fn poll_stock_programs(&mut self) {
        for prepared in &mut self.stock_programs {
            if matches!(&prepared.state, State::Reading(worker) if worker.is_finished())
                && let State::Reading(worker) =
                    std::mem::replace(&mut prepared.state, State::Ready(Err(String::new())))
            {
                prepared.state = State::Ready(
                    worker
                        .join()
                        .unwrap_or_else(|_| Err("The conversion stopped.".into()))
                        .map(Box::new),
                );
            }
        }
    }

    /// Drops every conversion, finished or not, when the packages they read change.
    pub(super) fn clear_stock_programs(&mut self) {
        self.stock_programs.clear();
    }

    /// The program that reproduces `effect` with its overrides, starting the conversion when
    /// nothing is reading.
    pub(super) fn prepare_stock_effect(
        &mut self,
        packages: &Path,
        ctx: &egui::Context,
        document: &str,
        effect: &WeaponSandboxPerkRuntimeRecipe,
        name: &str,
    ) -> Preparation {
        self.poll_stock_programs();
        let reading = self
            .stock_programs
            .iter()
            .any(|prepared| matches!(prepared.state, State::Reading(_)));
        let current = self
            .stock_programs
            .iter()
            .find(|prepared| prepared.document == document && prepared.source == *effect);
        match current.map(|prepared| &prepared.state) {
            Some(State::Ready(Ok(program))) => {
                let mut program = program.clone();
                program.name = name.to_owned();
                return Preparation::Ready(program);
            }
            Some(State::Ready(Err(_))) => return Preparation::Unavailable,
            Some(State::Reading(_)) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return Preparation::Reading;
            }
            None if reading => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return Preparation::Reading;
            }
            None => {}
        }
        let packages = packages.to_owned();
        let snapshot = effect.clone();
        let name = name.to_owned();
        let repaint = ctx.clone();
        self.stock_programs.push(Prepared {
            document: document.to_owned(),
            source: effect.clone(),
            state: State::Reading(thread::spawn(move || {
                let result = parameters::conversion::copy_effect(&packages, &snapshot, name);
                repaint.request_repaint();
                result
            })),
        });
        Preparation::Reading
    }

    /// Forgets conversions for effects this document no longer has in that form.
    pub(super) fn retain_stock_programs(&mut self, recipe: &PerkRecipe) {
        self.stock_programs.retain(|prepared| {
            prepared.document != recipe.id
                || matches!(prepared.state, State::Reading(_))
                || recipe.effects.contains(&prepared.source)
        });
    }

    pub(super) fn preparing_stock_effect(&self) -> bool {
        self.stock_programs
            .iter()
            .any(|prepared| matches!(prepared.state, State::Reading(_)))
    }
}
