# ws-tool

An easy to use websocket client/server toolkit, supporting blocking/async IO.

**feature matrix**

| IO type  | split | proxy | tls | buffered  stream | deflate | use as client | use as server |
| -------- | ----- | ----- | --- | ---------------- | ------- | ------------- | ------------- |
| blocking | ✅     | ✅     | ✅   | ✅                | ✅       | ✅             | ✅             |
| async    | ✅     | ✅     | ✅   | ✅                | ✅       | ✅             | ✅             |

For tls connection, ws-tool support both native-tls and rustls,
ws-tool also support simd unmask(need simd feature in nightly build) and simd utf checking for faster unmask & utf8 string checking.


It's tested by autobaha test suit. see [test report](https://privaterookie.github.io/ws-tool-stat/clients/index.html) of 4 example



## usage

Every example can be run with

```bash
cargo run --example <example_name> --all-features
```
command.

See [examples/server](examples/server.rs) for building a websocket echo server with self signed certs.

- [examples/echo](examples/echo.rs) demonstrates how to connect to a server.
- [echo_server](examples/echo_server.rs) demonstrates how to setup a server.
- [deflate_client](examples/deflate_client.rs) and [deflate_server](examples/deflate_server.rs) demostrate how to enable permessage-deflate extension
- [binance](examples/binance.rs) demostrates how to connect to wss server via http/socks proxy
- autobaha_xxx_client are autobaha test suit client
- bench_xxx are benchmark server examples, showing how to control read/write buffer or other low level config


### run autobahn testsuite

start test server

```bash
./script/start_autobahn_server.sh
```

run test on other terminal

```bash
cargo ac
cargo aac
cargo adc
cargo aadc
```

report files should be under `test_reports` dir.

**performance**


Performance is a complex issue, and a single performance indicator is not enough to describe the performance of a certain library. Here we only compare QPS as a brief description of ws-tool performance

My test machine is i9-12900k and 32GB, 3200MHz ddr4, and load test client is [load_test](./examples/load_test.rs)

Roughly compare with [EchoSever example](https://github.com/uNetworking/uWebSockets/blob/master/examples/EchoServer.cpp),  [tungstenite](./examples/bench_tungstenite.rs)


The following are the benchmark(1 connection only) results, there is no tungstenite with buffered stream test case, because there tungstenite does not work well with buffered stream. ws-tool test cases are all built with simd feature enabled, tokio runtime use current_thread flavor.


### 300 bytes payload size, 100000000 messages

```rust
cargo lt -- -p 300 --count 100000 -t 1 <url>
```

| server                        | count     | Duration(ms) | Message/sec    |
| ----------------------------- | --------- | ------------ | -------------- |
| uWebSocket                    | 100000000 | 16798        | 5953089.65     |
| tungstenite                   | 100000000 | 19905        | 5023863.35     |
| bench_server(no buffer)       | 100000000 | 42395        | 2358768.72     |
| bench_server(8k)              | 100000000 | 16541        | **6045583.70** |
| bench_async_server(no buffer) | 100000000 | 45774        | 2184646.31     |
| bench_async_server(8k)        | 100000000 | 16360        | **6112469.44** |


### 1M bytes payload size, 100000 messages

```rust
cargo lt -- -p 1048576 --count 100 -t 1 <url>
```

| server                        | count  | Duration(ms) | Message/sec |
| ----------------------------- | ------ | ------------ | ----------- |
| uWebSocket                    | 100000 | 34900        | 2865.33     |
| tungstenite                   | 100000 | 38745        | 2580.98     |
| bench_server(no buffer)       | 100000 | 29854        | 3349.63     |
| bench_server(8k)              | 100000 | 28887        | **3461.76** |
| bench_async_server(no buffer) | 100000 | 29280        | **3415.30** |
| bench_async_server(8k)        | 100000 | 29384        | 3403.21     |



you can try more combinations with [load_test](./examples/load_test.rs) tool

## http header style

for multiple extension/protocol, ws-tool prefer to use multiple header with the same name, instead of "," separated value.
but ws-tool still try to parse extension/protocol from "," separated header value.


## REF

- [WebSocket RFC](https://datatracker.ietf.org/doc/html/rfc6455)
- [permessage-deflate RFC](https://datatracker.ietf.org/doc/html/rfc7692)
- [tungstenite-rs](https://github.com/snapview/tungstenite-rs)
- [ws-rs](https://github.com/housleyjk/ws-rs)
