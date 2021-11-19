import asyncio
import websockets

async def hello():
    async with websockets.connect("ws://localhost:9000") as websocket:
        await websocket.send("Hello world!")
        resp = await websocket.recv()
        print(resp)

asyncio.run(hello())