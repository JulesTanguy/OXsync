#[macro_export]
macro_rules! get_timestamp {
    () => {{
        // Ensure the `time` crate is in scope where the macro is used
        let now =
            time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        let format =
            time::format_description::parse("[hour]:[minute]:[second].[subsecond digits:3]")
                .unwrap();
        let timestamp = now.format(&format).unwrap();
        timestamp
    }};
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {{
        let timestamp = $crate::get_timestamp!();
        println!("{} INFO {}", timestamp, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! err {
    ($($arg:tt)*) => {{
        let timestamp = $crate::get_timestamp!();
        println!("{} ERROR {}", timestamp, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
        let timestamp = $crate::get_timestamp!();
        println!("{} WARN {}", timestamp, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {{
        if *LOG_TRACE.get().unwrap() {
            let timestamp = $crate::get_timestamp!();
            println!("{} TRACE {}", timestamp, format_args!($($arg)*));
        }
    }};
}
