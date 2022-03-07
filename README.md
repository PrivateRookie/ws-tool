# ws-tool

An easy to use websocket client/server toolkit, supporting blocking/async IO.

**feature matrix**

| IO type  | proxy | tls | deflate | use as client | use as server |
| -------- | ----- | --- | ------- | ------------- | ------------- |
| blocking | 🚧wip  | ✅   | 🚧wip    | ✅             | ✅             |
| async    | ✅     | ✅   | 🚧wip    | ✅             | ✅             |


It's tested by autobaha test suit. see [examples/autobahn-client](examples/autobahn-client.rs)


**pretty good performance**

Roughly compare with [EchoSever example](https://github.com/uNetworking/uWebSockets/blob/master/examples/EchoServer.cpp), both [async](examples/bench_async_server.rs) and [blocking](examples/bench_server.rs) version of ws-tool echo serve win in multi client(more then 10) with 1m payload size.

My test machine is i9-12900k and 32GB, 3200MHz ddr4, use uWebSocket provided client example as perf client

uWebSocket
```bash
./benchmarks/load_test 50 localhost 9001 0 0 1
Using message size of 1 MB
Running benchmark now...
Msg/sec: 2066.000000
Msg/sec: 2105.000000
Msg/sec: 2056.000000
Msg/sec: 2061.250000
Msg/sec: 2043.250000
```

async ws-tool
```bash
./benchmarks/load_test 50 localhost 8080 0 0 1
Using message size of 1 MB
Running benchmark now...
Msg/sec: 4045.500000
Msg/sec: 3979.500000
Msg/sec: 3977.000000
Msg/sec: 4095.250000
Msg/sec: 3830.750000
```

blocking ws-tool
```bash
./benchmarks/load_test 50 localhost 8080 0 0 1
Using message size of 1 MB
Running benchmark now...
Msg/sec: 3640.250000
Msg/sec: 3723.750000
Msg/sec: 3705.000000
Msg/sec: 3690.000000
Msg/sec: 3491.750000
```

But when testing with single client, uWebSocket wins

```bash
./benchmarks/load_test 1 localhost 9001 0 0 1
Using message size of 1 MB
Running benchmark now...
Msg/sec: 3349.000000
Msg/sec: 3322.750000
Msg/sec: 3363.750000
```

async ws-tool
```bash
./benchmarks/load_test 1 localhost 8080 0 0 1
Using message size of 1 MB
Running benchmark now...
Msg/sec: 1315.250000
Msg/sec: 1315.250000
Msg/sec: 1311.750000
```

But I haven't done much optimization yet, if I have time, I should do some more optimizations, hopefully it will improve


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

- [ ] add proxy auth config
- [ ] support custom https proxy cert
- [ ] split client into writer & reader(working)


## REF

- [WebSocket RFC](https://tools.ietf.org/html/rfc6455)
- [tungstenite-rs](https://github.com/snapview/tungstenite-rs)
