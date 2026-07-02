use super::Command;

/// Parse a u64 value that may be in hex (0x...) or decimal format.
pub(super) fn parse_u64_hex_or_decimal(s: &str) -> Result<u64, String> {
    let lower = s.to_lowercase();
    if let Some(hex_digits) = lower.strip_prefix("0x") {
        u64::from_str_radix(hex_digits, 16).map_err(|_| format!("Invalid hex value '{}'", s))
    } else {
        s.parse::<u64>()
            .map_err(|_| format!("Invalid value '{}': expected decimal or hex (0x...)", s))
    }
}

pub(super) fn parse_non_negative_limit(arg: &str) -> Result<isize, String> {
    let value = arg.parse::<isize>().map_err(|_| {
        format!(
            "Invalid limit '{}': limit must be a non-negative integer",
            arg
        )
    })?;
    if value < 0 {
        return Err(format!(
            "Invalid limit '{}': limit must be non-negative",
            arg
        ));
    }
    Ok(value)
}

pub(super) fn parse_positive_limit(arg: &str) -> Result<isize, String> {
    let value = parse_non_negative_limit(arg)?;
    if value == 0 {
        return Err(format!(
            "Invalid limit '{}': limit must be greater than 0",
            arg
        ));
    }
    Ok(value)
}

pub(super) fn parse_time_convert(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("time_convert requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut time_value = None;
    let mut time_unit = None;
    let mut time_index = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--time-value" | "-v" => {
                i += 1;
                if i < args.len() {
                    time_value = args[i].parse().ok();
                } else {
                    return Err("--time-value requires a number".to_string());
                }
            }
            "--time-unit" | "-u" => {
                i += 1;
                if i < args.len() {
                    time_unit = Some(args[i].clone());
                } else {
                    return Err("--time-unit requires a value (ps/ns/us/ms/s)".to_string());
                }
            }
            "--time-index" | "-i" => {
                i += 1;
                if i < args.len() {
                    time_index = args[i].parse().ok();
                } else {
                    return Err("--time-index requires a number".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for time_convert", args[i])),
        }
        i += 1;
    }

    // Validate: either time-value+time-unit or time-index must be provided
    if time_value.is_some() && time_unit.is_none() {
        return Err("--time-unit is required when using --time-value".to_string());
    }
    if time_value.is_some() && time_index.is_some() {
        return Err("Cannot use both --time-value and --time-index simultaneously".to_string());
    }
    if time_value.is_none() && time_index.is_none() {
        return Err(
            "Either --time-value (with --time-unit) or --time-index must be provided".to_string(),
        );
    }

    Ok(Command::TimeConvert {
        waveform_id,
        time_value,
        time_unit,
        time_index,
    })
}
