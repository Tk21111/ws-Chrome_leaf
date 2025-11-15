import asyncio
import pyautogui
import pygetwindow as gw
import websockets
import json

SEVER = "192.168.1.137"  #todo add server
HOST = "127.0.0.1"
PORT = 24810
CHECK_INTERVAL = 0.2
HAVE_MONITOR = 2 #scale at 1920 * 2 = 3840 if screen not at 1080 pls change
DEVICE = 1

#dek69+1
class MiddleMan:
    def __init__(self):
        self.local_client = None
        self.global_ws = None
        self.tabs = []

    async def conn_local(self):
        async def handle_client(ws):
            print("Local client connected")
            self.local_client = ws
            try:
                async for msg in ws:
                    data = json.loads(msg)
                    # print(data)
                    if data.get("action") == "tabs":
                        self.tabs = data.get("tabs", [])
                        print("Received tabs:", self.tabs)
            except websockets.ConnectionClosed:
                print("Local client disconnected")
                self.local_client = None

        # start local WebSocket server
        server = await websockets.serve(handle_client, HOST, PORT)
        print(f"Local WS running on ws://{HOST}:{PORT}")
        return server  # don’t block here

    #send global action in local ws
    async def conn_global(self):
        try :
            self.global_ws = await websockets.connect(f"ws://{SEVER}:{PORT}")
            async for msg in self.global_ws:
                try:
                    data = json.loads(msg)
                    if data.get("action") == "tel" and data.get("device") == DEVICE:
                        self.local_client.send(msg)
                except json.JSONDecodeError:
                    print("Received invalid JSON:", msg)
                except Exception as e:
                    print("Error handling message:", e)
                    
        except Exception as e:
            print("fail to conn to global ws")
    async def get_tabs(self):
        # wait until local_client connected
        while not self.local_client:
            print("Waiting for local client to connect...")
            await asyncio.sleep(0.5)
        
        try:
            print("Requesting tabs...")
            await self.local_client.send(json.dumps({"action": "get_tabs"}))
        except Exception as e:
            print("Error sending get_tabs:", e)
    
    async def forward_to_server(self, urls):
        if not SEVER:
            print("No server configured yet.")
            return
        try:
            async with websockets.connect(f"ws://{SEVER}:{PORT}") as ws:
                print("send data to ws")
                data = {"action": "tel", "urls": urls , "device" : 2}
                await ws.send(json.dumps(data))
        except Exception as e:
            print("Failed to forward:", e)
            

    async def monitor_mouse(self): 
        screen_w , screen_h = pyautogui.size() 
        while True: 
            x , y = pyautogui.position() 
            win = None 
            for w in gw.getAllWindows(): 
                if "Chrome" in w.title and w.isActive: 
                    win = w 
                    break 

            if win and x >= screen_w *HAVE_MONITOR - 3: 
                await self.get_tabs() 
                if self.tabs: 
                    await self.forward_to_server(self.tabs) 
                    await asyncio.sleep(2) 
                else : 
                    print("cannot found tabs") 

            #main loop sleep 
            await asyncio.sleep(CHECK_INTERVAL)

async def main():
    m = MiddleMan()
    await m.conn_local()
    await m.monitor_mouse()
    await asyncio.gather(m.conn_local(), m.monitor_mouse())
if __name__ == "__main__":
    asyncio.run(main())

