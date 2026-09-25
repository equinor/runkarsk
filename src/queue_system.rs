use tracing::instrument;

// Parsing of Args to get the queue system has become a bit too complicated.
/// Over time it has become a kind of a linear state machine. This file
/// hopefully makes it make a bit more sense and a bit less buggy.
use crate::config;
use crate::exit;

use crate::queues::{LocalScheduler, Scheduler};
use std::env::var_os;
use std::fs::read_to_string;
use std::path::Path;
use std::process::Command;
use std::thread::available_parallelism;

fn count_lines(path: impl AsRef<Path>) -> usize {
    let path = path.as_ref();
    let text = read_to_string(path)
        .unwrap_or_else(|e| exit!("Could not open file '{}': {}", path.display(), e));
    text.lines().count()
}

/// Get maximum allowed CPU on this current system. Doesn't apply to cluster
/// workloads as we don't know how many CPU cores they have
fn get_max_allowed_cpu(requested: Option<usize>) -> usize {
    let clamp = |available: usize| available.min(requested.unwrap_or(available));

    for key in config::MACHINEFILES {
        let Some(file) = var_os(key) else {
            continue;
        };

        let hostfile_max = count_lines(file);
        return clamp(hostfile_max);
    }

    let machine_max = available_parallelism().map_or(1, |x| x.get());
    clamp(machine_max)
}

/// First step with data exactly as it comes from argument parsing
struct Step1 {
    queue: String,
    num_tasks_per_machine: Option<usize>,
    num_machines: usize,
}

impl Step1 {
    /// When running on HPC directly, we must ensure that we don't submit
    /// another job to the cluster. Our users may be using Ert to run this
    /// program, which is responsible for managing the jobs.
    fn enforce_local_on_hpc(self) -> Self {
        let are_we_on_hpc = config::MACHINEFILES.iter().all(|key| var_os(key).is_some());

        if self.queue != "local" && are_we_on_hpc {
            Self {
                queue: String::from("local"),
                num_tasks_per_machine: self.num_tasks_per_machine,
                num_machines: 1,
            }
        } else {
            self
        }
    }

    fn clamp_to_available_cores(self) -> Self {
        if self.queue == "local" {
            Self {
                queue: self.queue,
                num_tasks_per_machine: Some(get_max_allowed_cpu(self.num_tasks_per_machine)),
                num_machines: self.num_machines,
            }
        } else {
            self
        }
    }

    fn into_step2(self) -> Step2 {
        let num_tasks_per_machine = self.num_tasks_per_machine.unwrap_or_else(|| {
            exit!("Must specify -n/--num-tasks-per-machine when running on a non-local queue");
        });

        Step2 {
            queue: if self.queue == "local" {
                None
            } else {
                Some(self.queue)
            },
            num_tasks_per_machine,
            num_machines: self.num_machines,
        }
    }
}

struct Step2 {
    /// Some(_) if we are to use the queue system and None if local
    queue: Option<String>,
    num_tasks_per_machine: usize,
    num_machines: usize,
}

#[derive(Debug)]
pub enum QueueSystem {
    Local {
        num_tasks_per_machine: usize,
    },

    #[allow(unused)]
    Cluster {
        queue: String,
        num_tasks_per_machine: usize,
        num_machines: usize,
    },
}

impl QueueSystem {
    pub fn from_args(
        queue: String,
        num_tasks_per_machine: Option<usize>,
        num_machines: usize,
    ) -> Self {
        let step1 = Step1 {
            queue,
            num_tasks_per_machine,
            num_machines,
        };

        let step2 = step1
            .enforce_local_on_hpc()
            .clamp_to_available_cores()
            .into_step2();

        match step2.queue {
            Some(queue) => QueueSystem::Cluster {
                queue,
                num_tasks_per_machine: step2.num_tasks_per_machine,
                num_machines: step2.num_machines,
            },

            None => QueueSystem::Local {
                num_tasks_per_machine: step2.num_tasks_per_machine,
            },
        }
    }

    pub fn num_tasks(&self) -> usize {
        match self {
            QueueSystem::Local {
                num_tasks_per_machine,
            } => *num_tasks_per_machine,
            QueueSystem::Cluster {
                num_tasks_per_machine,
                num_machines,
                ..
            } => *num_tasks_per_machine * *num_machines,
        }
    }

    #[instrument(fields(otel.kind = "client"))]
    pub async fn exec(&self, command: Command) {
        match self {
            QueueSystem::Local { .. } => {
                let s = LocalScheduler::new();
                s.exec(command).await.unwrap();
            }

            QueueSystem::Cluster { .. } => {
                unimplemented!();
            }
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
        let expect = available_parallelism().unwrap().get();

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

    fn set_machinefile(path: impl AsRef<Path>, num_hosts: usize) {
        set_machinefile_for_var(path, num_hosts, config::MACHINEFILES[0]);
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
            let expect = num_hosts;

            assert_eq!(actual, expect);
        }
    }
}
