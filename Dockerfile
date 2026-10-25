FROM rust:1.99-slim AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && printf 'fn main() {}\n' > src/main.rs \
    && printf '' > src/lib.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
COPY templates ./templates
COPY static ./static
RUN cargo clean -p llibrary || true
RUN cargo build --release --locked

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates poppler-utils wget \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --uid 10001 --create-home llibrary

WORKDIR /app
COPY --from=builder /app/target/release/llibrary /usr/local/bin/llibrary
COPY --from=builder /app/static /app/static
COPY --from=builder /app/templates /app/templates
RUN mkdir -p /data && chown -R llibrary:llibrary /data /app

USER llibrary
ENV DATA_DIR=/data \
    DB_PATH=/data/library.db \
    BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["llibrary"]
CMD ["serve"]
