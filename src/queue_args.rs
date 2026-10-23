use std::num::NonZero;

use crate::queue_system::QueueSystemKind;
use clap::Parser;
use serde::Serialize;

#[derive(Debug, Parser, Serialize)]
pub struct QueueArgs {
    /// Queue system to use, usually one of: 'lsf', 'pbs'. Leave unset to run on this machine
    #[arg(short = 'Q', long, value_enum)]
    pub queue_system: Option<QueueSystemKind>,

    /// Job queue, or 'local' to run on this machine
    #[arg(short, long)]
    pub queue: Option<String>,

    /// Number of tasks/processes per machine
    #[arg(short = 'n', long)]
    pub num_tasks_per_machine: Option<NonZero<usize>>,

    /// Number of machines (nodes)
    #[arg(short = 'm', long)]
    pub num_machines: Option<NonZero<usize>>,

    /// Use 'local' job queue
    #[arg(short, long)]
    pub interactive: bool,
}

impl QueueArgs {
    pub fn is_local(&self) -> bool {
        self.queue_system.is_none() && (self.queue.is_none() || self.queue == Some("local".into()))
    }
}
