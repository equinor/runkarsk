mod setup;

use crate::setup::copy_test_data_to;
use setup::bin_runcirrus;
use testdir::testdir;

#[test]
fn empty_args() {
    let output = bin_runcirrus().output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(stdout, "");
    assert!(stderr.starts_with("error: the following required arguments were not provided:"));
}

#[test]
fn run_local_with_just_input_file() {
    let tmp = testdir!();
    copy_test_data_to(&tmp);

    std::fs::create_dir(tmp.join("project")).unwrap();
    std::fs::write(tmp.join("project/case.in"), "foobar").unwrap();

    let mut command = bin_runcirrus();
    command.env("KARSKSAL_ROOT", &tmp);
    command.arg(tmp.join("project/case.in"));

    let output = command.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    let np = std::thread::available_parallelism().unwrap();
    assert_eq!(
        stdout,
        format!(
            "Fake MPIRun with np={np}, machinefile=unset\nFake Cirrus 1.2.3 with args: -cirrusin {}\n",
            tmp.join("project/case.in").display()
        )
    );
    assert_eq!(stderr, "");
}

#[test]
fn run_local_with_specified_version() {
    let tmp = testdir!();
    copy_test_data_to(&tmp);

    std::fs::create_dir(tmp.join("project")).unwrap();
    std::fs::write(tmp.join("project/case.in"), "foobar").unwrap();

    let mut command = bin_runcirrus();
    command.env("KARSKSAL_ROOT", &tmp);
    command.args(["-v", "2.0.0"]);
    command.arg(tmp.join("project/case.in"));

    let output = command.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    let np = std::thread::available_parallelism().unwrap().get();
    assert_eq!(
        stdout,
        format!(
            "Fake MPIRun with version=2.0.0 np={}, machinefile=unset\nFake Cirrus 2.0.0 with args: -cirrusin {}\n",
            np,
            tmp.join("project/case.in").display()
        )
    );
    assert_eq!(stderr, "");
}

#[test]
fn run_local_with_specified_version_using_symlink() {
    let tmp = testdir!();
    copy_test_data_to(&tmp);

    std::fs::create_dir(tmp.join("project")).unwrap();
    std::fs::write(tmp.join("project/case.in"), "foobar").unwrap();

    let mut command = bin_runcirrus();
    command.env("KARSKSAL_ROOT", &tmp);
    command.args(["-v", "other-version"]);
    command.arg(tmp.join("project/case.in"));

    let output = command.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    // Note that the Fake MPIRun still reports version 2.0.0 and not "other-version"
    let np = std::thread::available_parallelism().unwrap().get();
    assert_eq!(
        stdout,
        format!(
            "Fake MPIRun with version=2.0.0 np={}, machinefile=unset\nFake Cirrus 2.0.0 with args: -cirrusin {}\n",
            np,
            tmp.join("project/case.in").display()
        )
    );
    assert_eq!(stderr, "");
}
