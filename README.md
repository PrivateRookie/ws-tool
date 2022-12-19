# ws-tool

An easy to use websocket client/server toolkit, supporting blocking/async IO.

**feature matrix**

| IO type  | split | proxy(auth) | tls | buffered  stream | deflate | use as client | use as server |
| -------- | ----- | ----------- | --- | ---------------- | ------- | ------------- | ------------- |
| blocking | ✅     | ✅           | ✅   | ✅                | 🚧wip    | ✅             | ✅             |
| async    | ✅     | ✅           | ✅   | ✅                | 🚧wip    | ✅             | ✅             |


It's tested by autobaha test suit. see [examples/autobahn-client](examples/autobahn-client.rs)


**pretty good performance**

Roughly compare with [EchoSever example](https://github.com/uNetworking/uWebSockets/blob/master/examples/EchoServer.cpp),  [tungstenite](./examples/bench_tungstenite.rs), both [async](examples/bench_async_server.rs) and [blocking](examples/bench_server.rs) version of ws-tool echo serve win in single client(more then 10) with 1m payload size.

My test machine is i9-12900k and 32GB, 3200MHz ddr4, use uWebSocket provided client example as perf client

| server  | ws-tool | ws-tool async | uWebSocket | tungstenite |
| ------- | ------- | ------------- | ---------- | ----------- |
| msg/sec | 5150±   | 5150±         | 3200±      | 2850±       |


## usage

Every example can be run with

```bash
cargo run --example <example_name> --all-features
```
command.

See [examples/server](examples/server.rs) for building a websocket echo server with self signed certs.

[examples/echo](examples/echo.rs) demonstrate how to connect to a wss server.


[examples/binance](examples/binance.rs) show how to connect via proxy



### run autobaha testsuit

start test server

```bash
./script/start_autobahn_server.sh
```

run test on other terminal

```bash
cargo run --example autobahn_client --all-features
```

report files should be under `test_reports` dir.


## autobahn test report

<details>
<summary>click to expand report</summary>

![report](./assets/report.jpeg)
</details>

## TODO

- [x] add proxy auth config
- [ ] support custom https proxy cert
- [x] split client into writer & reader
- [x] add buffered stream


## REF

- [WebSocket RFC](https://tools.ietf.org/html/rfc6455)
- [tungstenite-rs](https://github.com/snapview/tungstenite-rs)
