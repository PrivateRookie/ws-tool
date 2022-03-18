#!/bin/bash

set -e

echo "building..."
echo "cargo build --no-default-features"
cargo build --no-default-features

echo "building..."
echo "cargo build --no-default-features --features blocking"
cargo build --no-default-features --features blocking

echo "building..."
echo "cargo build --no-default-features --features blocking,tls_rustls"
cargo build --no-default-features --features blocking,tls_rustls

echo "building..."
echo "cargo build --no-default-features --features blocking,tls_rustls"
cargo build --no-default-features --features blocking,tls_rustls

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

