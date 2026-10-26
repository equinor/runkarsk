use tracing::instrument;

// Parsing of Args to get the queue system has become a bit too complicated.
/// Over time it has become a kind of a linear state machine. This file
/// hopefully makes it make a bit more sense and a bit less buggy.
use crate::config;

use crate::config::MACHINEFILES;
use crate::queue_args::QueueArgs;
use clap::ValueEnum;
use serde::Serialize;
use std::env::var_os;
use std::error::Error;
use std::fmt;
use std::num::NonZero;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::process::Command;
use std::thread::available_parallelism;
use tokio::process::Command as AsyncCommand;

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
        Self::from_args_private(args, &QueueSystemKind::available(), in_cluster_job())
    }

    fn from_args_private(
        args: &QueueArgs,
        kinds: &[QueueSystemKind],
        in_cluster_job: bool,
    ) -> Result<Self, String> {
        let max_n = available_parallelism().unwrap_or(NonZeroUsize::MIN);

        if in_cluster_job {
            if (args.queue.is_some() && args.queue != Some("local".into()))
                || args.queue_system.is_some()
                || args.num_machines.is_some()
            {
                Err("Already running as a HPC job. It is invalid to specify -Q/--queue-system, -q/--queue or -m/--num-machines".to_string())
            } else {
                Ok(Self::Local(
                    args.num_tasks_per_machine.unwrap_or(NonZeroUsize::MIN),
                ))
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
                if kinds.contains(&kind) {
                    Ok(Self::Cluster {
                        kind,
                        queue,
                        num_machines,
                        num_tasks_per_machine,
                    })
                } else {
                    Err(format!(
                        "Can't start job: Queue system '{kind}' requested, but isn't available"
                    ))
                }
            } else if kinds.len() > 1 {
                Err(format!(
                    "Can't start job: Multiple queue systems detected. Select one of [{}] by using -Q/--queue-system",
                    kinds
                        .iter()
                        .map(|q| q.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            } else if let Some(kind) = kinds.iter().next() {
                Ok(Self::Cluster {
                    kind: *kind,
                    queue,
                    num_machines,
                    num_tasks_per_machine,
                })
            } else {
                Err("Can't start job: No queue systems detected.".to_string())
            }
        }
    }

    pub fn prepare_exec(&self, command: Command) -> AsyncCommand {
        match self {
            QueueSystem::Local { .. } => AsyncCommand::from(command),

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

                c
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
    use crate::config::runner_path;

    use super::*;
    use clap::Parser as _;

    fn parse_args(args: &[&str]) -> Result<QueueArgs, clap::Error> {
        QueueArgs::try_parse_from(std::iter::once("/bin/dummy").chain(args.iter().copied()))
    }

    fn prepare_command(
        args: &[&str],
        kinds: &[QueueSystemKind],
        in_cluster_job: bool,
    ) -> Result<Vec<String>, String> {
        let qs = QueueSystem::from_args_private(&parse_args(args).unwrap(), kinds, in_cluster_job)?;
        let command = qs.prepare_exec(qs.command());
        let command = command.as_std();

        Ok(std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|s| s.to_string_lossy().into_owned())
            .collect())
    }

    fn runner() -> String {
        runner_path().into_string().unwrap()
    }

    const NONE: &[QueueSystemKind; 0] = &[];
    const LSF: &[QueueSystemKind; 1] = &[QueueSystemKind::Lsf];
    const PBS: &[QueueSystemKind; 1] = &[QueueSystemKind::Pbs];
    const LSF_PBS: &[QueueSystemKind; 2] = &[QueueSystemKind::Lsf, QueueSystemKind::Pbs];

    #[test]
    fn test_default_args() {
        let nproc = available_parallelism().unwrap();

        assert_eq!(
            prepare_command(&[], NONE, false).unwrap(),
            [runner(), "-n".into(), format!("{nproc}")]
        );
    }

    #[test]
    fn test_default_args_as_cluster_job() {
        assert_eq!(
            prepare_command(&[], NONE, true).unwrap(),
            [runner().as_str(), "-n", "1"]
        );
    }

    #[test]
    fn test_queue_name_is_local() {
        let nproc = available_parallelism().unwrap();

        assert_eq!(
            prepare_command(&["-q", "local"], NONE, false).unwrap(),
            [runner(), "-n".into(), format!("{nproc}")]
        );
    }

    #[test]
    fn test_queue_name_is_local_as_cluster_job() {
        assert_eq!(
            prepare_command(&["-q", "local"], NONE, true).unwrap(),
            [runner().as_str(), "-n", "1"]
        );
    }

    #[test]
    fn test_queue_is_not_local_as_cluster_job() {
        assert_eq!(
            prepare_command(&["-q", "normal"], NONE, true).unwrap_err(),
            "Already running as a HPC job. It is invalid to specify -Q/--queue-system, -q/--queue or -m/--num-machines"
        );
    }

    #[test]
    fn test_queue_system_defined_as_cluster_job() {
        assert_eq!(
            prepare_command(&["-Q", "lsf"], NONE, true).unwrap_err(),
            "Already running as a HPC job. It is invalid to specify -Q/--queue-system, -q/--queue or -m/--num-machines"
        );
    }

    #[test]
    fn test_num_machines_defined_as_cluster_job() {
        assert_eq!(
            prepare_command(&["-m", "2"], NONE, true).unwrap_err(),
            "Already running as a HPC job. It is invalid to specify -Q/--queue-system, -q/--queue or -m/--num-machines"
        );
    }

    #[test]
    fn test_local_num_tasks() {
        assert_eq!(
            prepare_command(&["-n", "2"], NONE, false).unwrap(),
            [runner().as_str(), "-n", "2"]
        );
    }

    #[test]
    fn test_local_num_machines() {
        assert_eq!(
            prepare_command(&["-m", "2"], NONE, false).unwrap_err(),
            "Can't start job: Specify a queue system (-Q/--queue-system) to run on '2' machines"
        );
    }

    #[test]
    fn test_reject_implicit_queue_when_no_queue_systems() {
        assert_eq!(
            prepare_command(&["-q", "normal"], NONE, false).unwrap_err(),
            "Can't start job: No queue systems detected."
        );
    }

    #[test]
    fn test_implicit_queue_when_lsf_exists() {
        assert_eq!(
            prepare_command(&["-q", "normal"], LSF, false).unwrap(),
            [
                "bsub",
                "-n",
                "1",
                "-R",
                "span[ptile=1]",
                "-q",
                "normal",
                "--",
                runner().as_str(),
                "-n",
                "1",
            ]
        );
    }

    #[test]
    fn test_implicit_queue_when_pbs_exists() {
        assert_eq!(
            prepare_command(&["-q", "normal"], PBS, false).unwrap(),
            [
                "qsub",
                "-l",
                "select=1:ncpus=1:mpiproc=1",
                "-l",
                "place=scatter",
                "-q",
                "normal",
                "--",
                runner().as_str(),
                "-n",
                "1",
            ]
        );
    }

    #[test]
    fn test_reject_implicit_queue_system_when_multiple_are_available() {
        assert_eq!(
            prepare_command(&["-q", "normal"], LSF_PBS, false).unwrap_err(),
            "Can't start job: Multiple queue systems detected. Select one of [lsf, pbs] by using -Q/--queue-system"
        );
    }

    #[test]
    fn test_lsf_with_num_tasks() {
        assert_eq!(
            prepare_command(&["-q", "normal", "-n", "3"], LSF, false).unwrap(),
            [
                "bsub",
                "-n",
                "3",
                "-R",
                "span[ptile=3]",
                "-q",
                "normal",
                "--",
                runner().as_str(),
                "-n",
                "3",
            ]
        );
    }

    #[test]
    fn test_lsf_with_num_machines() {
        assert_eq!(
            prepare_command(&["-q", "normal", "-m", "5"], LSF, false).unwrap(),
            [
                "bsub",
                "-n",
                "5",
                "-R",
                "span[ptile=1]",
                "-q",
                "normal",
                "--",
                runner().as_str(),
                "-n",
                "5",
            ]
        );
    }

    #[test]
    fn test_lsf_with_both_num_tasks_and_num_machines() {
        assert_eq!(
            prepare_command(
                &["-Q", "lsf", "-q", "normal", "-n", "3", "-m", "5"],
                LSF,
                false
            )
            .unwrap(),
            [
                "bsub",
                "-n",
                "15",
                "-R",
                "span[ptile=3]",
                "-q",
                "normal",
                "--",
                runner().as_str(),
                "-n",
                "15",
            ]
        );
    }

    #[test]
    fn test_reject_lsf_when_not_installed() {
        assert_eq!(
            prepare_command(&["-Q", "lsf"], PBS, false).unwrap_err(),
            "Can't start job: Queue system 'lsf' requested, but isn't available"
        );
    }

    #[test]
    fn test_reject_pbs_when_not_installed() {
        assert_eq!(
            prepare_command(&["-Q", "pbs"], LSF, false).unwrap_err(),
            "Can't start job: Queue system 'pbs' requested, but isn't available"
        );
    }
}
