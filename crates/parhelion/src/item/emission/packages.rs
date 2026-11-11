//! Planned package jobs with bounded concurrency and caller-thread progress reporting.
use super::*;
use crate::workflow::{BuildActivity, OperationStatus};
use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Condvar, Mutex, mpsc},
    time::Instant,
};

mod activity;

const MAX_WORKERS: usize = 4;
const WORKING_BUDGET: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct Ticket(usize);

pub(super) struct Packages<'a> {
    directory: &'a Path,
    jobs: Vec<Job>,
}

enum Kind {
    Overlay {
        replacements: Vec<ReplacementSpec>,
        tags: Vec<NewTagSpec>,
        references: Vec<crate::NewTagReferenceOverride>,
    },
    Standalone(crate::asset_packages::AssetPackage),
    /// A standalone package whose extended header region names tags.
    Named {
        tags: Vec<NewTagSpec>,
        named: Vec<crate::format::NamedTag>,
        references: Vec<crate::extend::NewTagReferenceOverride>,
    },
}

#[derive(Clone, Copy)]
enum Check {
    Overlay(usize),
    Host(usize),
    Private { start: usize, count: usize },
    Standalone(usize),
}

struct Job {
    id: u16,
    kind: Kind,
    check: Check,
    weight: usize,
}

enum Event {
    Activity {
        index: usize,
        label: String,
        timestamp: Instant,
        activity: BuildActivity,
        completed: bool,
    },
    Done(usize, AuthoringResult<crate::ExtendedOverlayArtifact>),
}

struct Queue {
    pending: VecDeque<(usize, Job)>,
    active: usize,
    bytes: usize,
    failed: bool,
}

impl<'a> Packages<'a> {
    pub(super) fn new(directory: &'a Path) -> Self {
        Self {
            directory,
            jobs: Vec::new(),
        }
    }

    pub(super) fn overlay(
        &mut self,
        id: u16,
        replacements: Vec<ReplacementSpec>,
        tags: Vec<NewTagSpec>,
        references: Vec<crate::NewTagReferenceOverride>,
    ) -> AuthoringResult<Ticket> {
        let count = tags.len();
        self.push(
            id,
            Kind::Overlay {
                replacements,
                tags,
                references,
            },
            Check::Overlay(count),
        )
    }

    /// An overlay that replaces tables of package `id` and adds no entries.
    pub(super) fn tables(
        &mut self,
        id: u16,
        replacements: Vec<ReplacementSpec>,
    ) -> AuthoringResult<Ticket> {
        self.overlay(id, replacements, Vec::new(), Vec::new())
    }

    pub(super) fn standalone(
        &mut self,
        package: crate::asset_packages::AssetPackage,
    ) -> AuthoringResult<Ticket> {
        let count = package.tags.len();
        self.push(
            package.id,
            Kind::Standalone(package),
            Check::Standalone(count),
        )
    }

    pub(super) fn named(
        &mut self,
        id: u16,
        tags: Vec<NewTagSpec>,
        named: Vec<crate::format::NamedTag>,
        references: Vec<crate::extend::NewTagReferenceOverride>,
    ) -> AuthoringResult<Ticket> {
        let count = tags.len();
        self.push(
            id,
            Kind::Named {
                tags,
                named,
                references,
            },
            Check::Standalone(count),
        )
    }

    pub(super) fn host(&mut self, ticket: Ticket, count: usize) {
        self.jobs[ticket.0].check = Check::Host(count);
    }

    pub(super) fn private(&mut self, ticket: Ticket, start: usize, count: usize) {
        self.jobs[ticket.0].check = Check::Private { start, count };
    }

