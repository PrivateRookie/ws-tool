# ws-tool

An easy to use websocket client/server toolkit, supporting blocking/async IO.

**feature matrix**

| IO type  | split | proxy | tls | buffered  stream | deflate | use as client | use as server |
| -------- | ----- | ----- | --- | ---------------- | ------- | ------------- | ------------- |
| blocking | ✅     | ✅     | ✅   | ✅                | ✅       | ✅             | ✅             |
| async    | ✅     | ✅     | ✅   | ✅                | ✅       | ✅             | ✅             |

web framework integration

- **axum** see [examples/ext_axum](./examples/ext_axum.rs)
- **poem** see [examples/ext_poem](./examples/ext_poem.rs)

For tls connection, ws-tool support both native-tls and rustls,
ws-tool also support simd utf checking for faster utf8 string checking.

It's tested by autobaha test suit. see [test report](https://privaterookie.github.io/ws-tool-stat/clients/index.html) of 4 example



## usage

Every example can be run with

```bash
cargo run --example <example_name> --all-features
```
command.

See 
- [examples/echo_async_server](examples/server.rs) for building a websocket echo server with self signed cert.
- [examples/echo](examples/echo.rs) demonstrates how to connect to a server.
- [binance](examples/binance.rs) demonstrates how to connect to wss server via http/socks proxy
- [poem](examples/poem.rs) demonstrates how to integrate with poem web framework.
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


The following are the benchmark(1 connection only) results, there is no tungstenite with buffered stream test case, because there tungstenite does not work well with buffered stream, fastwebsockets is not added, because it's performance is very bad in this test case, if you know how to improve fastwebsockets performance, please open a PR! tokio runtime use current_thread flavor.


### 300 bytes payload size, 100000000 messages

```rust
cargo lt -- -b 819200 -p 300 --count 100000 -t 1 <url>
```

| server                   | count     | Duration(ms) | Message/sec     |
| ------------------------ | --------- | ------------ | --------------- |
| uWebSocket               | 100000000 | 10014        | 9986019.57      |
| tungstenite              | 100000000 | 21566        | 4636928.50      |
| bench_server(8k)         | 100000000 | 29597        | 3378720.82      |
| bench_server(800k)       | 100000000 | 9320         | **10729613.73** |
| bench_async_server(8k)   | 100000000 | 17846        | 5603496.58      |
| bench_async_server(800k) | 100000000 | 14006        | 7139797.23      |


### 1M bytes payload size, 100000 messages

```rust
cargo lt -- -p 1048576 --count 100 -t 1 <url>
```

| server                        | count  | Duration(ms) | Message/sec |
| ----------------------------- | ------ | ------------ | ----------- |
| uWebSocket                    | 100000 | 34195        | 2924.40     |
| tungstenite                   | 100000 | 40139        | 2491.34     |
| bench_server(no buffer)       | 100000 | 16405        | **6095.70** |
| bench_server(8k)              | 100000 | 17240        | 5800.46     |
| bench_async_server(no buffer) | 100000 | 17190        | 5817.34     |
| bench_async_server(8k)        | 100000 | 17027        | 5873.03     |


you can try more combinations with [load_test](./examples/load_test.rs) tool

## http header style

for multiple extension/protocol, ws-tool prefer to use multiple header with the same name, instead of "," separated value.
but ws-tool still try to parse extension/protocol from "," separated header value.


## REF

- [WebSocket RFC](https://datatracker.ietf.org/doc/html/rfc6455)
- [permessage-deflate RFC](https://datatracker.ietf.org/doc/html/rfc7692)
- [tungstenite-rs](https://github.com/snapview/tungstenite-rs)
- [ws-rs](https://github.com/housleyjk/ws-rs)
