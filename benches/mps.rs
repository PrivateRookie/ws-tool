use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use ws_tool::{
    codec::{default_handshake_handler, BytesCodec},
    stream::BufStream,
    ClientBuilder, ServerBuilder,
};

fn ws_tool_sync_no_buffer(c: &mut Criterion) {
    std::thread::spawn(move || {
        let listener = std::net::TcpListener::bind("127.0.0.1:15523").unwrap();
        loop {
            let (stream, _) = listener.accept().unwrap();
            std::thread::spawn(move || {
                let mut server =
                    ServerBuilder::accept(stream, default_handshake_handler, BytesCodec::factory)
                        .unwrap();
                loop {
                    let msg = server.receive().unwrap();
                    if msg.code.is_close() {
                        break;
                    }
                    server.send(&msg.data[..]).unwrap();
                }
            });
        }
    });
    std::thread::spawn(move || {
        let listener = std::net::TcpListener::bind("127.0.0.1:15524").unwrap();
        loop {
            let (stream, _) = listener.accept().unwrap();
            std::thread::spawn(move || {
                let mut server =
                    ServerBuilder::accept(stream, default_handshake_handler, |req, stream| {
                        BytesCodec::factory(req, BufStream::new(stream))
                    })
                    .unwrap();
                loop {
                    let msg = server.receive().unwrap();
                    if msg.code.is_close() {
                        break;
                    }

                    server.send(&msg.data[..]).unwrap();
                }
            });
        }
    });

    let mut group = c.benchmark_group("ws_tool_sync_no_buffer");
    for size in [100, 512, 1024, 2048, 4096].iter() {
        group.throughput(criterion::Throughput::Bytes(*size));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let client = ClientBuilder::new()
                    .connect(
                        "ws://127.0.0.1:15523".parse().unwrap(),
                        |key, resp, stream| BytesCodec::check_fn(key, resp, BufStream::new(stream)),
                    )
                    .unwrap();
                let (mut read, mut write) = client.split();

                let w = std::thread::spawn(move || {
                    let payload = vec![0].repeat(size as usize);
                    for _ in 0..10000 {
                        write.send(&payload[..]).unwrap();
                    }
                    write.flush().unwrap();
                    write.send((1000u16, &[][..])).unwrap();
                });
                let r = std::thread::spawn(move || {
                    for _ in 0..10000 {
                        read.receive().unwrap();
                    }
                });
                r.join().and_then(|_| w.join()).unwrap();
            })
        });
    }
    group.finish();
    let mut group = c.benchmark_group("ws_tool_sync_8k_buffer");
    for size in [100, 512, 1024, 2048, 4096].iter() {
        group.throughput(criterion::Throughput::Bytes(*size));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let client = ClientBuilder::new()
                    .connect(
                        "ws://127.0.0.1:15524".parse().unwrap(),
                        |key, resp, stream| BytesCodec::check_fn(key, resp, BufStream::new(stream)),
                    )
                    .unwrap();
                let (mut read, mut write) = client.split();

                let w = std::thread::spawn(move || {
                    let payload = vec![0].repeat(size as usize);
                    for _ in 0..10000 {
                        write.send(&payload[..]).unwrap();
                    }
                    write.flush().unwrap();
                    write.send((1000u16, &[][..])).unwrap();
                });
                let r = std::thread::spawn(move || {
                    for _ in 0..10000 {
                        read.receive().unwrap();
                    }
                });
                r.join().and_then(|_| w.join()).unwrap();
            })
        });
    }
    group.finish();
}

criterion_group!(benches, ws_tool_sync_no_buffer);
criterion_main!(benches);
