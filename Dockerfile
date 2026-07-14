FROM rust:1.85-bookworm AS builder

WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY core/Cargo.toml core/Cargo.toml
COPY proxy/Cargo.toml proxy/Cargo.toml
COPY cli/Cargo.toml cli/Cargo.toml
COPY core/src core/src
COPY proxy/src proxy/src
COPY cli/src cli/src

RUN cargo build --release --package freeclaude-proxy

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 freeclaude

COPY --from=builder /src/target/release/freeclaude-proxy /usr/local/bin/freeclaude-proxy

USER freeclaude
EXPOSE 3000
ENV FREECLAUDE_PROXY_PORT=3000

HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:3000/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/freeclaude-proxy"]
