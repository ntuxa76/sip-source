# syntax=docker/dockerfile:1
FROM docker.io/library/node:22-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

FROM docker.io/library/rust:1.92-bookworm AS core
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=web /src/web/dist ./web/dist
RUN cargo build --release -p vpd

# The engine behind the Local file storage, stripped of debug symbols.
FROM docker.io/library/rust:1.92-bookworm AS engine
COPY --from=docker.io/clickhouse/clickhouse-server:25.8 /usr/bin/clickhouse /tmp/clickhouse
RUN strip --strip-all -o /clickhouse /tmp/clickhouse && rm /tmp/clickhouse && mkdir /data

FROM gcr.io/distroless/cc-debian12:nonroot
LABEL org.opencontainers.image.title="sip-source" \
      org.opencontainers.image.description="SIP/VoIP capture, troubleshooting and monitoring panel"
COPY --from=core /src/target/release/vpd /usr/local/bin/vpd
COPY --from=engine /clickhouse /usr/local/bin/clickhouse
COPY --from=engine --chown=65532:65532 /data /data
ENV SS_LISTEN=0.0.0.0:8080 SS_DATA_DIR=/data
EXPOSE 8080 9060/udp 9060/tcp
HEALTHCHECK --interval=15s --timeout=5s --start-period=60s --retries=4 CMD ["/usr/local/bin/vpd", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/vpd"]
