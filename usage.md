# usage cases

## common features

1. read/write websocket frame
2. read/write bytes from client
3. read/write string from client
4. read/write serializer/de-serializer structure from client
5. split client into read/write half
6. config tcp no_delay, proxy, fragment size, ping interval
7. handle handshake(extension, protocol or other header)
8. create from tcp stream(tokio or std stream)

### read/write websocket frame

```rust
let mut client = ConnBuilder::new("url")
                        .cert("cert_path")
                        .proxy("proxy_socket")
                        .on_handshake(
                            |resp: http::Response, stream: &mut WsStream| -> Result<FrameCodec, WsError> {
                                 Ok(FrameCodec::default()) }
                            )
                        .codec(FrameCodec::default())
                        .connect().await?;

while let Some(Ok(frame)) = client.next().await? {
    println!("{:?}" frame);
}
```

### read/write bytes from client

```rust
let mut client = ConnBuilder::new("url")
                        .cert("cert_path")
                        .proxy("proxy_socket")
                        .on_handshake(|resp: http::Response, stream: &mut WsStream| -> Result<BytesCodec, WsError> { Ok(BytesCodec::default()) })
                        .connect().await?;

// if client send close or ping frame, payload will be send with data
while let Some(Ok(data)) = client.next().await? {
    println!("{:?}" data);
}
```

### read/write string from client

```rust
let mut client = ConnBuilder::new("url")
                        .cert("cert_path")
                        .proxy("proxy_socket")
                        .on_handshake(|resp: http::Response, stream: &mut WsStream| -> Result<StringCodec, WsError> { Ok(StringCodec::default()) })
                        .connect().await?;

// if client send close or ping frame, payload will be send with data, maybe you need close utf-8 validation
while let Some(Ok(str_data)) = client.next().await? {
    println!("{:?}" str_data);
}
```

### read/write serializer/de-serializer structure from client

```rust
let mut client = ConnBuilder::new("url")
                        .cert("cert_path")
                        .proxy("proxy_socket")
                        .on_handshake(|resp: http::Response, stream: &mut WsStream| -> Result<MessageCodec, WsError> { Ok(MessageCodec::default()) })
                        .connect().await?;

// if client send close or ping frame, payload will be send with data
while let Some(Ok(msg)) = client.next().await? {
    println!("{:?}" msg);
}
```

### split client into read/write half

```rust
let client = .. // do connection prepare work;
let (read, write) = client.split();
```

### handle handshake(extension, protocol or other header)

```rust
let mut client = ConnBuilder::new("url")
                        .cert("cert_path")
                        .proxy("proxy_socket")
                        .on_handshake(|resp: http::Response, stream: &mut WsStream| -> Result<StringCodec, WsError> { 
                            let zipped = req.headers.get("web-socket-protocols") == Some("gzip");
                            Ok(GzipCodec::new(zipped))
                         })
                        .connect().await?;
```

### create from tcp stream(tokio or std stream)

```rust
let mut client = stream_handshake(stream, on_handshake);

let mut client = stream_no_handshake(stream, codec);
```
