use std::ffi;
#[cfg(feature = "log")]
use std::fs;

pub mod api;
pub mod channel;
#[cfg(feature = "frontend")]
pub mod frontend;
pub mod input;
mod rdp;
pub mod service;

#[cfg(feature = "service-clipboard")]
mod clipboard;
#[cfg(feature = "service-command")]
mod command;
#[cfg(feature = "service-ftp")]
mod ftp;
#[cfg(feature = "service-socks5")]
mod socks5;
#[cfg(feature = "service-stage0")]
mod stage0;

mod log;
#[cfg(feature = "backend")]
mod util;

pub const VIRTUAL_CHANNEL_NAME: &ffi::CStr = c"SOXY";

pub enum Level {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl<'a> TryFrom<&'a str> for Level {
    type Error = String;

    fn try_from(s: &'a str) -> Result<Self, <Self as TryFrom<&'a str>>::Error> {
        match s.to_uppercase().as_ref() {
            "OFF" => Ok(Self::Off),
            "ERROR" => Ok(Self::Error),
            "WARN" | "WARNING" => Ok(Self::Warn),
            "INFO" => Ok(Self::Info),
            "DEBUG" => Ok(Self::Debug),
            "TRACE" => Ok(Self::Trace),
            _ => Err("invalid log level".into()),
        }
    }
}

#[cfg(not(feature = "log"))]
pub const fn init_logs(_level: Level, _file: Option<&String>) {}

#[cfg(feature = "log")]
impl From<Level> for simplelog::LevelFilter {
    fn from(level: Level) -> Self {
        match level {
            Level::Off => simplelog::LevelFilter::Off,
            Level::Error => simplelog::LevelFilter::Error,
            Level::Warn => simplelog::LevelFilter::Warn,
            Level::Info => simplelog::LevelFilter::Info,
            Level::Debug => simplelog::LevelFilter::Debug,
            Level::Trace => simplelog::LevelFilter::Trace,
        }
    }
}

#[cfg(feature = "log")]
pub fn init_logs(level: Level, file: Option<&String>) {
    let level_filter = level.into();

    let config = simplelog::ConfigBuilder::new()
        .set_level_padding(simplelog::LevelPadding::Right)
        .set_target_level(simplelog::LevelFilter::Off)
        .set_thread_level(simplelog::LevelFilter::Error)
        .set_thread_mode(simplelog::ThreadLogMode::Names)
        .set_time_format_rfc2822()
        .build();

    let mut loggers: Vec<Box<dyn simplelog::SharedLogger>> = vec![simplelog::TermLogger::new(
        level_filter,
        config.clone(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    )];

    if let Some(file) = file {
        if let Ok(file) = fs::File::options()
            .create(true)
            .append(false)
            .truncate(true)
            .write(true)
            .open(file)
        {
            loggers.push(simplelog::WriteLogger::new(level_filter, config, file));
        }
    }

    let _ = simplelog::CombinedLogger::init(loggers);
}
