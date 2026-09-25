use std::env;
use std::path::PathBuf;

pub fn karsksal_root() -> PathBuf {
    env::var_os("KARSKSAL_ROOT")
        .or(option_env!("KARSKSAL_ROOT").map(|s| s.into()))
        .expect("environment variable KARSKSAL_ROOT to be set")
        .into()
}

pub fn wrapper_path() -> PathBuf {
    karsksal_root().join("bin").join("cirrus")
}

pub fn runner_path() -> PathBuf {
    let path = env::current_exe().expect("Couldn't obtain this program's path");
    path.parent().expect("Couldn't get parent").join("runner")
}

/// Environment variable for "machinefile"/"hostfile" - a list of hosts with one
/// line per hostname
pub const MACHINEFILES: [&str; 2] = ["LSB_MCPU_HOSTS", "PBS_NODEFILE"];
