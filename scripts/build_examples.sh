#!/bin/bash

set -e
echo "building autobahn_async_client ..."
cargo build -q --example autobahn_async_client -F async

echo "building autobahn_async_deflate_client ..."
cargo build -q --example autobahn_async_deflate_client -F async -F deflate

echo "building autobahn_client ..."
cargo build -q --example autobahn_client

echo "building autobahn_deflate_client ..."
cargo build -q --example autobahn_deflate_client -F deflate

echo "building bench_async_server ..."
cargo build -q --example bench_async_server -F async

echo "building bench_deflate_server ..."
cargo build -q --example bench_deflate_server -F deflate

echo "building bench_fastwebsockets ..."
cargo build -q --example bench_fastwebsockets -F async

echo "building bench_server ..."
cargo build -q --example bench_server

echo "building bench_tungstenite ..."
cargo build -q --example bench_tungstenite -F async

echo "building binance ..."
cargo build -q --example binance -F async -F deflate -F async_tls_rustls

echo "building echo ..."
cargo build -q --example echo -F async

echo "building echo_async_server ..."
cargo build -q --example echo_async_server -F async -F async_tls_rustls

echo "building echo_server ..."
cargo build -q --example echo_server

echo "building ext_axum ..."
cargo build -q --example ext_axum -F axum --no-default-features 

echo "building ext_poem ..."
cargo build -q --example ext_poem -F poem

echo "building load_test ..."
cargo build -q --example load_test -F deflate

echo "building tls_proxy_deflate_client ..."
cargo build -q --example tls_proxy_deflate_client 

