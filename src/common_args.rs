use clap::{CommandFactory, FromArgMatches, Parser};
use serde::Serialize;
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Parser, Serialize)]
pub struct CommonArgs {
    /// Cirrus .in input file
    #[arg(required_unless_present = "print_versions")]
    pub input: Option<PathBuf>,

    /// Job queue, or 'local' to run on this machine
    #[arg(short, long, default_value = "local")]
    pub queue: String,

    /// Number of tasks/processes per machine
    #[arg(short = 'n', long)]
    pub num_tasks_per_machine: Option<usize>,

    /// Number of machines (nodes)
    #[arg(short = 'm', long, default_value_t = 1)]
    pub num_machines: usize,

    /// Use 'local' job queue
    #[arg(short, long)]
    pub interactive: bool,

    /// Version of Cirrus to use
    #[arg(short = 'v', long)]
    pub version: Option<String>,

    /// Directory to store the output to
    #[arg(short = 'o', long)]
    pub output_directory: Option<PathBuf>,

    /// Exclusive node usage [default: shared]
    #[arg(short = 'e', long)]
    pub exclusive: bool,

    /// Output Cirrus versions and exit
    #[arg(long)]
    pub print_versions: bool,

    /// Print the command that would be executed
    #[arg(long)]
    pub dry_run: bool,

    /// OpenTelemetry Trace ID
    #[arg(long, hide = true)]
    pub otel_trace_id: Option<String>,

    /// OpenTelemetry parent span ID
    #[arg(long, hide = true)]
    pub otel_parent_span_id: Option<String>,
}

fn converted_deprecated_args() -> Vec<OsString> {
    env::args_os()
        .map(|arg| match arg.to_str() {
            Some("-nm") => OsString::from("-n"),
            Some("-nn") => OsString::from("-m"),
            _ => arg,
        })
        .collect()
}

pub fn parse(bin_name: &'static str) -> CommonArgs {
    let matches = CommonArgs::command_for_update()
        .bin_name(bin_name)
        .get_matches_from(converted_deprecated_args());
    CommonArgs::from_arg_matches(&matches).unwrap_or_else(|err| err.exit())
}
