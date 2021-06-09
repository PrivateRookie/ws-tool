# Websocket 协议摘要

## 流程

client 握手包(标准http 请求) -> 握手成功 -> 交换数据

1. 创建 TCP 连接
2. 如果是 TLS 加密, 进行 TLS 协议握手
3. 发送握手 http 请求
4. 握手成功, 数据传输
5. 关闭客户端

## http 握手请求说明

```
GET /chat HTTP/1.1                          # http 协议不得小于 1.1
Host: server.example.com                    # 用于向服务端告知当前 Host
Upgrade: websocket                          # 告诉服务器升级协议
Connection: Upgrade                         # 同上
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ== # 用于防止伪造请求
Origin: http://example.com                  # 防止未认证的跨域请求
Sec-WebSocket-Protocol: chat, superchat     # 告知服务端自己支持的协议
Sec-WebSocket-Version: 13                   # 告知服务端自己的 websocket 版本
```

## 关闭通信

双方都可以通过发送控制帧来关闭通信. 当一方收到关闭帧时, 应发送关闭帧给对方.

[closing handshake](https://tools.ietf.org/html/rfc6455#section-1.4)