{ config, ... }:

let

  clickhousePort = toString config.processes.clickhouse-server.ports.main.value;
  grpcEndpoint = config.services.opentelemetry-collector.settings.receivers.otlp.protocols.grpc.endpoint;

in {
  languages.rust.enable = true;

  env = {
    KARSKSAL_ROOT = "/prog/ccs/cirrus";
    RUNKARSK_OTEL_EXPORTER_OTLP_ENDPOINT = "http://${grpcEndpoint}";
  };

  services = {
    clickhouse.enable = true;

    opentelemetry-collector = {
      enable = true;

      settings = {
        receivers = {
          otlp = {
            protocols = {
              grpc.endpoint = "localhost:4317";
              http.endpoint = "localhost:4318";
            };
          };
        };

        processors = {
          batch = {
            timeout = "5s";
            send_batch_size = 100000;
          };
        };

        exporters = {
          clickhouse = {
            endpoint = "tcp://127.0.0.1:${clickhousePort}";
            database = "otel";
            ttl = "72h";
            logs_table_name = "otel_logs";
            traces_table_name = "otel_traces";
            metrics_table_name = "otel_metrics";
            timeout = "5s";
            retry_on_failure = {
              enabled = true;
              initial_interval = "5s";
              max_interval = "30s";
              max_elapsed_time = "300s";
            };
          };
        };

        service = {
          pipelines = {
            traces = {
              receivers = [ "otlp" ];
              processors = [ "batch" ];
              exporters = [ "clickhouse" ];
            };
          };

          telemetry = {
            logs = {
              level = "debug";
            };
          };
        };
      };
    };
  };

  processes.opentelemetry-collector.after = [ "devenv:processes:clickhouse-server" ];

  tasks."app:create-database" = {
    description = "Create the ClickHouse database before launching OpenTelemetry Collector";
    exec = ''
      clickhouse client --port ${clickhousePort} "CREATE DATABASE IF NOT EXISTS otel"
    '';
    after = [ "devenv:processes:clickhouse-server" ];
    before = [ "devenv:processes:opentelemetry-collector" ];
  };
}
