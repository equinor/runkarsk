use tracing::instrument;

// Parsing of Args to get the queue system has become a bit too complicated.
/// Over time it has become a kind of a linear state machine. This file
/// hopefully makes it make a bit more sense and a bit less buggy.
use crate::config;
use crate::exit;

use crate::config::MACHINEFILES;
use crate::queue_args::QueueArgs;
use clap::ValueEnum;
use serde::Serialize;
use std::env::var_os;
use std::error::Error;
use std::fmt;
use std::fs::read_to_string;
use std::num::NonZero;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::thread::available_parallelism;
use tokio::process::Command as AsyncCommand;

fn count_lines(path: impl AsRef<Path>) -> usize {
    let path = path.as_ref();
    let text = read_to_string(path)
        .unwrap_or_else(|e| exit!("Could not open file '{}': {}", path.display(), e));
    text.lines().count()
}

/// Get maximum allowed CPU on this current system. Doesn't apply to cluster
/// workloads as we don't know how many CPU cores they have
fn get_max_allowed_cpu(requested: Option<NonZero<usize>>) -> NonZero<usize> {
    let clamp = |available: NonZero<usize>| available.min(requested.unwrap_or(available));

    for key in config::MACHINEFILES {
        let Some(hostfile_max) = var_os(key).map(count_lines).and_then(NonZero::new) else {
            continue;
        };
        return clamp(hostfile_max);
    }

    let machine_max = available_parallelism().unwrap_or(NonZeroUsize::MIN);
    clamp(machine_max)
}

#[derive(Debug, Copy, Clone, ValueEnum, Serialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum QueueSystemKind {
    Lsf,
    Pbs,
}

impl QueueSystemKind {
    pub const ALL: [Self; 2] = [Self::Lsf, Self::Pbs];

    pub fn name(self) -> &'static str {
        match self {
            Self::Lsf => "lsf",
            Self::Pbs => "pbs",
        }
    }

    pub fn program_name(self) -> &'static str {
        match self {
            Self::Lsf => "bsub",
            Self::Pbs => "qsub",
        }
    }

    pub fn which(self) -> Option<PathBuf> {
        which::which(self.program_name()).ok()
    }

    pub fn available() -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|kind| kind.which().is_some())
            .collect()
    }
}

impl fmt::Display for QueueSystemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

fn in_cluster_job() -> bool {
    MACHINEFILES.iter().any(|key| var_os(key).is_some())
}

#[derive(Debug)]
pub enum QueueSystem {
    Local(NonZero<usize>),
    Cluster {
        kind: QueueSystemKind,
        queue: Option<String>,
        num_tasks_per_machine: NonZero<usize>,
        num_machines: NonZero<usize>,
    },
}

