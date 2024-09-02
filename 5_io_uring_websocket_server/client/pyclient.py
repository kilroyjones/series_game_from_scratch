import asyncio
import websockets
import threading

async def receive_messages(websocket):
    while True:
        try:
            message = await websocket.recv()
            print(f"Received: {message}")
        except websockets.exceptions.ConnectionClosed:
            print("Connection closed")
            break

async def send_messages(websocket):
    while True:
        message = await asyncio.get_event_loop().run_in_executor(None, input, "Enter message: ")
        if message.lower() == 'exit':
            break
        await websocket.send(message)

async def main():
    uri = "ws://localhost:8080"
    async with websockets.connect(uri) as websocket:
        print(f"Connected to {uri}")
        receive_task = asyncio.create_task(receive_messages(websocket))
        send_task = asyncio.create_task(send_messages(websocket))
        await asyncio.gather(receive_task, send_task)

if __name__ == "__main__":
    asyncio.run(main())