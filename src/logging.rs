use anyhow::{Result, anyhow};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;

#[cfg(feature = "telemetry-otel")]
use opentelemetry::KeyValue;
#[cfg(feature = "telemetry-otel")]
use opentelemetry::global;
#[cfg(feature = "telemetry-otel")]
use opentelemetry::trace::TracerProvider as _;
#[cfg(feature = "telemetry-otel")]
use opentelemetry_otlp::WithExportConfig;
#[cfg(feature = "telemetry-otel")]
use opentelemetry_sdk::Resource;
#[cfg(feature = "telemetry-otel")]
use opentelemetry_sdk::trace::SdkTracerProvider;
#[cfg(feature = "telemetry-otel")]
use std::time::Duration;
#[cfg(feature = "telemetry-otel")]
use tokio::runtime::Runtime;
#[cfg(feature = "telemetry-otel")]
use tracing_subscriber::Layer;
#[cfg(feature = "telemetry-otel")]
use tracing_subscriber::layer::SubscriberExt;
#[cfg(feature = "telemetry-otel")]
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::TelemetryConfig;

pub struct LoggingHandle {
    #[cfg(feature = "telemetry-otel")]
    telemetry_runtime: Option<Runtime>,
    #[cfg(feature = "telemetry-otel")]
    telemetry_provider: Option<SdkTracerProvider>,
}

/// Initialize structured stderr logging for CLI lifecycle events.
///
/// # Errors
///
/// Returns an error if the global tracing subscriber was already initialized.
pub fn init(
    verbose: u8,
    quiet: bool,
    telemetry: Option<&TelemetryConfig>,
) -> Result<LoggingHandle> {
    let level = level_from_flags(verbose, quiet);
    let active_telemetry = telemetry.filter(|cfg| cfg.is_active());

    #[cfg(feature = "telemetry-otel")]
    if let Some(cfg) = active_telemetry {
        match init_telemetry_provider(cfg) {
            Ok((runtime, provider)) => {
                let tracer = provider.tracer("99problems");
                let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
                let otel_filter = tracing_subscriber::filter::filter_fn(|metadata| {
                    let target = metadata.target();
                    target == "99problems"
                        || target.starts_with("99problems::")
                        || target.starts_with("reqwest_tracing")
                });
                tracing_subscriber::registry()
                    .with(
                        fmt::layer()
                            .with_writer(std::io::stderr)
                            .without_time()
                            .with_target(false)
                            .with_filter(level),
                    )
                    .with(otel_layer.with_filter(otel_filter))
                    .try_init()
                    .map_err(|err| anyhow!("failed to initialize logging: {err}"))?;
                return Ok(LoggingHandle {
                    telemetry_runtime: Some(runtime),
                    telemetry_provider: Some(provider),
                });
            }
            Err(err) => {
                eprintln!("Warning: telemetry init failed; continuing without telemetry: {err}");
            }
        }
    }

    #[cfg(not(feature = "telemetry-otel"))]
    if active_telemetry.is_some() {
        eprintln!(
            "Warning: telemetry is configured, but this binary was built without telemetry support (feature 'telemetry-otel')."
        );
    }

    fmt()
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(false)
        .with_max_level(level)
        .try_init()
        .map_err(|err| anyhow!("failed to initialize logging: {err}"))?;

    #[cfg(feature = "telemetry-otel")]
    {
        Ok(LoggingHandle {
            telemetry_runtime: None,
            telemetry_provider: None,
        })
    }
    #[cfg(not(feature = "telemetry-otel"))]
    {
        Ok(LoggingHandle {})
    }
}

impl LoggingHandle {
    pub fn shutdown(&mut self) {
        #[cfg(feature = "telemetry-otel")]
        {
            let runtime = self.telemetry_runtime.take();
            let provider = self.telemetry_provider.take();
            if let (Some(runtime), Some(provider)) = (runtime, provider) {
                let _guard = runtime.enter();
                if let Err(err) = provider.shutdown() {
                    eprintln!("Warning: telemetry shutdown failed: {err}");
                }
            }
        }
    }
}

#[cfg(feature = "telemetry-otel")]
fn init_telemetry_provider(cfg: &TelemetryConfig) -> Result<(Runtime, SdkTracerProvider)> {
    let endpoint = cfg
        .otlp_endpoint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("telemetry.otlp_endpoint is empty"))?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("99problems-otel")
        .enable_all()
        .build()?;

    let _guard = runtime.enter();
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .build()?;
    let batch_config = opentelemetry_sdk::trace::BatchConfigBuilder::default()
        .with_max_export_timeout(Duration::from_millis(export_timeout_ms()))
        .build();
    let batch =
        opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor::builder(
            exporter,
            opentelemetry_sdk::runtime::Tokio,
        )
        .with_batch_config(batch_config)
        .build();
    let service_name = std::env::var("OTEL_SERVICE_NAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "99problems".to_string());
    let resource = Resource::builder_empty()
        .with_attributes([
            KeyValue::new("service.name", service_name),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ])
        .build();
    let provider = SdkTracerProvider::builder()
        .with_span_processor(batch)
        .with_resource(resource)
        .build();
    global::set_tracer_provider(provider.clone());

    Ok((runtime, provider))
}

#[cfg(feature = "telemetry-otel")]
fn export_timeout_ms() -> u64 {
    std::env::var("OTEL_BSP_EXPORT_TIMEOUT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1_000)
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