    fn push(&mut self, id: u16, kind: Kind, check: Check) -> AuthoringResult<Ticket> {
        let profile = crate::package_profile::authored_package(id)
            .ok_or_else(|| invalid("An emitted package has no registered profile"))?;
        match &kind {
            Kind::Standalone(package) => crate::mod_family::build::record_package(
                id,
                profile.file_name,
                &package.tags,
                &package.references,
            ),
            Kind::Named {
                tags, references, ..
            } => crate::mod_family::build::record_package(id, profile.file_name, tags, references),
            Kind::Overlay { .. } => {}
        }
        let payload = match &kind {
            Kind::Overlay {
                replacements, tags, ..
            } => replacements
                .iter()
                .map(|spec| spec.payload.len())
                .chain(tags.iter().map(|tag| tag.payload.len()))
                .fold(0usize, usize::saturating_add),
            Kind::Standalone(package) => package
                .tags
                .iter()
                .map(|tag| tag.payload.len())
                .fold(0usize, usize::saturating_add),
            Kind::Named { tags, .. } => tags
                .iter()
                .map(|tag| tag.payload.len())
                .fold(0usize, usize::saturating_add),
        };
        // Source readers and block scratch accompany both the input and encoded output. This
        // is an admission estimate, not a bound on the size of the completed artifact set.
        let weight = payload.saturating_mul(3).saturating_add(64 * 1024 * 1024);
        let ticket = Ticket(self.jobs.len());
        self.jobs.push(Job {
            id,
            kind,
            check,
            weight,
        });
        Ok(ticket)
    }

    pub(super) fn build(
        self,
        order: Vec<Ticket>,
        progress: &mut build::Progress<'_>,
    ) -> AuthoringResult<Vec<crate::ExtendedOverlayArtifact>> {
        let count = self.jobs.len();
        if order.len() != count
            || order.iter().map(|ticket| ticket.0).collect::<BTreeSet<_>>() != (0..count).collect()
        {
            return Err(validation(
                "The package plan does not emit each job exactly once",
            ));
        }
        if self
            .jobs
            .iter()
            .map(|job| job.id)
            .collect::<BTreeSet<_>>()
            .len()
            != count
        {
            return Err(validation(
                "The package plan contains two outputs for one package",
            ));
        }
        // Keep the shared DLL alive across jobs, but release it before the source view closes.
        let _encoder = crate::block_codec::PackageBlockEncoder::open_for_packages(self.directory)?;
        let chains = crate::chain::PatchChains::scan(self.directory)?;
        let workers = std::env::var("PARHELION_PACKAGE_WORKERS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from))
            .clamp(1, MAX_WORKERS)
            .min(count.max(1));
        let stages = self
            .jobs
            .iter()
            .map(|job| {
                crate::package_profile::authored_package(job.id)
                    .expect("checked when planned")
                    .file_name
                    .to_owned()
            })
            .collect::<Vec<_>>();
        let queue = (
            Mutex::new(Queue {
                pending: self.jobs.into_iter().enumerate().collect(),
                active: 0,
                bytes: 0,
                failed: false,
            }),
            Condvar::new(),
        );
        let (send, receive) = mpsc::sync_channel(workers * 2);
        let mut artifacts = (0..count).map(|_| None).collect::<Vec<_>>();
        let mut stages = stages;
        let mut failure = None;
        let mut reporting_failed = false;
        progress.diagnostic(format!(
            "Package workers: {workers}. Admission budget: 256 MiB."
        ));
        let queued_at = Instant::now();
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..workers {
                let send = send.clone();
                let queue = &queue;
                let chains = &chains;
                let worker = std::thread::Builder::new().spawn_scoped(scope, move || {
                    loop {
                        let (index, job) = {
                            let mut state =
                                queue.0.lock().unwrap_or_else(|error| error.into_inner());
                            loop {
                                if state.failed || state.pending.is_empty() {
                                    return;
                                }
                                // The first job in plan order that fits the budget starts. A job
                                // over the budget alone starts once nothing else runs.
                                let bytes = state.bytes;
                                let fitting = if state.active == 0 {
                                    Some(0)
                                } else {
                                    state.pending.iter().position(|(_, job)| {
                                        bytes.saturating_add(job.weight) <= WORKING_BUDGET
                                    })
                                };
                                if let Some(position) = fitting {
                                    let next =
                                        state.pending.remove(position).expect("listed position");
                                    state.active += 1;
                                    state.bytes = state.bytes.saturating_add(next.1.weight);
                                    break next;
                                }
                                state = queue
                                    .1
                                    .wait(state)
                                    .unwrap_or_else(|error| error.into_inner());
                            }
                        };
                        let mut activity = activity::Reporter::new(&send, index, job.id, queued_at);
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            job.run(chains, &mut |stage| activity.start(stage))
                        }))
                        .unwrap_or_else(|_| {
                            Err(validation(format!(
                                "Package {:04x} worker panicked",
                                job.id
                            )))
                        });
                        let failed = result.is_err();
                        let weight = job.weight;
                        drop(job);
                        // Release admission before reporting completion. Every worker is joined
                        // even after an error, and no worker writes the staged package directory.
                        {
                            let mut state =
                                queue.0.lock().unwrap_or_else(|error| error.into_inner());
                            state.active -= 1;
                            state.bytes = state.bytes.saturating_sub(weight);
                            state.failed |= failed;
                            queue.1.notify_all();
                        }
                        activity.finish(failed);
                        if send.send(Event::Done(index, result)).is_err() {
                            return;
                        }
                    }
                });
                match worker {
                    Ok(handle) => handles.push(handle),
                    Err(error) => {
                        queue
                            .0
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .failed = true;
                        queue.1.notify_all();
                        failure = Some(validation(format!(
                            "Could not start a package worker: {error}"
                        )));
                        break;
                    }
                }
            }
            drop(send);
            for event in receive {
                match event {
                    Event::Activity {
                        index,
                        label,
                        timestamp,
                        activity,
                        completed,
                    } => {
                        if matches!(activity, BuildActivity::Package { .. }) {
                            stages[index].clone_from(&label);
                        }
                        if !reporting_failed
                            && catch_unwind(AssertUnwindSafe(|| {
                                progress.activity(&label, timestamp, activity, completed);
                            }))
                            .is_err()
                        {
                            reporting_failed = true;
                            queue
                                .0
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .failed = true;
                            queue.1.notify_all();
                            failure.get_or_insert_with(|| {
                                validation("Package progress reporting stopped unexpectedly")
                            });
                        }
                    }
                    Event::Done(index, Ok(artifact)) => {
                        artifacts[index] = Some(artifact);
                    }
                    Event::Done(index, Err(error)) => {
                        if failure.is_none() {
                            failure = Some(error.context(stages[index].clone()));
                        }
                    }
                }
            }
            for handle in handles {
                if handle.join().is_err() && failure.is_none() {
                    failure = Some(validation("A package worker stopped unexpectedly"));
                }
            }
        });
        if !reporting_failed {
            progress.diagnostic(format!(
                "Package emission elapsed: {:.3}s.",
                queued_at.elapsed().as_secs_f64()
            ));
        }
        if let Some(error) = failure {
            return Err(error);
        }
        order
            .into_iter()
            .map(|ticket| {
                artifacts[ticket.0]
                    .take()
                    .ok_or_else(|| validation("A package job produced no artifact"))
            })
            .collect()
    }
}

