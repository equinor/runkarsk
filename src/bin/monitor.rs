use runkarsk::telemetry::{self, log_tokio_command};
use tokio::process::Command;
use tracing::instrument;

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

fn main() {
    telemetry::run_main(env!("CARGO_BIN_NAME"), async || {
        let span = tracing::info_span!("main");
        let _entered = span.enter();
        telemetry::log_program_environment();
        start().await
    });
}
