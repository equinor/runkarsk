use anyhow::{Result, anyhow};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::{Resource, trace::SdkTracerProvider};
use serde::Serialize;
use std::collections::HashMap;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer};

#[allow(unused_macros)]
macro_rules! config {
    ($name:literal, $error_prefix:literal) => {
        std::env::var(concat!("RUNKARSK_", $name))
            .ok()
            .or(option_env!($name).map(str::to_owned))
            .ok_or_else(|| {
                anyhow!(
                    "{}: Environment variable {:?} needs to be provided at compile-time or {:?} needs to be provided at run-time",
                    $error_prefix,
                    $name,
                    concat!("RUNKARSK_", $name),
                )
            })
    };
}

#[cfg(feature = "appinsights")]
fn add_appinsights_exporter(resource: Resource) -> Result<SdkTracerProvider> {
    // Standard naming for this environment variable
    //
    // https://learn.microsoft.com/en-us/azure/azure-monitor/app/opentelemetry-enable?tabs=python#modify-your-application
    let connection_string = config!(
        "APPLICATIONINSIGHTS_CONNECTION_STRING",
        "Application Insights support is disabled"
    )?;
    let exporter = opentelemetry_application_insights::Exporter::new_from_connection_string(
        connection_string,
        reqwest::blocking::Client::new(),
    )
    .map_err(|err| anyhow::Error::msg(err.to_string()))?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    Ok(provider)
}

#[cfg(not(feature = "appinsights"))]
fn add_appinsights_exporter(_resource: Resource) -> Result<SdkTracerProvider> {
    Err(anyhow!(
        "Application Insights support is disabled at build-time"
    ))
}

#[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
fn add_otlp_exporter(resource: Resource) -> Result<SdkTracerProvider> {
    let endpoint = config!(
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "OpenTelemetry Exporter is disabled"
    )?;
    let protocol = config!(
        "OTEL_EXPORTER_OTLP_PROTOCOL",
        "OpenTelemetry Exporter is disabled due to undefined protocol"
    )?;

    let exporter = match protocol.trim() {
        #[cfg(feature = "otlp-grpc")]
        "grpc" => {
            use opentelemetry_otlp::WithExportConfig as _;
            opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .with_protocol(opentelemetry_otlp::Protocol::Grpc)
                .build()
                .map_err(|err| anyhow!("{}", err))
        }

        #[cfg(feature = "otlp-http")]
        "http/protobuf" => {
            use opentelemetry_otlp::WithExportConfig as _;
            let url = format!("{}/v1/traces", endpoint.trim_end_matches('/'));
            opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_endpoint(url)
                .with_protocol(opentelemetry_otlp::Protocol::HttpBinary)
                .build()
                .map_err(|err| anyhow!("{}", err))
        }

        other => Err(anyhow!("unsupported protocol {other:?}")),
    }?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    Ok(provider)
}

#[cfg(not(any(feature = "otlp-grpc", feature = "otlp-http")))]
fn add_otlp_exporter(_resource: Resource) -> Result<SdkTracerProvider> {
    Err(anyhow!("OpenTelemetry Exporter is disabled at build-time"))
}

pub struct Telemetry {
    otel_provider: Option<SdkTracerProvider>,
}

impl Telemetry {
    pub fn init(service_name: &str) -> Self {
        let fmt_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("off"));

        let otel_filter = EnvFilter::new("info,otel::tracing=trace");

        let resource = Resource::builder_empty()
            .with_service_name(service_name.to_string())
            .build();

        let mut warnings = vec![];
        let mut have_otel_provider = false;
        let mut otel_provider = None;
        let mut otel_layer = None;
        for provider in [
            add_appinsights_exporter(resource.clone()),
            add_otlp_exporter(resource.clone()),
        ] {
            match (provider, have_otel_provider) {
                (Ok(provider), false) => {
                    let tracer = provider.tracer(service_name.to_string());
                    otel_layer = Some(
                        tracing_opentelemetry::layer()
                            .with_tracer(tracer)
                            .with_filter(otel_filter.clone()),
                    );
                    otel_provider = Some(provider);
                    have_otel_provider = true;
                }
                (Ok(_), _) => {}
                (Err(err), _) => {
                    warnings.push(format!("{}", err));
                }
            }
        }

        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .compact();

        tracing_subscriber::registry()
            .with(otel_layer)
            .with(fmt_layer.with_filter(fmt_filter))
            .init();

        for message in warnings {
            tracing::warn!("{}", message);
        }

        Self { otel_provider }
    }

    pub fn shutdown(&mut self) {
        let Some(provider) = self.otel_provider.take() else {
            return;
        };

        let finish = move || {
            if let Err(e) = provider.shutdown() {
                eprintln!("Failed to finalise telemetry: {e}");
            }
            drop(provider);
        };

        match tokio::runtime::Handle::try_current() {
            Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(finish);
            }
            Ok(_) => std::thread::scope(|s| {
                s.spawn(finish);
            }),
            Err(_) => finish(),
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

pub fn run_main<F, R>(service_name: &str, f: F) -> R::Output
where
    F: FnOnce() -> R,
    R: std::future::Future,
{
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to create Tokio runtime");

    let _telemetry = {
        let _entered = runtime.enter();
        Telemetry::init(service_name)
    };

    runtime.block_on(async move { f().await })
}
