use std::env;

use chrono::{Datelike, Local, TimeZone, Timelike};

fn main() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-env-changed=RXIU_BUILD_TIME");

    let timestamp = if let Ok(custom) = env::var("RXIU_BUILD_TIME") {
        parse_seconds(&custom).unwrap_or_else(current_timestamp)
    } else if let Ok(epoch) = env::var("SOURCE_DATE_EPOCH") {
        parse_seconds(&epoch).unwrap_or_else(current_timestamp)
    } else {
        current_timestamp()
    };

    let build_version = format_version(timestamp);
    println!("cargo:rustc-env=RXIU_BUILD_VERSION=V{}", build_version);
}

fn current_timestamp() -> i64 {
    Local::now().timestamp()
}

fn parse_seconds(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok()
}

fn format_version(timestamp: i64) -> String {
    let dt = Local
        .timestamp_opt(timestamp, 0)
        .single()
        .unwrap_or_else(|| Local.timestamp_opt(0, 0).unwrap());
    let year = (dt.year() % 100) as i32;
    format!(
        "{:02}-{:02}{:02}.{:02}{:02}",
        year,
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute()
    )
}
