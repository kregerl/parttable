#[macro_export]
macro_rules! log_todo {
    ($($arg:tt)*) => {
        log::debug!(target: module_path!(), "[{}:{}] {}: {}", file!(), line!(), "TODO", format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_fixme {
    ($($arg:tt)*) => {
        log::debug!(target: module_path!(), "[{}:{}] {}: {}", file!(), line!(), "FIXME", format_args!($($arg)*));
    };
}