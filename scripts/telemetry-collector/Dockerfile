# Build: docker build -t cx-telemetry-otelcol:0.165.0 .
FROM otel/opentelemetry-collector-builder:0.165.0 AS build
USER root
COPY builder.yaml /build/builder.yaml
RUN ["/usr/local/bin/ocb", "--config=/build/builder.yaml"]

FROM otel/opentelemetry-collector-contrib:0.165.0 AS certs

FROM scratch
COPY --from=certs /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /build/out/cx-telemetry-otelcol /otelcol
USER 10001
ENTRYPOINT ["/otelcol"]
