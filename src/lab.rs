//! The run: generate mutants, filter them, test the baseline, then each
//! mutant across the requested jobs.

use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
    sync::{Mutex, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};

use crate::{
    arch::Aarch64,
    build_dir::{BuildDir, CopyOptions},
    cargo::Invocation,
    cli::{BaselineStrategy, Sharding},
    console::Console,
    mutant::Mutant,
    options::{FALLBACK_TIMEOUT, Options},
    outcome::{ExitCode, LabOutcome, Phase, Scenario, ScenarioOutcome, Summary},
    output::OutputDir,
    source::SourceTree,
};

/// Per-phase limits derived from the baseline.
#[derive(Clone, Copy, Debug)]
struct Timeouts {
    build: Duration,
    test: Duration,
}

/// One mutant queued for a worker, with its output file stem.
struct Job {
    index: usize,
    mutant: Mutant,
    stem: String,
}

pub struct Lab {
    options: Options,
    tree: SourceTree,
    files: Vec<std::path::PathBuf>,
    mutants: Vec<Mutant>,
}

impl Lab {
    /// Discovers the files, generates the mutants, and applies every filter.
    pub fn new(options: Options) -> Result<Self> {
        let tree = SourceTree::new(options.dir.clone());
        // Globs apply to the file each site sits in, so an included `.s`
        // can be selected or excluded on its own name.
        let mut sites = Vec::new();
        for file in tree.asm_files(options.gitignore)? {
            sites.extend(
                tree.sites(&file)?
                    .into_iter()
                    .filter(|site| options.examines_file(&site.file.path)),
            );
        }
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        for site in &sites {
            if !files.contains(&site.file.path) {
                files.push(site.file.path.clone());
            }
        }

        let mut mutants = Vec::new();
        for site in &sites {
            for mutation in site.instruction.mutations(&Aarch64, &options.operators) {
                mutants.push(Mutant {
                    site: site.clone(),
                    mutation,
                });
            }
        }
        mutants.retain(|mutant| {
            options.examines_mutant(&mutant.name())
                && options
                    .in_diff
                    .as_ref()
                    .is_none_or(|diff| diff.contains(&mutant.site.file.path, mutant.site.line + 1))
        });

        if let Some(shard) = options.shard {
            let total = mutants.len();
            mutants = mutants
                .into_iter()
                .enumerate()
                .filter(|(i, _)| match options.sharding {
                    Sharding::RoundRobin => i % shard.n == shard.k,
                    Sharding::Slice => {
                        let width = total.div_ceil(shard.n);
                        i / width == shard.k
                    }
                })
                .map(|(_, m)| m)
                .collect();
        }
        if options.shuffle {
            Self::shuffle(&mut mutants);
        }

        Ok(Self {
            options,
            tree,
            files,
            mutants,
        })
    }

