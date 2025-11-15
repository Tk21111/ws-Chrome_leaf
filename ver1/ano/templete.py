import asyncio
import websockets
import json
import pygetwindow as gw
import pyautogui

LOCAL_WS = "ws://127.0.0.1:24810"   # local WS server
REMOTE_WS = "ws://REMOTE_IP:24810"  # remote WS server
CHECK_INTERVAL = 0.1

class Middleman:
    def __init__(self):
        self.local_ws = None
        self.lock = asyncio.Lock()  # ensure only one request at a time

    async def connect_local(self):
        """Connect to local WS and keep it hot."""
        while True:
            try:
                self.local_ws = await websockets.connect(LOCAL_WS)
                print("Connected to local WS")
                break
            except Exception as e:
                print("Failed to connect to local WS, retrying:", e)
                await asyncio.sleep(2)

    async def get_chrome_tab(self):
        """Send get_tabs to local WS and wait for response."""
        async with self.lock:  # prevent overlapping requests
            try:
                await self.local_ws.send("get_tabs")
                url = await self.local_ws.recv()
                return url
            except Exception as e:
                print("Local WS failed, reconnecting:", e)
                await self.connect_local()
                return None

    async def forward_to_remote(self, url):
        """Send URL to remote WS."""
        try:
            async with websockets.connect(REMOTE_WS) as ws:
                data = {"action": "teleport_window", "url": url}
                await ws.send(json.dumps(data))
            print(f"Forwarded URL to remote: {url}")
        except Exception as e:
            print("Failed to send to remote WS:", e)

    async def monitor_mouse(self):
        """Main loop: detect dragging out of Chrome and forward URL."""
        screen_w, screen_h = pyautogui.size()
        while True:
            x, y = pyautogui.position()
            win = None

            # Find active Chrome window
            for w in gw.getAllWindows():
                if "Chrome" in w.title and w.isActive:
                    win = w
                    break

            if win and x >= screen_w - 3:  # mouse near right edge
                url = await self.get_chrome_tab()
                if url and url.startswith("http"):
                    await self.forward_to_remote(url)
                    await asyncio.sleep(2)  # avoid spamming
            await asyncio.sleep(CHECK_INTERVAL)


async def main():
    m = Middleman()
    await m.connect_local()   # connect once
    await m.monitor_mouse()

if __name__ == "__main__":
    asyncio.run(main())
