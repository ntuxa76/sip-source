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

FROM gcr.io/distroless/cc-debian12:nonroot
LABEL org.opencontainers.image.title="voip-panel" \
      org.opencontainers.image.description="SIP/VoIP capture, troubleshooting and monitoring panel"
COPY --from=core /src/target/release/vpd /usr/local/bin/vpd
ENV VP_LISTEN=0.0.0.0:8080
EXPOSE 8080 9060/udp 9060/tcp
HEALTHCHECK --interval=15s --timeout=5s --start-period=60s --retries=4 CMD ["/usr/local/bin/vpd", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/vpd"]
