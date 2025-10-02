# CONTRIBUTING

1. Install latest stable Rust toolchain.
2. Install `cargo-insta` tool:
    - generic: `cargo install cargo-insta`
    - unix: `curl -LsSf https://insta.rs/install.sh | sh`
    - windows: `powershell -c "irm https://insta.rs/install.ps1 | iex"`


Build:

```
cargo build --release
```


Run tests - you need `docker` installed:

```bash
cargo test
```

If snapshot tests fail, you can review the differences with:

```bash
cargo insta review
```
