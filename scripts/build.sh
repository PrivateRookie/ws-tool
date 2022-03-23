#!/bin/bash

set -e

echo "building..."
echo "cargo build --no-default-features"
cargo build --no-default-features

echo "building..."
echo "cargo build --no-default-features --features sync"
cargo build --no-default-features --features sync

echo "building..."
echo "cargo build --no-default-features --features sync,sync_tls_rustls"
cargo build --no-default-features --features sync,sync_tls_rustls

echo "building..."
echo "cargo build --no-default-features --features sync,sync_tls_rustls"
cargo build --no-default-features --features sync,sync_tls_rustls

echo "building..."
echo "cargo build --no-default-features --features async"
cargo build --no-default-features --features async

echo "building..."
echo "cargo build --no-default-features --features async,async_tls_rustls"
cargo build --no-default-features --features async,async_tls_rustls

echo "building..."
echo "cargo build --no-default-features --features async,async_tls_rustls,async_proxy"
cargo build --no-default-features --features async,async_tls_rustls,async_proxy

echo "building..."
echo "cargo build --no-default-features --features async,async_proxy"
cargo build --no-default-features --features async,async_proxy

echo "building..."
echo "cargo build --no-default-features --all-features"
cargo build --no-default-features --all-features

