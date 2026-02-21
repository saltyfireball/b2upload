#!/usr/bin/env bash

set -e
cargo clean --release --manifest-path src-tauri/Cargo.toml
cargo tauri build
