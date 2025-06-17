use opentelemetry::global;
use opentelemetry::trace::Tracer;
use opentelemetry::trace::TracerProvider;
use opentelemetry::KeyValue;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    logs::SdkLoggerProvider, metrics::SdkMeterProvider, trace::SdkTracerProvider, Resource,
};
use std::sync::OnceLock;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

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

fn init_logs() -> SdkLoggerProvider {
    let exporter = LogExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()
        .expect("Failed to create log exporter");

    SdkLoggerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(get_resource())
        .build()
}

fn init_traces() -> SdkTracerProvider {
    let exporter = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()
        .expect("Failed to create trace exporter");

    SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(get_resource())
        .build()
}

fn init_metrics() -> SdkMeterProvider {
    let exporter = MetricExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()
        .expect("Failed to create metric exporter");

    SdkMeterProvider::builder()
        .with_periodic_exporter(exporter)
        .with_resource(get_resource())
        .build()
}

pub struct Telemetry {
    tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,
    logger_provider: SdkLoggerProvider,
}

impl Telemetry {
    pub fn init() -> Self {
        let logger_provider = init_logs();

        let otel_log_filter = EnvFilter::new("info")
            .add_directive("opentelemetry=off".parse().unwrap())
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("tonic=off".parse().unwrap())
            .add_directive("h2=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap());
        let otel_log_layer =
            OpenTelemetryTracingBridge::new(&logger_provider).with_filter(otel_log_filter);

        // let filter_fmt =
        //     EnvFilter::new("info").add_directive("opentelemetry=debug".parse().unwrap());
        // let fmt_layer = tracing_subscriber::fmt::layer()
        //     .with_thread_names(true)
        //     .with_filter(filter_fmt);

        let tracer_provider = init_traces();
        global::set_tracer_provider(tracer_provider.clone());

        let meter_provider = init_metrics();
        global::set_meter_provider(meter_provider.clone());

        let otel_span_filter = EnvFilter::new("info")
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("tonic=off".parse().unwrap())
            .add_directive("h2=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap());
        let otel_span_layer =
            OpenTelemetryLayer::new(tracer_provider.tracer("linkup")).with_filter(otel_span_filter);

        tracing_subscriber::registry()
            .with(otel_log_layer)
            // .with(fmt_layer)
            .with(otel_span_layer)
            .init();

        Self {
            logger_provider,
            meter_provider,
            tracer_provider,
        }
    }

    pub fn shutdown(self) {
        let mut shutdown_errors = Vec::new();
        if let Err(e) = self.tracer_provider.shutdown() {
            shutdown_errors.push(format!("tracer provider: {}", e));
        }

        if let Err(e) = self.meter_provider.shutdown() {
            shutdown_errors.push(format!("meter provider: {}", e));
        }

        if let Err(e) = self.logger_provider.shutdown() {
            shutdown_errors.push(format!("logger provider: {}", e));
        }

        if !shutdown_errors.is_empty() {
            eprintln!(
                "Failed to shutdown providers:{}",
                shutdown_errors.join("\n")
            );
        }
    }
}
