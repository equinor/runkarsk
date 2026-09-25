use std::cell::Cell;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;

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