impl Job {
    fn run(
        &self,
        chains: &crate::chain::PatchChains,
        report: &mut dyn FnMut(&str),
    ) -> AuthoringResult<crate::ExtendedOverlayArtifact> {
        let profile = crate::package_profile::authored_package(self.id)
            .ok_or_else(|| invalid("An emitted package has no registered profile"))?;
        if let Some(path) = crate::mod_family::build::kept_path(profile.file_name) {
            report(&format!("Keeping Installed Package: {}", profile.file_name));
            let tags = match &self.kind {
                Kind::Standalone(package) => &package.tags,
                Kind::Named { tags, .. } => tags,
                Kind::Overlay { .. } => {
                    return Err(invalid("A stock patch cannot be kept in an importer"));
                }
            };
            return crate::ExtendedOverlayArtifact::reuse(&path, tags);
        }
        // A family build's later pass asks again for most packages of the pass before.
        let key = crate::turn_cache::keeps_packages().then(|| self.key(&profile));
        let reused = key.as_ref().and_then(crate::turn_cache::package);
        if let Some(artifact) = &reused
            && !crate::turn_cache::verifies_packages()
        {
            report(&format!("Reusing Unchanged Package: {}", profile.file_name));
            self.verify(artifact)?;
            return Ok(artifact.clone());
        }
        let mut current = profile.file_name.to_owned();
        let mut progress = |stage: &str| {
            current = format!("{stage}: {}", profile.file_name);
            report(&current);
        };
        // These builders open their own package readers. Only the immutable directory scan
        // crosses workers. The legacy codec retains its process-wide native-call lock.
        let artifact = match &self.kind {
            Kind::Overlay {
                replacements,
                tags,
                references,
            } => crate::extend::build_extended_overlay_with_chains(
                chains,
                self.id,
                replacements,
                tags,
                references,
                &mut progress,
            ),
            Kind::Standalone(package) => crate::extend::build_standalone_package_with_chains(
                chains,
                self.id,
                profile.file_name,
                &package.tags,
                &package.references,
                &mut progress,
            ),
            Kind::Named {
                tags,
                named,
                references,
            } => crate::extend::build_named_package_with_chains(
                chains,
                self.id,
                profile.file_name,
                tags,
                named,
                references,
                &mut progress,
            ),
        }
        .map_err(|error| error.context(current))?;
        self.verify(&artifact)?;
        if let Some(earlier) = reused
            && earlier.bytes() != artifact.bytes()
        {
            return Err(validation(format!(
                "{} built differently from the copy an earlier pass kept, so package reuse misses an input",
                profile.file_name
            )));
        }
        if let Some(key) = key {
            crate::turn_cache::keep_package(key, &artifact);
        }
        Ok(artifact)
    }

