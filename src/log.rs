use core::fmt;

/// Kernel logging with levels, wall-clock time, and uptime.
/// VGA: [I] message         — clean, no timestamps
/// Log: [12:34:56] [0.000] [I] message  — full info

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}

impl LogLevel {
    fn prefix(self) -> &'static str {
        match self {
            LogLevel::Info => "I",
            LogLevel::Warn => "W",
            LogLevel::Error => "E",
            LogLevel::Debug => "D",
        }
    }
}

pub fn log(level: LogLevel, args: fmt::Arguments) {
    let level_str = level.prefix();

    // VGA — clean output, no timestamps
    crate::vga::write_fmt(format_args!("[{}] {}\n", level_str, args));

    // Serial/kernel.log — full timestamps
    let t = crate::rtc::read();
    let (sec, ms) = crate::timer::elapsed();
    crate::serial::write_fmt(format_args!(
        "[{:02}:{:02}:{:02}] [{}.{:03}] [{}] {}\n",
        t.hours, t.minutes, t.seconds, sec, ms, level_str, args
    ));
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log::log($crate::log::LogLevel::Info, format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::log::log($crate::log::LogLevel::Warn, format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::log::log($crate::log::LogLevel::Error, format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::log::log($crate::log::LogLevel::Debug, format_args!($($arg)*));
    };
}
