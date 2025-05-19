use serde::Serialize;

/// Print JSON to stdout.
pub fn print_json<T: Serialize>(value: &T) -> Result<(), serde_json::Error> {
    let json = serde_json::to_string(value)?;
    println!("{}", json);
    Ok(())
}

/// Print pretty JSON to stdout.
pub fn print_pretty_json<T: Serialize>(value: &T) -> Result<(), serde_json::Error> {
    let json = serde_json::to_string_pretty(value)?;
    println!("{}", json);
    Ok(())
}

/// Print JSON to stderr.
pub fn eprint_json<T: Serialize>(value: &T) -> Result<(), serde_json::Error> {
    let json = serde_json::to_string(value)?;
    eprintln!("{}", json);
    Ok(())
}

/// Print pretty JSON to stderr.
pub fn eprint_pretty_json<T: Serialize>(value: &T) -> Result<(), serde_json::Error> {
    let json = serde_json::to_string_pretty(value)?;
    eprintln!("{}", json);
    Ok(())
}
