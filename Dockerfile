# syntax=docker/dockerfile:1

FROM rust:1.91-bookworm AS builder
WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY src ./src
COPY .env ./.env

ARG DATABASE_URL
ENV DATABASE_URL=${DATABASE_URL}

# Regenera o codigo em generated/ antes de compilar.
# Se DATABASE_URL nao estiver disponivel no build, este passo fica apenas como aviso.
RUN if [ -n "$DATABASE_URL" ]; then cargo run -p schema-reflector --bin generate; else echo "DATABASE_URL not set, skipping generated refresh"; fi

COPY generated ./generated
RUN cargo build --release --locked --bin flux-generated

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /workspace/target/release/flux-generated /usr/local/bin/flux-generated

EXPOSE 3000
CMD ["flux-generated"]
