use clap::CommandFactory as _;
use clap::error::ErrorKind;
use runkarsk::common_args::CommonArgs;

use runkarsk::common_args;
use runkarsk::queue_system::QueueSystem;
use runkarsk::spec::Spec;
use runkarsk::telemetry::Telemetry;
use runkarsk::util::CommandTracing;
use std::env;
use tracing::instrument;

#[instrument(skip(args), fields(request = %serde_json::to_string(&args).unwrap_or_else(|_| "<failed to serialize>".to_string())))]
async fn start(args: CommonArgs) {
    let spec = match Spec::new("cirrus", args.input.unwrap(), args.version) {
        Ok(spec) => spec,
        Err(err) => {
            eprintln!("{err}");
            tracing::error!("{}", err);
            return;
        }
    };

    let qs = QueueSystem::from_args(&args.queue_args)
        .map_err(|err| CommonArgs::command().error(ErrorKind::ValueValidation, err))
        .unwrap_or_else(|err| err.exit());

    let mpirun_path = spec.get_bin("mpirun").to_owned();
    let output_directory = args
        .output_directory
        .unwrap_or(spec.get_case_dir().to_owned());
    let mut command = qs.command();

    command
        .arg("-C")
        .arg(output_directory)
        .arg("-c")
        .arg(spec.get_case_name())
        .arg("-M")
        .arg(mpirun_path)
        .tracing();

    command
        .arg(spec.get_bin("cirrus"))
        .arg("-cirrusin")
        .arg(spec.get_input());

    qs.exec(command).await.unwrap();
}

#[tokio::main]
async fn main() {
    let args = common_args::parse(env!("CARGO_BIN_NAME"));

    let _telemetry = Telemetry::init(env!("CARGO_BIN_NAME"));

    start(args).await;
}
