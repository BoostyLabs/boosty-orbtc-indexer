set dotenv-load := true

LINUX_IMAGE := "orbtc-indexer-linux"
IMAGE := "orbtc-indexer"

default: list

list:
    just --list

install_dev_tools:
    cargo install cargo-insta --locked --force
    cargo install cargo-outdated --locked --force
    cargo install taplo-cli --locked --force
    cargo install cargo-machete
    cargo install sqlx-cli --no-default-features --features postgres --locked --force
    pip install -U pre-commit

build:
    cargo build --release -p orbtc
    cp target/release/orbtc ./bin/
    cp target/release/orbtc-api ./bin/
    cp target/release/orbtc-indexer ./bin/

build-docker-image:
    docker build -t {{IMAGE}} -f Dockerfile .

check_deps:
    cargo machete
    cargo outdated -R

check:
    cargo fmt --check
    cargo check --all --all-targets
    cargo clippy --all-targets -- -D warnings

fmt-cargo:
    taplo fmt -c .taplo.conf

fix: fmt-cargo
    cargo fmt --all
    cargo clippy --fix --allow-staged --allow-dirty --all-targets -- -D warnings
    cargo check --all --all-targets
    pre-commit run --all-files

test:
    ulimit -n 65536 && cargo test
