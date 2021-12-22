
# WSS (WS over TLS) server example, with a self-signed certificate

import asyncio
import pathlib
import ssl
import websockets

async def hello(websocket, path):
    while True:
        name = await websocket.recv()
        if name == 'quit':
            print('remote exit')
            await websocket.close()
            break
        greeting = f"Hello {name}!"
        await websocket.send(greeting)
        print(f"> {greeting}")

ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
key_pem = pathlib.Path(__file__).with_name("target-key.pem")
cert_pem = pathlib.Path(__file__).with_name("target.pem")
ssl_context.load_cert_chain(cert_pem, key_pem)

start_server = websockets.serve(
    hello, "0.0.0.0", 4430,  compression = "deflate", ssl=ssl_context,
)
print("running on 0.0.0.0:4430")
asyncio.get_event_loop().run_until_complete(start_server)
asyncio.get_event_loop().run_forever()