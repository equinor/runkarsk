use std::collections::HashMap;

use opentelemetry::propagation::TextMapPropagator as _;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use runkarsk::telemetry::{self, Telemetry, log_tokio_command};
use tokio::process::Command;
use tracing::instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

#[instrument]
async fn start() -> i32 {
    let args_os: Vec<_> = std::env::args_os().skip(1).collect();

    let mut command = Command::new(&args_os[0]);

    command.args(&args_os[1..]);

    log_tokio_command(&command);

    command
        .spawn()
        .unwrap()
        .wait()
        .await
        .unwrap()
        .code()
        .unwrap()
}

#[tokio::main]
async fn main() {
    let telemetry = Telemetry::init(env!("CARGO_BIN_NAME"));

    let carrier: HashMap<_, _> = ["traceparent", "tracestate"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)))
        .collect();

    let parent = TraceContextPropagator::new().extract(&carrier);
    let code = {
        let span = tracing::info_span!("main");
        if !carrier.is_empty() {
            span.set_parent(parent).unwrap();
        }
        let _entered = span.enter();
        telemetry::log_program_environment();
        start().await
    };
    telemetry.shutdown();
    std::process::exit(code);
}
