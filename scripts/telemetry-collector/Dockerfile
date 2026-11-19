# Build: docker build -t cx-telemetry-otelcol:0.165.0 .
# Both base images are pinned by digest: a tag can be pushed again. To bump the
# version, take the new digests from
# `docker buildx imagetools inspect otel/opentelemetry-collector-builder:<version>`.
FROM otel/opentelemetry-collector-builder:0.165.0@sha256:2c15b2e66f09577f897a0106a13a36c3040e4329396a04a8814c05513891fd7a AS build
USER root
COPY builder.yaml /build/builder.yaml
RUN ["/usr/local/bin/ocb", "--config=/build/builder.yaml"]

FROM otel/opentelemetry-collector-contrib:0.165.0@sha256:e40d61f10c9a617cca1ffd1be4d7e2b3a4b0f0001798ff07e58cabe82e618cea AS certs

FROM scratch
COPY --from=certs /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /build/out/cx-telemetry-otelcol /otelcol
USER 10001
ENTRYPOINT ["/otelcol"]
