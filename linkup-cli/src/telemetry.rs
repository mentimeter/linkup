use opentelemetry::global;
use opentelemetry::trace::TracerProvider;
use opentelemetry::KeyValue;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithHttpConfig;
use opentelemetry_otlp::{LogExporter, MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    logs::SdkLoggerProvider, metrics::SdkMeterProvider, trace::SdkTracerProvider, Resource,
};
use std::sync::OnceLock;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

use crate::local_config::OtelConfig;

fn get_resource() -> Resource {
    static RESOURCE: OnceLock<Resource> = OnceLock::new();

    RESOURCE
        .get_or_init(|| {
            Resource::builder()
                .with_service_name("linkup")
                .with_attributes(vec![KeyValue::new(
                    "service.version",
                    env!("CARGO_PKG_VERSION"),
                )])
                .build()
        })
        .clone()
}

fn init_logs(config: &OtelConfig) -> SdkLoggerProvider {
    let endpoint = format!("{}/v1/logs", config.exporter_otlp_endpoint);

    let mut exporter_builder = LogExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .with_protocol(Protocol::HttpBinary);

    if let Some(headers) = &config.exporter_otlp_headers {
        exporter_builder = exporter_builder.with_headers(headers.clone());
    }

    let exporter = exporter_builder
        .build()
        .expect("Failed to create log exporter");

    SdkLoggerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(get_resource())
        .build()
}

fn init_traces(config: &OtelConfig) -> SdkTracerProvider {
    let endpoint = format!("{}/v1/traces", config.exporter_otlp_endpoint);

    let mut exporter_builder = SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .with_protocol(Protocol::HttpBinary);

    if let Some(headers) = &config.exporter_otlp_headers {
        exporter_builder = exporter_builder.with_headers(headers.clone());
    }

    let exporter = exporter_builder
        .build()
        .expect("Failed to create log exporter");

    SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(get_resource())
        .build()
}

fn init_metrics(config: &OtelConfig) -> SdkMeterProvider {
    let endpoint = format!("{}/v1/metrics", config.exporter_otlp_endpoint);

    let mut exporter_builder = MetricExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .with_protocol(Protocol::HttpBinary);

    if let Some(headers) = &config.exporter_otlp_headers {
        exporter_builder = exporter_builder.with_headers(headers.clone());
    }

    let exporter = exporter_builder
        .build()
        .expect("Failed to create log exporter");

    SdkMeterProvider::builder()
        .with_periodic_exporter(exporter)
        .with_resource(get_resource())
        .build()
}

#[derive(Default)]
pub struct Telemetry {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl Telemetry {
    pub fn init(otel_config: Option<OtelConfig>) -> Self {
        if let Some(otel_config) = otel_config {
            let logger_provider = init_logs(&otel_config);
            let tracer_provider = init_traces(&otel_config);
            let meter_provider = init_metrics(&otel_config);

            global::set_tracer_provider(tracer_provider.clone());
            global::set_meter_provider(meter_provider.clone());

            let otel_log_filter = EnvFilter::new("info")
                .add_directive("opentelemetry=off".parse().unwrap())
                .add_directive("hyper=off".parse().unwrap())
                .add_directive("tonic=off".parse().unwrap())
                .add_directive("h2=off".parse().unwrap())
                .add_directive("reqwest=off".parse().unwrap());
            let otel_log_layer =
                OpenTelemetryTracingBridge::new(&logger_provider).with_filter(otel_log_filter);

            let otel_span_filter = EnvFilter::new("info")
                .add_directive("hyper=off".parse().unwrap())
                .add_directive("tonic=off".parse().unwrap())
                .add_directive("h2=off".parse().unwrap())
                .add_directive("reqwest=off".parse().unwrap());
            let otel_span_layer = OpenTelemetryLayer::new(tracer_provider.tracer("linkup"))
                .with_filter(otel_span_filter);

            let fmt_layer = EnvFilter::try_from_default_env().ok().map(|e| {
                let filter = e.add_directive("opentelemetry=debug".parse().unwrap());
                tracing_subscriber::fmt::layer()
                    .with_thread_names(true)
                    .with_filter(filter)
            });

            let subscriber = tracing_subscriber::registry()
                .with(otel_log_layer)
                .with(otel_span_layer)
                .with(fmt_layer);

            subscriber.init();

            Self {
                tracer_provider: Some(tracer_provider),
                meter_provider: Some(meter_provider),
                logger_provider: Some(logger_provider),
            }
        } else {
            let fmt_layer = EnvFilter::try_from_default_env().ok().map(|e| {
                let filter = e.add_directive("opentelemetry=debug".parse().unwrap());
                tracing_subscriber::fmt::layer()
                    .with_thread_names(true)
                    .with_filter(filter)
            });

            let subscriber = tracing_subscriber::registry().with(fmt_layer);
            subscriber.init();

            Self::default()
        }
    }

    pub fn shutdown(self) {
        let mut shutdown_errors = Vec::new();

        if let Some(tracer_provider) = self.tracer_provider {
            if let Err(e) = tracer_provider.shutdown() {
                shutdown_errors.push(format!("tracer provider: {}", e));
            }
        }

        if let Some(meter_provider) = self.meter_provider {
            if let Err(e) = meter_provider.shutdown() {
                shutdown_errors.push(format!("meter provider: {}", e));
            }
        }

        if let Some(logger_provider) = self.logger_provider {
            if let Err(e) = logger_provider.shutdown() {
                shutdown_errors.push(format!("logger provider: {}", e));
            }
        }

        if !shutdown_errors.is_empty() {
            eprintln!(
                "Failed to shutdown providers:{}",
                shutdown_errors.join("\n")
            );
        }
    }
}
