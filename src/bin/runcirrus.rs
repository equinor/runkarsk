use opentelemetry::propagation::TextMapPropagator;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use runkarsk::common_args::CommonArgs;
use runkarsk::config;

use runkarsk::common_args;
use runkarsk::queue_system::QueueSystem;
use runkarsk::spec::Spec;
use runkarsk::telemetry::Telemetry;
use std::collections::HashMap;
use std::env;
use std::process::Command;
use tracing::instrument;

#[instrument(skip(args), fields(request = %serde_json::to_string(&args).unwrap_or_else(|_| "<failed to serialize>".to_string())))]
async fn start(args: CommonArgs) {
    let spec = match Spec::new(args.input.unwrap(), args.version) {
        Ok(spec) => spec,
        Err(err) => {
            tracing::error!("{}", err);
            return;
        }
    };

    let qs = QueueSystem::from_args(args.queue, args.num_tasks_per_machine, args.num_machines);

    let mpirun_path = spec.get_bin("mpirun").to_owned();
    let output_directory = args
        .output_directory
        .unwrap_or(spec.get_case_dir().to_owned());
    let num_tasks = qs.num_tasks();
    let mut command = Command::new(config::runner_path());

    command
        .arg("-C")
        .arg(output_directory)
        .arg("-c")
        .arg(spec.get_case_name())
        .arg("-n")
        .arg(num_tasks.to_string())
        .arg("-M")
        .arg(mpirun_path);

    let mut fields = HashMap::new();
    TraceContextPropagator::new().inject(&mut fields);
    command.envs(fields);

    command
        .arg(spec.get_bin("cirrus"))
        .arg("-cirrusin")
        .arg(spec.get_input());

    qs.exec(command).await;
}

#[tokio::main]
async fn main() {
    let args = common_args::parse(env!("CARGO_BIN_NAME"));

    let _telemetry = Telemetry::init(env!("CARGO_BIN_NAME"));

    start(args).await;
}
