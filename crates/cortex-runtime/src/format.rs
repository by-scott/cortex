pub fn format_duration(total_secs: i64) -> String {
    if total_secs < 60 {
        return format!("{total_secs}s");
    }
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    if mins < 60 {
        return format!("{mins}m {secs}s");
    }
    let hours = mins / 60;
    let rem_mins = mins % 60;
    if hours < 24 {
        return format!("{hours}h {rem_mins}m");
    }
    let days = hours / 24;
    let rem_hours = hours % 24;
    format!("{days}d {rem_hours}h")
}

pub fn fmt_tokens(n: u64) -> String {
    if n < 1000 {
        format!("{n}")
    } else {
        let thousands = f64::from(u32::try_from(n / 1000).unwrap_or(u32::MAX));
        let hundreds = f64::from(u32::try_from(n % 1000).unwrap_or(999)) / 1000.0;
        format!("{:.1}k", thousands + hundreds)
    }
}
