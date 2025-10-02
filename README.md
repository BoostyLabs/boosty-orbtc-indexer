# OrBTC Indexer

### Table of Contents

- [Build](#build)
- [Prerequisites](#prerequisites)
- [Setting up Local Dev Environment](#local-dev-environment)
- [Technical Architecture](docs/technical-architecture.md)

## Build

1. Setup `rust` language env.
2. Build service using [just](https://github.com/casey/just).
3. Get the release binary.

```sh
just build
```

## Prerequisites

Running `indexer` service requires the following external dependencies:
- Docker
- Docker Compose
- **PostgreSQL**
- **Bitcoin RPC** - due to limitations of the rpc crate that we use, we can work only with `bitcoind` RPC with user/password authorization and as address must be set IP address, not a domain.

#### Data Storage Layers

- **PostgreSQL** application's main database
- **Redis** cache layer

## Local Dev Environment

To run the service locally, you need to have several services running in the background. For reference, this is what my local setup looks like:

### Running Application

```sh
## Start indexer and api-server separatedly

env RUST_LOG=info ./orbtc -c path/to/config.toml api-server

# in the another shell
env RUST_LOG=info ./orbtc -c path/to/config.toml indexer

```

## Deployment

All required instructions and configurations examples can be found in [Deployment Guide](scripts/deployment/Readme.md)

## Profile perf

1. Install `perf`
2. Install `hotspot` - https://github.com/KDAB/hotspot/releases
3. Install `cargo-flamegraph` - https://github.com/flamegraph-rs/flamegraph

How to
- https://rust-lang.github.io/packed_simd/perf-guide/prof/linux.html
- https://nnethercote.github.io/perf-book/profiling.html

## License

This project is open source under the [Apache 2.0 license](/LICENSE).
