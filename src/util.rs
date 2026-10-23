use std::cell::Cell;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;

use opentelemetry::propagation::TextMapPropagator as _;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use std::process::Command;
use tokio::process::Command as AsyncCommand;

thread_local! {
    pub static BIN_NAME: Cell<&'static str> = const { Cell::new("runkarsk") };
}

#[macro_export]
macro_rules! exit {
    (code=$code:expr, $($arg:tt)*) => {{
        tracing::error!($($arg)*);
        std::process::exit($code);
    }};

    ($($arg:tt)*) => {
        exit!(code=1, $($arg)*)
    };
}

/// Check if a Linux kernel module is loaded. Return
pub fn have_linux_module(name: &str) -> bool {
    if cfg!(target_os = "linux") {
        let file = File::open("/proc/modules").expect("Couldn't open /proc/modules for reading");
        let reader = BufReader::new(file);

        return reader
            .lines()
            .map_while(Result::ok)
            .any(|line| line.split_whitespace().next() == Some(name));
    }

    false
}

pub trait CommandTracing {
    fn tracing(&mut self) -> &mut Self;
}

impl CommandTracing for Command {
    fn tracing(&mut self) -> &mut Self {
        let mut fields = HashMap::new();
        TraceContextPropagator::new().inject(&mut fields);
        self.envs(fields);
        self
    }
}

impl CommandTracing for AsyncCommand {
    fn tracing(&mut self) -> &mut Self {
        let mut fields = HashMap::new();
        TraceContextPropagator::new().inject(&mut fields);
        self.envs(fields);
        self
    }
}
