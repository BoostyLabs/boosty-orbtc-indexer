# ---- BUILDER IMAGE ----
FROM rust:alpine AS builder

# Diesel ORM depends on libpq, libpq depends on libssl.
# Unfortunately rustc doesn't want to staticly link openssl.
# I haven't found any suitable solution yet.
# At least we have pretty small alpine image now.
ARG RUSTFLAGS="-C target-feature=-crt-static"

WORKDIR /build
RUN apk add musl-dev pkgconfig libpq-dev \
    && rustup target add x86_64-unknown-linux-musl \
    && cargo install diesel_cli --no-default-features --features postgres --force \
    && cp $(which diesel) ./diesel

WORKDIR /opt/app
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl -p btc-indexer \
    && cp ./target/x86_64-unknown-linux-musl/release/btc-indexer ./indexer

# ---- RUNNER IMAGE ----

FROM alpine:latest

WORKDIR /app
RUN chown nobody /app

RUN apk add --no-cache musl-dev libpq-dev
COPY --from=builder --chown=nobody:root /build/ /app/bin/
COPY --from=builder --chown=nobody:root /opt/app/indexer /app/

USER nobody

EXPOSE 3000

CMD ["/app/indexer"]