    /// Fisher-Yates with a splitmix64 stream seeded from the clock.
    fn shuffle(mutants: &mut [Mutant]) {
        let mut state = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64);
        let mut next = move || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        for i in (1..mutants.len()).rev() {
            let j = (next() % (i as u64 + 1)) as usize;
            mutants.swap(i, j);
        }
    }

    pub fn run(self) -> Result<ExitCode> {
        if self.options.list_files {
            for file in &self.files {
                println!("{}", file.display());
            }
            return Ok(ExitCode::Success);
        }
        if self.options.list {
            self.list()?;
            return Ok(ExitCode::Success);
        }
        self.test()
    }

    fn list(&self) -> Result<()> {
        if self.options.json {
            let records: Vec<_> = self.mutants.iter().map(Mutant::record).collect();
            println!("{}", serde_json::to_string_pretty(&records)?);
            return Ok(());
        }
        for mutant in &self.mutants {
            println!("{mutant}");
            if self.options.diff {
                print!("{}", mutant.diff());
            }
        }
        Ok(())
    }

    /// Distinct `log/` and `diff/` stems: several operators share a site.
    fn stems(&self) -> Vec<String> {
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        self.mutants
            .iter()
            .map(|mutant| {
                let stem = mutant.file_stem();
                let n = seen.entry(stem.clone()).or_default();
                *n += 1;
                if *n == 1 { stem } else { format!("{stem}_{n}") }
            })
            .collect()
    }

    fn console(&self) -> Console {
        Console {
            show_caught: self.options.show_caught,
            show_unviable: self.options.show_unviable,
            no_times: self.options.no_times,
        }
    }

    fn build_dir(&self) -> Result<BuildDir> {
        if self.options.in_place {
            Ok(BuildDir::in_place(&self.tree))
        } else {
            BuildDir::copy_from(
                &self.tree,
                CopyOptions {
                    copy_target: self.options.copy_target,
                    copy_vcs: self.options.copy_vcs,
                    gitignore: self.options.gitignore,
                    leak: self.options.leak_dirs,
                },
            )
        }
    }

    fn test(&self) -> Result<ExitCode> {
        let console = self.console();
        let output = OutputDir::new(&self.options.output_parent)?;
        let records: Vec<_> = self.mutants.iter().map(Mutant::record).collect();
        output.write_mutants(&records)?;
        console.found(self.mutants.len());

        let started = Instant::now();
        let mut lab = LabOutcome::new(self.mutants.len());
        let first = self.build_dir()?;

        let timeouts = match self.options.baseline {
            BaselineStrategy::Run => {
                let baseline = self.baseline(&first, &output)?;
                console.scenario(&baseline);
                let timeouts = self.timeouts_from(&baseline);
                lab.add(baseline);
                if lab.exit_code() == ExitCode::BaselineFailed {
                    lab.finish();
                    output.write_outcomes(&lab)?;
                    bail!(
                        "baseline failed; see {}",
                        output.path().join("log/baseline.log").display()
                    );
                }
                timeouts
            }
            BaselineStrategy::Skip => Timeouts {
                build: self.options.build_timeout.unwrap_or(FALLBACK_TIMEOUT),
                test: self.options.timeout.unwrap_or(FALLBACK_TIMEOUT),
            },
        };

        let jobs = if self.options.in_place {
            1
        } else {
            self.options.jobs
        };
        let mut build_dirs = vec![first];
        for _ in 1..jobs {
            build_dirs.push(self.build_dir()?);
        }

        let stems = self.stems();
        let queue: Mutex<VecDeque<Job>> = Mutex::new(
            self.mutants
                .iter()
                .cloned()
                .zip(stems)
                .enumerate()
                .map(|(index, (mutant, stem))| Job {
                    index,
                    mutant,
                    stem,
                })
                .collect(),
        );
        let (sender, receiver) = mpsc::channel::<(usize, Result<ScenarioOutcome>)>();

        thread::scope(|scope| {
            for build_dir in &build_dirs {
                let sender = sender.clone();
                let queue = &queue;
                let output = &output;
                scope.spawn(move || {
                    loop {
                        let job = queue.lock().map(|mut q| q.pop_front()).unwrap_or(None);
                        let Some(job) = job else { break };
                        let outcome = self.test_mutant(build_dir, &job, output, timeouts);
                        if sender.send((job.index, outcome)).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(sender);
            for (_, outcome) in receiver {
                let outcome = outcome?;
                console.scenario(&outcome);
                output.record(&outcome)?;
                lab.add(outcome);
            }
            Ok::<(), anyhow::Error>(())
        })?;

        lab.finish();
        output.write_outcomes(&lab)?;
        console.summary(&lab, started.elapsed().as_secs_f64());
        Ok(lab.exit_code())
    }

    /// Builds and tests the unmutated tree.
    fn baseline(&self, build_dir: &BuildDir, output: &OutputDir) -> Result<ScenarioOutcome> {
        let (mut log, log_path) = output.create_log("baseline")?;
        let generous = Timeouts {
            build: self
                .options
                .build_timeout
                .unwrap_or(Duration::from_secs(3600)),
            test: self.options.timeout.unwrap_or(Duration::from_secs(3600)),
        };
        let phases = self.phases(build_dir, &mut log, generous)?;
        Ok(ScenarioOutcome::new(
            Scenario::Baseline,
            log_path,
            None,
            phases,
            !self.options.check_only,
        ))
    }

    /// Mutant timeouts: explicit, else a multiple of the baseline's time
    /// with a floor.
    fn timeouts_from(&self, baseline: &ScenarioOutcome) -> Timeouts {
        let scaled = |phase: Phase, multiplier: f64, minimum: Duration| {
            baseline
                .duration(phase)
                .map(|secs| Duration::from_secs_f64(secs * multiplier).max(minimum))
                .unwrap_or(FALLBACK_TIMEOUT)
        };
        Timeouts {
            build: self.options.build_timeout.unwrap_or_else(|| {
                scaled(
                    Phase::Build,
                    self.options.build_timeout_multiplier,
                    self.options.minimum_test_timeout,
                )
            }),
            test: self.options.timeout.unwrap_or_else(|| {
                scaled(
                    Phase::Test,
                    self.options.timeout_multiplier,
                    self.options.minimum_test_timeout,
                )
            }),
        }
    }

    /// Build, then test if the build passed and tests were asked for.
    fn phases(
        &self,
        build_dir: &BuildDir,
        log: &mut std::fs::File,
        timeouts: Timeouts,
    ) -> Result<Vec<crate::outcome::PhaseResult>> {
        let build = Invocation::new(self.options.cargo.build_argv(), build_dir.path()).run(
            Phase::Build,
            log,
            timeouts.build,
        )?;
        let built = build.process_status == crate::outcome::ProcessStatus::Success;
        let mut phases = vec![build];
        if built && !self.options.check_only {
            phases.push(
                Invocation::new(self.options.cargo.test_argv(), build_dir.path()).run(
                    Phase::Test,
                    log,
                    timeouts.test,
                )?,
            );
        }
        Ok(phases)
    }

    fn test_mutant(
        &self,
        build_dir: &BuildDir,
        job: &Job,
        output: &OutputDir,
        timeouts: Timeouts,
    ) -> Result<ScenarioOutcome> {
        let mutant = &job.mutant;
        let (mut log, log_path) = output.create_log(&job.stem)?;
        writeln!(log, "{mutant}")?;
        let diff_path = output.write_diff(&job.stem, &mutant.diff())?;

        build_dir.apply(mutant)?;
        let phases = self.phases(build_dir, &mut log, timeouts);
        build_dir
            .restore(mutant)
            .with_context(|| format!("restore after {mutant}"))?;
        let outcome = ScenarioOutcome::new(
            Scenario::Mutant(mutant.record()),
            log_path,
            Some(diff_path),
            phases?,
            !self.options.check_only,
        );
        debug_assert!(outcome.summary != Summary::BaselineFailed);
        Ok(outcome)
    }
}
