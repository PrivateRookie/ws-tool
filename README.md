# ws-tool

An easy to use websocket client/server toolkit, supporting blocking/async IO.

**feature matrix**

| IO type  | split | proxy(auth) | tls | buffered  stream | deflate | use as client | use as server |
| -------- | ----- | ----------- | --- | ---------------- | ------- | ------------- | ------------- |
| blocking | ✅     | ✅           | ✅   | ✅                | ✅       | ✅             | ✅             |
| async    | ✅     | ✅           | ✅   | ✅                | ✅       | ✅             | ✅             |


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


### run autobaha testsuit

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


The following are the benchmark(1 connection only) results


### 300 bytes payload size, 50000000 messages

```rust
cargo lt -- -p 300 --count 50000 -t 1 <url>
```

| server                        | count    | Duration(ms) | Message/sec |
| ----------------------------- | -------- | ------------ | ----------- |
| uWebSocket                    | 50000000 | 10785        | 4636068.61  |
| tungstenite                   | 50000000 | 22045        | 2268088.00  |
| bench_server(no buffer)       | 50000000 | 28473        | 1756049.59  |
| bench_server(8k)              | 50000000 | 10791        | 4633490.87  |
| bench_async_server(no buffer) | 50000000 | 41068        | 1217492.94  |
| bench_async_server(8k)        | 50000000 | 14892        | 3357507.39  |


### 1M bytes payload size, 100000 messages

```rust
cargo lt -- -p 1048576 --count 100 -t 1 <url>
```

| server                        | count  | Duration(ms) | Message/sec |
| ----------------------------- | ------ | ------------ | ----------- |
| uWebSocket                    | 100000 | 37545        | 2663.47     |
| tungstenite                   | 100000 | 41413        | 2414.70     |
| bench_server(no buffer)       | 100000 | 32689        | 3059.13     |
| bench_server(8k)              | 100000 | 29949        | 3339.01     |
| bench_async_server(no buffer) | 100000 | 38959        | 2566.80     |
| bench_async_server(8k)        | 100000 | 31378        | 3186.95     |



you can try more combinations with [load_test](./examples/load_test.rs) tool

## http header style

for multiple extension/protocol, ws-tool prefer to use multiple header with the same name, instead of "," separated value.
but ws-tool still try to parse extension/protocol from "," separated header value.


## REF

- [WebSocket RFC](https://datatracker.ietf.org/doc/html/rfc6455)
- [permessage-deflate RFC](https://datatracker.ietf.org/doc/html/rfc7692)
- [tungstenite-rs](https://github.com/snapview/tungstenite-rs)
- [ws-rs](https://github.com/housleyjk/ws-rs)