    /// Whether the package kept its planned entry table and complete resource groups.
    fn verify(&self, artifact: &crate::ExtendedOverlayArtifact) -> AuthoringResult<()> {
        let valid = match self.check {
            Check::Host(count) => artifact.plan.final_entry_count == count,
            Check::Overlay(count) => {
                artifact.plan.appended_tags.len() == count
                    && if count == 0 {
                        artifact.plan.original_entry_count == artifact.plan.final_entry_count
                    } else {
                        artifact.plan.append_start_entry_count.checked_add(count)
                            == Some(artifact.plan.final_entry_count)
                    }
            }
            Check::Standalone(count) => {
                artifact.plan.original_entry_count == 0
                    && artifact.plan.final_entry_count == count
                    && artifact.plan.appended_tags.len() == count
            }
            Check::Private { start, count } => {
                super::validate_private_runtime(Some(artifact), start, count)?;
                true
            }
        };
        if !valid {
            return Err(validation(format!(
                "Package {:04x} did not preserve its planned entry table and complete resource groups",
                self.id
            )));
        }
        Ok(())
    }

    /// A digest of everything the package's build reads besides the source packages, which stay
    /// the same for the whole turn: its profile, the layout the member builds in, and every entry
    /// and reference it writes.
    fn key(&self, profile: &crate::package_profile::AuthoredPackage) -> crate::turn_cache::Hash {
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        let mut field = |bytes: &[u8]| {
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        };
        field(b"parhelion-package-job");
        field(
            format!(
                "{profile:?}/{:?}/{:?}/{:?}",
                crate::package_profile::host(),
                crate::package_profile::member(),
                crate::package_profile::window()
            )
            .as_bytes(),
        );
        fn tags(field: &mut dyn FnMut(&[u8]), tags: &[NewTagSpec]) {
            for tag in tags {
                field(format!("{:?}/{:?}", tag.template_tag, tag.storage).as_bytes());
                field(&tag.payload);
            }
        }
        match &self.kind {
            Kind::Overlay {
                replacements,
                tags: appended,
                references,
            } => {
                field(b"overlay");
                for replacement in replacements {
                    field(&replacement.tag.0.to_le_bytes());
                    field(&replacement.payload);
                }
                tags(&mut field, appended);
                field(format!("{references:?}").as_bytes());
            }
            Kind::Standalone(package) => {
                field(b"standalone");
                tags(&mut field, &package.tags);
                field(format!("{:?}", package.references).as_bytes());
            }
            Kind::Named {
                tags: appended,
                named,
                references,
            } => {
                field(b"named");
                tags(&mut field, appended);
                field(format!("{named:?}/{references:?}").as_bytes());
            }
        }
        digest.finalize().into()
    }
}
