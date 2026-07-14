FROM rust:1.85-bookworm AS builder

WORKDIR /src

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libfontconfig1-dev \
        libgl1-mesa-dev \
        libwayland-dev \
        libx11-dev \
        libxkbcommon-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY build.rs ./

RUN cargo build --release --bin freeclaude-proxy

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