impl QueueSystem {
    pub fn from_args(args: &QueueArgs) -> Result<Self, String> {
        let max_n = available_parallelism().unwrap_or(NonZeroUsize::MIN);
        let max_qs = QueueSystemKind::available();

        if in_cluster_job() {
            if args.queue.is_some() || args.queue_system.is_some() || args.num_machines.is_some() {
                Err("Already running as a HPC job. It is invalid to specify -Q/--queue-system, -q/--queue or -m/--num-machines".to_string())
            } else {
                Ok(Self::Local(get_max_allowed_cpu(args.num_tasks_per_machine)))
            }
        } else if args.is_local() {
            if let Some(m) = args.num_machines {
                Err(format!(
                    "Can't start job: Specify a queue system (-Q/--queue-system) to run on '{m}' machines"
                ))
            } else if let Some(req_n) = args.num_tasks_per_machine {
                if req_n > max_n {
                    Err(format!(
                        "Can't start job: Requested {req_n} CPUs, but maximum is {max_n} on this machine"
                    ))
                } else {
                    Ok(Self::Local(req_n))
                }
            } else {
                Ok(Self::Local(max_n))
            }
        } else {
            let queue = args.queue.clone();
            let num_machines = args.num_machines.unwrap_or(NonZeroUsize::MIN);
            let num_tasks_per_machine = args.num_tasks_per_machine.unwrap_or(NonZeroUsize::MIN);

            if let Some(kind) = args.queue_system {
                if kind.which().is_none() {
                    Err(format!(
                        "Can't start job: Queue system '{kind}' requested, but isn't available"
                    ))
                } else {
                    Ok(Self::Cluster {
                        kind,
                        queue,
                        num_machines,
                        num_tasks_per_machine,
                    })
                }
            } else if max_qs.len() > 1 {
                Err(format!(
                    "Can't start job: Multiple queue systems detected. Select one of [{}] by using -Q/--queue-system",
                    max_qs
                        .into_iter()
                        .map(|q| q.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            } else if let Some(kind) = max_qs.into_iter().next() {
                Ok(Self::Cluster {
                    kind,
                    queue,
                    num_machines,
                    num_tasks_per_machine,
                })
            } else {
                unreachable!()
            }
        }
    }

    #[instrument(fields(otel.kind = "client"))]
    pub async fn exec(&self, command: Command) -> Result<(), Box<dyn Error>> {
        match self {
            QueueSystem::Local { .. } => {
                let mut command = AsyncCommand::from(command);

                let mut child = command.spawn()?;
                child.wait().await?;
                Ok(())
            }

            QueueSystem::Cluster {
                kind,
                queue,
                num_machines,
                num_tasks_per_machine,
            } => {
                let mut c = AsyncCommand::new(kind.program_name());

                match kind {
                    QueueSystemKind::Lsf => {
                        c.arg("-n")
                            .arg(self.num_tasks().to_string())
                            .arg("-R")
                            .arg(format!("span[ptile={num_tasks_per_machine}]"));
                    }
                    QueueSystemKind::Pbs => {
                        c.arg("-l")
                               .arg(format!("select={num_machines}:ncpus={num_tasks_per_machine}:mpiproc={num_tasks_per_machine}"))
                               .arg("-l").arg("place=scatter");
                    }
                }

                // LSF and PBS use the same argument for queue
                if let Some(queue) = queue {
                    c.arg("-q").arg(queue);
                };

                c.arg("--")
                    .arg(command.get_program())
                    .args(command.get_args());

                let mut child = c.spawn()?;
                child.wait().await?;
                Ok(())
            }
        }
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(config::runner_path());

        command.arg("-n").arg(self.num_tasks().to_string());

        command
    }

    pub fn num_tasks(&self) -> NonZero<usize> {
        match self {
            Self::Local(n) => *n,
            Self::Cluster {
                num_tasks_per_machine,
                num_machines,
                ..
            } => num_machines.saturating_mul(*num_tasks_per_machine),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use testdir::testdir;

    /// Test fixture that removes MACHINEFILE variables from the environment in case they are
    /// defined
    struct HideVars {
        vars_os: Vec<(&'static str, Option<OsString>)>,
    }

    impl HideVars {
        fn new() -> Self {
            let vars_os: Vec<(&'static str, Option<OsString>)> = config::MACHINEFILES
                .iter()
                .copied()
                .map(|key| (key, std::env::var_os(key)))
                .collect();

            for (key, val) in vars_os.iter() {
                if val.is_some() {
                    unsafe { std::env::remove_var(key) }
                }
            }

            Self { vars_os }
        }
    }

    impl Drop for HideVars {
        fn drop(&mut self) {
            for (key, val) in self.vars_os.iter() {
                unsafe {
                    match val {
                        Some(x) => std::env::set_var(key, x),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

    #[test]
    fn test_get_max_allowed_cpu_with_no_hostfile_defined() {
        let _hide_vars = HideVars::new();

        let actual = get_max_allowed_cpu(None);
        let expect = available_parallelism().unwrap();

        assert_eq!(actual, expect);
    }

    /// Create a machinefile and set a specific environment variable to point to it.
    fn set_machinefile_for_var(path: impl AsRef<Path>, num_hosts: usize, key: &str) {
        let path = path.as_ref();

        std::fs::write(path, "localhost\n".repeat(num_hosts))
            .expect("Couldn't create a machinefile");
        unsafe {
            std::env::set_var(key, path.as_os_str());
        }
    }

    #[test]
    fn test_get_max_allowed_cpu() {
        let params = [
            (config::MACHINEFILES[0], 2),
            (config::MACHINEFILES[0], 4),
            (config::MACHINEFILES[1], 8),
        ];

        let tmp_path = testdir!();

        for (i, (key, num_hosts)) in params.into_iter().enumerate() {
            let _hide_vars = HideVars::new();

            set_machinefile_for_var(tmp_path.join(format!("hostfile-{}", i)), num_hosts, key);

            let actual = get_max_allowed_cpu(None);
            let expect = NonZero::new(num_hosts).unwrap();

            assert_eq!(actual, expect);
        }
    }
}
