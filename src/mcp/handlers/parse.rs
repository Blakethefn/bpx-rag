use anyhow::{anyhow, Result};
use serde_json::Value;

pub(super) fn string_list(args: &Value, key: &str) -> Result<Vec<String>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("'{key}' must contain only strings"))
            })
            .collect(),
        Some(_) => Err(anyhow!("'{key}' must be an array of strings")),
    }
}

pub(super) fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("'{key}' is required"))
}

pub(super) fn bounded_u32(args: &Value, key: &str, default: u32, min: u32, max: u32) -> Result<u32> {
    let value = match args[key].as_u64() {
        Some(value) => u32::try_from(value).map_err(|_| anyhow!("'{key}' is out of range"))?,
        None => default,
    };
    if !(min..=max).contains(&value) {
        return Err(anyhow!("'{key}' must be between {min} and {max}"));
    }
    Ok(value)
}

pub(super) fn optional_bounded_u32(args: &Value, key: &str) -> Result<Option<u32>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let number = value
                .as_u64()
                .ok_or_else(|| anyhow!("'{key}' must be a positive integer"))?;
            if number == 0 || number > u32::MAX as u64 {
                return Err(anyhow!("'{key}' must be between 1 and {}", u32::MAX));
            }
            Ok(Some(number as u32))
        }
    }
}

pub(super) fn bounded_usize(args: &Value, key: &str, default: usize, min: usize, max: usize) -> Result<usize> {
    let value = match args[key].as_u64() {
        Some(value) => usize::try_from(value).map_err(|_| anyhow!("'{key}' is out of range"))?,
        None => default,
    };
    if !(min..=max).contains(&value) {
        return Err(anyhow!("'{key}' must be between {min} and {max}"));
    }
    Ok(value)
}
