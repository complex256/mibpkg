/// println! that never panics: when the reader of stdout is gone (the web server restarting
/// during a mibpkg self-update closes the pipe), an action must still finish its work.
#[macro_export]
macro_rules! say {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

/// eprintln! that never panics (see `say!`).
#[macro_export]
macro_rules! warn {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($t)*);
    }};
}

pub mod fsutil;
pub mod paths;
pub mod manifest;
pub mod archive;
pub mod index;
pub mod unit;
pub mod ctx;
pub mod lock;
pub mod sys;
pub mod backups;
pub mod state;
pub mod hook;
pub mod bootd;
pub mod config;
pub mod media;
pub mod control;
pub mod install;
pub mod legacy;
pub mod status;
pub mod supervisor;
pub mod gem;
pub mod cli;
pub mod conditions;
pub mod persist;
pub mod clock;
pub mod fetch;
pub mod online;
pub mod catalog;
pub mod firewall;
pub mod web;
