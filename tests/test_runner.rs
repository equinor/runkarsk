mod setup;

use crate::setup::copy_test_data_to;
use setup::bin_runner;
use std::path::PathBuf;
use std::process::Command;
use testdir::testdir;

#[test]
fn empty_args() {
    // We expect an error message in stderr from clap-rs
    let output = bin_runner().output().unwrap();
    assert_eq!(String::from_utf8(output.stdout), Ok(String::new()));

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("error: the following required arguments were not provided:"));
}

fn setup_command_in(tmp: &PathBuf) -> Command {
    copy_test_data_to(&tmp);

    let mut command = bin_runner();
    command.args(["-n", "2", "-c", "somecase"]);

    let outdir = tmp.join("outdir");
    command.arg("-C").arg(outdir.clone().into_os_string());

    let mpirun = tmp
        .join("cirrus/versions/1.2.3/bin/mpirun")
        .into_os_string();
    command.arg("-M").arg(mpirun);

    let cirrus = tmp
        .join("cirrus/versions/1.2.3/bin/cirrus")
        .into_os_string();
    command.arg(cirrus).args(["-cirrusin", "somefile.in"]);

    command
}

#[test]
fn basic_runner_invocation() {
    let tmp = testdir!();

    let mut command = setup_command_in(&tmp);
    let output = command.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(
        stdout,
        "Fake MPIRun with np=2, machinefile=unset\nFake Cirrus 1.2.3 with args: -cirrusin somefile.in\n"
    );
    assert_eq!(stderr, "");

    let stdout = std::fs::read_to_string(tmp.join("outdir").join("somecase.LOG")).unwrap();
    let stderr = std::fs::read_to_string(tmp.join("outdir").join("somecase.ERR")).unwrap();
    assert_eq!(
        stdout,
        "Fake MPIRun with np=2, machinefile=unset\nFake Cirrus 1.2.3 with args: -cirrusin somefile.in\n"
    );
    assert_eq!(stderr, "");
}

#[test]
fn runner_with_machinefile() {
    let tmp = testdir!();
    let mut command = setup_command_in(&tmp);

    let machinefile = tmp.join("machinefile");
    std::fs::write(&machinefile, "localhost\n".repeat(4)).unwrap();
    command.env(
        runkarsk::config::MACHINEFILES[0],
        machinefile.clone().into_os_string(),
    );

    let output = command.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(
        stdout,
        format!(
            "Fake MPIRun with np=unset, machinefile={}\nFake Cirrus 1.2.3 with args: -cirrusin somefile.in\n",
            machinefile.display()
        )
    );
    assert_eq!(stderr, "");

    let stdout = std::fs::read_to_string(tmp.join("outdir").join("somecase.LOG")).unwrap();
    let stderr = std::fs::read_to_string(tmp.join("outdir").join("somecase.ERR")).unwrap();
    assert_eq!(
        stdout,
        format!(
            "Fake MPIRun with np=unset, machinefile={}\nFake Cirrus 1.2.3 with args: -cirrusin somefile.in\n",
            machinefile.display()
        )
    );
    assert_eq!(stderr, "");
}
