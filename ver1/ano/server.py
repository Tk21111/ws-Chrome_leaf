import asyncio
import websockets
import json
import webbrowser

HOST = "0.0.0.0"
PORT = 24810


async def handler(ws):
    async for msg in ws:
        data = json.loads(msg)
        if data.get("action") == "tel":
            urls = data.get("urls")
            
            for url in urls :
                webbrowser.open(url)
async def main():
    async with websockets.serve(handler , HOST , PORT):
        await asyncio.Future() #keep runing
        

if __name__ == "__main__":
    asyncio.run(main())