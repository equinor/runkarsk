use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::{Resource, trace::SdkTracerProvider};
use tracing::Subscriber;
use tracing::field::Field;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt as _;

struct SimpleConsoleFormat {
    name: String,
}

impl<S, N> FormatEvent<S, N> for SimpleConsoleFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let level = event.metadata().level();

        let (white, color, reset) = if writer.has_ansi_escapes() {
            use tracing::Level;
            let color = match *level {
                Level::ERROR => "\x1b[1;31m",
                Level::WARN => "\x1b[1;33m",
                Level::INFO => "\x1b[34m",
                Level::DEBUG => "\x1b[37m",
                Level::TRACE => "\x1b[35m",
            };

            ("\x1b[1;37m", color, "\x1b[0m")
        } else {
            ("", "", "")
        };

        let mut message = String::new();
        event.record(&mut |field: &Field, value: &dyn std::fmt::Debug| {
            if field.name() == "message" {
                message = format!("{:?}", value);
            }
        });

        writeln!(
            writer,
            "{white}{}{reset} ({color}{level}{reset}) {message}",
            self.name,
        )
    }
}

pub struct Telemetry {
    provider: Option<SdkTracerProvider>,
}

impl Telemetry {
    pub fn init(service_name: &str) -> Self {
        let fmt_layer = tracing_subscriber::fmt::layer().event_format(SimpleConsoleFormat {
            name: service_name.to_string(),
        });

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
                .with(fmt_layer)
                .with(otel_layer)
                .init();
            return Self {
                provider: Some(provider),
            };
        } else {
            let env_filter = EnvFilter::from_default_env();

            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .init();
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
