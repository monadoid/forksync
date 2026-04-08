use anyhow::{Context, Result};
use opentelemetry::trace::TracerProvider;
use opentelemetry::{KeyValue, global};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, SpanExporter};
use opentelemetry_sdk::logs::{BatchLogProcessor, SdkLoggerProvider};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::{Resource, trace as sdktrace};
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, Registry};

pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(logger_provider) = self.logger_provider.take() {
            let _ = logger_provider.shutdown();
        }
        if let Some(tracer_provider) = self.tracer_provider.take() {
            let _ = tracer_provider.shutdown();
        }
    }
}

pub fn init_telemetry(verbose: bool, json_logs: bool) -> Result<TelemetryGuard> {
    let otel_enabled = std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some();
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(if verbose {
            "debug"
        } else if json_logs || otel_enabled {
            "info"
        } else {
            "off"
        })
    });

    let resource = Resource::builder_empty()
        .with_attributes([
            KeyValue::new("service.name", "forksync"),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ])
        .build();

    let fmt_layer = if verbose || json_logs {
        Some(if json_logs {
            fmt::layer()
                .json()
                .with_target(false)
                .with_current_span(false)
                .with_span_list(false)
                .boxed()
        } else {
            fmt::layer()
                .compact()
                .with_target(false)
                .with_thread_names(false)
                .without_time()
                .boxed()
        })
    } else {
        None
    };

    if otel_enabled {
        global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );

        let tracer_provider = init_tracer_provider(resource.clone())?;
        let tracer = tracer_provider.tracer("forksync");
        let logger_provider = init_logger_provider(resource)?;
        let log_layer = OpenTelemetryTracingBridge::new(&logger_provider);

        Registry::default()
            .with(env_filter)
            .with(fmt_layer)
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .with(log_layer)
            .init();

        Ok(TelemetryGuard {
            tracer_provider: Some(tracer_provider),
            logger_provider: Some(logger_provider),
        })
    } else {
        Registry::default().with(env_filter).with(fmt_layer).init();

        Ok(TelemetryGuard {
            tracer_provider: None,
            logger_provider: None,
        })
    }
}

fn init_tracer_provider(resource: Resource) -> Result<SdkTracerProvider> {
    let exporter = SpanExporter::builder()
        .with_tonic()
        .build()
        .context("build OTLP span exporter")?;

    Ok(sdktrace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build())
}

fn init_logger_provider(resource: Resource) -> Result<SdkLoggerProvider> {
    let exporter = LogExporter::builder()
        .with_tonic()
        .build()
        .context("build OTLP log exporter")?;

    Ok(SdkLoggerProvider::builder()
        .with_log_processor(BatchLogProcessor::builder(exporter).build())
        .with_resource(resource)
        .build())
}
