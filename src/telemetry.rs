use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::{Resource, trace::SdkTracerProvider};
use serde::Serialize;
use std::collections::HashMap;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

pub struct Telemetry {
    provider: Option<SdkTracerProvider>,
}

impl Telemetry {
    pub fn init(service_name: &str) -> Self {
        let otlp_endpoint_var = std::env::var("RUNKARSK_OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .or(option_env!("OTEL_EXPORTER_OTLP_ENDPOINT").map(|s| s.to_owned()));

        if let Some(otlp_endpoint) = otlp_endpoint_var {
            let env_filter = EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,otel::tracing=trace"));

            let resource = Resource::builder_empty()
                .with_service_name(service_name.to_string())
                .build();

            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_tonic() // OTLP over gRPC
                .with_endpoint(otlp_endpoint)
                .with_protocol(opentelemetry_otlp::Protocol::Grpc)
                .build()
                .expect("Failed to create OTLP exporter");

            let provider = SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource)
                .build();

            let tracer = provider.tracer(service_name.to_string());

            let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

            tracing_subscriber::registry()
                .with(env_filter)
                .with(otel_layer)
                .init();
            return Self {
                provider: Some(provider),
            };
        } else {
            let env_filter = EnvFilter::from_default_env();

            tracing_subscriber::registry().with(env_filter).init();
        };

        Self { provider: None }
    }

    pub fn with(service_name: &str, f: impl FnOnce()) {
        let _telemetry_guard = Self::init(service_name);
        let span = tracing::error_span!("main");
        let _span_guard = span.enter();
        f();
    }

    pub fn shutdown(&self) {
        if let Some(ref provider) = self.provider
            && let Err(e) = provider.shutdown()
        {
            eprintln!("Failed to finalise telemetry: {e}");
        }
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn log_command(command: &std::process::Command) {
    tracing::info!(
        program = ?command.get_program(),
        args = ?command.get_args().collect::<Vec<_>>(),
        envs = ?command.get_envs().collect::<Vec<_>>(),
        cwd = ?command.get_current_dir(),
        "command"
    );
}

fn to_json<T: Serialize>(data: &T) -> String {
    serde_json::to_string(data).unwrap_or_else(|_| "<fail>".to_string())
}

pub fn log_tokio_command(command: &tokio::process::Command) {
    log_command(command.as_std());
}

pub fn log_program_environment() {
    let program = std::env::current_exe().unwrap_or_else(|_| "<unknown>".into());
    let args = std::env::args().collect::<Vec<_>>();
    let envs = std::env::vars().collect::<HashMap<_, _>>();

    tracing::info!(
        arg0 = %to_json(&program),
        args = %to_json(&args),
        envs = %to_json(&envs),
        "program environment"
    );
}
