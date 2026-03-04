use anyhow::{Result, anyhow};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;

/// Initialize structured stderr logging for CLI lifecycle events.
///
/// # Errors
///
/// Returns an error if the global tracing subscriber was already initialized.
pub fn init(verbose: u8, quiet: bool) -> Result<()> {
    let level = level_from_flags(verbose, quiet);
    fmt()
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(false)
        .with_max_level(level)
        .try_init()
        .map_err(|err| anyhow!("failed to initialize logging: {err}"))?;
    Ok(())
}

#[must_use]
fn level_from_flags(verbose: u8, quiet: bool) -> LevelFilter {
    if quiet {
        return LevelFilter::ERROR;
    }

    match verbose {
        0 => LevelFilter::WARN,
        1 => LevelFilter::INFO,
        2 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbosity_maps_to_expected_levels() {
        assert_eq!(level_from_flags(0, false), LevelFilter::WARN);
        assert_eq!(level_from_flags(1, false), LevelFilter::INFO);
        assert_eq!(level_from_flags(2, false), LevelFilter::DEBUG);
        assert_eq!(level_from_flags(3, false), LevelFilter::TRACE);
        assert_eq!(level_from_flags(7, false), LevelFilter::TRACE);
    }

    #[test]
    fn quiet_overrides_verbose() {
        assert_eq!(level_from_flags(0, true), LevelFilter::ERROR);
        assert_eq!(level_from_flags(3, true), LevelFilter::ERROR);
    }
}
