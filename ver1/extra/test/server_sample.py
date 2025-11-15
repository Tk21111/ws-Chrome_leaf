import os
import asyncio
import json
import logging
import functools
from aiohttp import web, WSMsgType
import mss
import numpy as np
import cv2
from aiortc import RTCPeerConnection, RTCSessionDescription, RTCIceCandidate, VideoStreamTrack
from av import VideoFrame
import pyautogui
from typing import Dict, Any

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("webrtc-ws-ctrl")

# Config
HOST = "0.0.0.0"
PORT = int(os.getenv("PORT", 3030))
# FIX: Use the environment variable, not a hardcoded value
AUTH_TOKEN = os.getenv("AUTH_TOKEN", "changeme") # set a strong token in env for real use
FPS = 20
WIDTH = 1920
HEIGHT = 1080
MONITOR_INDEX = 1 # primary monitor usually 1

# Shared state
clients: Dict[str, Dict[str, Any]] = {}  # peer_id -> {ws, pc}
pcs = set()
_latest_frame = None
_frame_lock = asyncio.Lock()


# ---- Shared capture loop ----
async def capture_loop():
    global _latest_frame
    
    logger.info("capture loop")
    sct = mss.mss()
    monitor = sct.monitors[MONITOR_INDEX]
    interval = 1.0 / FPS
    logger.info(f"Starting capture loop: monitor={MONITOR_INDEX} target={WIDTH}x{HEIGHT} @ {FPS}FPS")
    while True:
        try:
            shot = sct.grab(monitor)
            img = np.array(shot)[:, :, :3]  # drop alpha
            # resize faster with cv2
            img = cv2.resize(img, (WIDTH, HEIGHT), interpolation=cv2.INTER_CUBIC)
            # mss returns BGRA/BGR order; convert to RGB for VideoFrame
            img = cv2.cvtColor(img, cv2.COLOR_BGR2RGB)

            async with _frame_lock:
                _latest_frame = img
        except Exception as e:
            logger.exception("capture error: %s", e)
        # use precise sleep
        await asyncio.sleep(interval)


# ---- Shared track that reads latest frame ----
class SharedScreenTrack(VideoStreamTrack):
    def __init__(self):
        super().__init__()
        self._black_frame = np.zeros((HEIGHT, WIDTH, 3), dtype=np.uint8)

    async def recv(self):
        # FIX: This is the correct implementation.
        # next_timestamp() already sleeps until the correct PTS.
        # No while-loop or extra sleep is needed.
        pts, time_base = await self.next_timestamp()

        img = None
        async with _frame_lock:
            if _latest_frame is not None:
                img = _latest_frame.copy()

        # FIX: If capture isn't ready, send a black frame to avoid errors
        if img is None:
            # logger.info("no image")
            img = self._black_frame

        frame = VideoFrame.from_ndarray(img, format="rgb24")
        frame.pts = pts
        frame.time_base = time_base
        return frame


# ---- WebSocket signaling + control handler ----
async def websocket_handler(request):
    ws = web.WebSocketResponse()
    await ws.prepare(request)
    loop = asyncio.get_event_loop()

    # simple auth step
    auth_msg = await ws.receive()
    if auth_msg.type != WSMsgType.TEXT:
        await ws.close()
        return ws
    try:
        auth = json.loads(auth_msg.data)
    except Exception:
        await ws.close()
        return ws

    if auth.get("type") != "auth" or auth.get("token") != AUTH_TOKEN:
        await ws.send_json({"type": "auth", "ok": False, "reason": "invalid token"})
        await ws.close()
        return ws

    await ws.send_json({"type": "auth", "ok": True})
    
    peer_id = str(id(ws))
    logger.info("Client %s connected and authenticated", peer_id)
    clients[peer_id] = {"ws": ws, "pc": None}
    
    try:
        async for msg in ws:
            if msg.type == WSMsgType.TEXT:
                data = json.loads(msg.data)
                t = data.get("type")
                # --- Offer from browser ---
                if t == "offer":
                    sdp = data.get("sdp")
                    if not sdp:
                        await ws.send_json({"type": "error", "message": "missing sdp"})
                        continue
                    
                    pc = RTCPeerConnection()
                    pcs.add(pc)
                    clients[peer_id]["pc"] = pc
                    
                    @pc.on("icecandidate")
                    async def on_icecandidate(candidate):
                        # FIX: Send candidate in the format the browser expects
                        if candidate:
                            try:
                                await ws.send_json({
                                    "type": "candidate",
                                    "candidate": {
                                        "candidate": candidate.candidate,
                                        "sdpMid": candidate.sdpMid,
                                        "sdpMLineIndex": candidate.sdpMLineIndex,
                                    }
                                })
                            except Exception:
                                pass # ws might be closing

                    @pc.on("connectionstatechange")
                    async def on_connstatechange():
                        logger.info("PC %s state: %s", peer_id, pc.connectionState)
                        if pc.connectionState in ("failed", "closed", "disconnected"):
                            await pc.close()
                            pcs.discard(pc)
                            clients[peer_id]["pc"] = None

                    # Now set remote offer and produce answer.
                    try:
                        # FIX: Set remote description *before* adding the track
                        # This lets aiortc match the track to the client's "recvonly" transceiver
                        await pc.setRemoteDescription(RTCSessionDescription(sdp=sdp, type="offer"))
                        
                        # FIX: Add track *after* setting remote description
                        pc.addTrack(SharedScreenTrack())

                        answer = await pc.createAnswer()
                        await pc.setLocalDescription(answer)
                        
                        await ws.send_json({"type": "answer", "sdp": pc.localDescription.sdp})
                        logger.info("Answered offer for peer %s", peer_id)

                    except Exception as e:
                        logger.exception("Error handling offer for peer %s: %s", peer_id, e)
                        try:
                            await ws.send_json({"type": "error", "message": "server SDP error", "details": str(e)})
                        except Exception:
                            pass
                        # cleanup pc
                        await pc.close()
                        pcs.discard(pc)
                        clients[peer_id]["pc"] = None

                # --- ICE candidate from client ---
                elif t == "candidate":
                    candidate_data = data.get("candidate")
                    pc = clients[peer_id].get("pc")
                    if pc and candidate_data:
                        try:
                            # FIX: Create RTCIceCandidate from the dict client sends
                            ice = RTCIceCandidate(
                                sdpMid=candidate_data.get("sdpMid"),
                                sdpMLineIndex=candidate_data.get("sdpMLineIndex"),
                                candidate=candidate_data.get("candidate"),
                            )
                            await pc.addIceCandidate(ice)
                        except Exception:
                            logger.exception("adding candidate failed")

                # --- Remote control messages ---
                elif t == "control":
                    action = data.get("action")
                    payload = data.get("payload", {})
                    # FIX: Run blocking pyautogui calls in a thread pool
                    await loop.run_in_executor(None, handle_control, action, payload)
                
                elif t == "bye":
                    break
                else:
                    await ws.send_json({"type": "error", "message": "unknown message type"})
            elif msg.type == WSMsgType.ERROR:
                logger.error("ws connection closed with exception %s", ws.exception())
    finally:
        # cleanup this peer
        info = clients.pop(peer_id, None)
        if info:
            pc = info.get("pc")
            if pc:
                await pc.close()
                pcs.discard(pc)
        await ws.close()
        logger.info("client disconnected: %s", peer_id)
    return ws


# ---- Control handling using pyautogui (BLOCKING calls) ----
def handle_control(action: str, payload: dict):
    """
    This function is run in a thread pool to avoid blocking asyncio.
    action: "mouse", "scroll", "key"
    """
    try:
        if action == "mouse":
            ev = payload.get("event")
            nx = payload.get("x")
            ny = payload.get("y")
            btn = payload.get("button", "left")
            if nx is None or ny is None:
                return
            # map normalized coords to real monitor coords
            with mss.mss() as sct:
                mon = sct.monitors[MONITOR_INDEX]
                real_w = mon["width"]
                real_h = mon["height"]
                # apply monitor's offset
                rx = mon["left"] + int(nx * real_w)
                ry = mon["top"] + int(ny * real_h)

            if ev == "move":
                pyautogui.moveTo(rx, ry, duration=0)
            elif ev == "click":
                pyautogui.click(rx, ry, button=btn)
            elif ev == "down":
                pyautogui.mouseDown(rx, ry, button=btn)
            elif ev == "up":
                pyautogui.mouseUp(rx, ry, button=btn)
        
        elif action == "scroll":
            dx = int(payload.get("dx", 0))
            dy = int(payload.get("dy", 0)) # client sends wheel up as positive
            # pyautogui.scroll uses positive for up
            if dy:
                pyautogui.scroll(dy)
            if dx:
                try:
                    pyautogui.hscroll(dx) # client sends wheel right as positive
                except Exception:
                    pass
        
        elif action == "key":
            ev = payload.get("event")
            key = payload.get("key")
            if not key:
                return
            # Map JS key names to pyautogui names
            key_map = {
                "ArrowUp": "up",
                "ArrowDown": "down",
                "ArrowLeft": "left",
                "ArrowRight": "right",
                "Enter": "enter",
                "Escape": "esc",
                "Tab": "tab",
                " ": "space",
                "Backspace": "backspace",
                "Delete": "delete",
                # Add more as needed
            }
            key = key_map.get(key, key)

            if ev == "press":
                pyautogui.press(key)
            elif ev == "down":
                pyautogui.keyDown(key)
            elif ev == "up":
                pyautogui.keyUp(key)
    except Exception as e:
        logger.exception("control handling error: %s", e)


# ---- aiohttp app ----

# FIX: Embed HTML content here
HTML_CONTENT = """
<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <title>Screen Broadcast + Remote Control</title>
  <style>
    body {
      background: #111;
      color: #eee;
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 8px;
      padding: 16px;
      font-family: sans-serif;
    }
    video {
      border-radius: 8px;
      box-shadow: 0 0 20px #000;
      width: 90vw;
      max-width: 1920;
      height: auto;
      background: #333;
      cursor: crosshair;
    }
    .controls {
      display: flex;
      gap: 8px;
    }
    button, input {
      font-size: 14px;
    }
  </style>
</head>
<body>
  <h2>Screen Broadcast (WebSocket Signaling) - Remote Control Enabled</h2>
  <video id="video" autoplay playsinline></video>
  <div class="controls">
    <label>Auth token: <input id="token" value="{AUTH_TOKEN}" style="width:320px"></label>
    <button id="connect">Connect</button>
  </div>
  <p>Click inside the video to send clicks. Use mouse wheel to scroll. Press keys to send keyboard events (when 'Video Focus' checkbox is checked).</p>
  <label><input type="checkbox" id="videoFocus"> Video Focus (capture keyboard)</label>
  <pre id="logs" style="color: #888;"></pre>

  <script>
    let ws;
    let pc;
    const video = document.getElementById('video');
    const connectBtn = document.getElementById('connect');
    const tokenInput = document.getElementById('token');
    const videoFocus = document.getElementById('videoFocus');
    const logs = document.getElementById('logs');

    function log(msg) {
        console.log(msg);
        logs.textContent = msg + "\\n" + logs.textContent;
    }

    async function connect() {
      const token = tokenInput.value;
      if (!token) {
        alert("Please enter an auth token.");
        return;
      }
      
      const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
      const url = `${proto}//${window.location.host}/ws`;
      ws = new WebSocket(url);
      log(`Connecting to ${url}...`);

      ws.onopen = async () => {
        log('WebSocket connected. Authenticating...');
        ws.send(JSON.stringify({ type: 'auth', token }));
      };

      ws.onmessage = async (evt) => {
        const msg = JSON.parse(evt.data);
        if (msg.type === 'auth') {
          if (!msg.ok) {
            alert('Auth failed: ' + (msg.reason || ''));
            log(`Auth failed: ${msg.reason}`);
            ws.close();
            return;
          }
          log('Authenticated. Creating WebRTC offer...');
          
          const iceConfig = {
            iceServers: [{ urls: 'stun:stun.l.google.com:19302' }]
            };
        pc = new RTCPeerConnection(iceConfig);
        
       
          pc.ontrack = e => {
            log('Got remote track.');
            video.srcObject = e.streams[0];
          };

          pc.onicecandidate = e => {
            if (e.candidate) {
              ws.send(JSON.stringify({ type: 'candidate', candidate: e.candidate }));
            }
          };

          pc.onconnectionstatechange = () => log(`PC state: ${pc.connectionState}`);
          
          // We want to *receive* video, so direction is "recvonly"
          pc.addTransceiver("video", { direction: "recvonly" });
          
          const offer = await pc.createOffer();
          await pc.setLocalDescription(offer);
          
          log('Sending offer...');
          ws.send(JSON.stringify({ type: 'offer', sdp: offer.sdp }));
          
           const sender = pc.getSenders().find(s => s.track.kind === 'video');
          let params = sender.getParameters();
          params.encodings = [{ maxBitrate: 5000000 }]; // 5 Mbps
          sender.setParameters(params);

        } else if (msg.type === 'answer') {
          log('Received answer. Setting remote description...');
          try {
            await pc.setRemoteDescription({ type: 'answer', sdp: msg.sdp });
          } catch (e) {
            log('Error setting remote description: ' + e);
          }
        
        } else if (msg.type === 'candidate') {
          // FIX: This is the critical missing piece.
          // We must add the ICE candidates from the server.
          if (msg.candidate) {
            log('Received ICE candidate.');
            try {
              await pc.addIceCandidate(msg.candidate);
            } catch (e) {
              console.error('Error adding received ice candidate', e);
              log('Error adding ICE candidate: ' + e);
            }
          }
        
        } else if (msg.type === 'error') {
            log(`Server error: ${msg.message}`);
        
        } else {
          log(`Unknown WS msg: ${msg.type}`);
        }
      };

      ws.onclose = () => {
        log('WebSocket closed.');
        if (pc) pc.close();
        video.srcObject = null;
      };
      ws.onerror = e => log('WebSocket error.');
    }

    // map click coords to normalized (0..1)
    function normalizeClientCoords(clientX, clientY) {
      const rect = video.getBoundingClientRect();
      const x = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
      const y = Math.max(0, Math.min(1, (clientY - rect.top) / rect.height));
      return { x, y };
    }
    
    function sendControl(action, payload) {
        if (!ws || ws.readyState !== WebSocket.OPEN) return;
        ws.send(JSON.stringify({
          type: 'control',
          action: action,
          payload: payload
        }));
    }

    video.addEventListener('mousemove', (ev) => {
      // Only send move if a button is pressed (dragging)
      if (ev.buttons === 0) return;
      const { x, y } = normalizeClientCoords(ev.clientX, ev.clientY);
      sendControl('mouse', { event: 'move', x: x, y: y });
    });
    
    video.addEventListener('mousedown', (ev) => {
      const { x, y } = normalizeClientCoords(ev.clientX, ev.clientY);
      sendControl('mouse', { event: 'down', x: x, y: y, button: ev.button === 2 ? 'right' : 'left' });
    });
    
    video.addEventListener('mouseup', (ev) => {
      const { x, y } = normalizeClientCoords(ev.clientX, ev.clientY);
      sendControl('mouse', { event: 'up', x: x, y: y, button: ev.button === 2 ? 'right' : 'left' });
    });

    // prevent context menu on right-click
    video.addEventListener('contextmenu', (ev) => ev.preventDefault());

    // wheel -> scroll
    video.addEventListener('wheel', (ev) => {
      // Use wheel delta directly for smoother scrolling
      // pyautogui.scroll expects positive for up, negative for down
      // ev.deltaY is positive for down, negative for up. So just invert.
      const dy = -ev.deltaY;
      const dx = -ev.deltaX;
      sendControl('scroll', { dx: dx, dy: dy });
      ev.preventDefault();
    });

    // keyboard
    window.addEventListener('keydown', (ev) => {
      if (!videoFocus.checked) return;
      sendControl('key', { event: 'down', key: ev.key });
      ev.preventDefault();
    });

    window.addEventListener('keyup', (ev) => {
      if (!videoFocus.checked) return;
      sendControl('key', { event: 'up', key: ev.key });
      ev.preventDefault();
    });

    connectBtn.onclick = () => {
        if (ws) ws.close();
        if (pc) pc.close();
        connect();
    };
  </script>
</body>
</html>
"""

async def index(request):
    # FIX: Serve the HTML content from the string, injecting the auth token
    content = HTML_CONTENT.replace("{AUTH_TOKEN}", AUTH_TOKEN)
    return web.Response(text=content, content_type="text/html")


async def on_shutdown(app):
    logger.info("Shutting down: closing peer connections")
    coros = [pc.close() for pc in list(pcs)]
    await asyncio.gather(*coros)


async def start_capture(app):
    app['capture_task'] = asyncio.create_task(capture_loop())

async def cleanup_capture(app):
    task = app.get('capture_task')
    if task:
        task.cancel()
        try:
            await task
        except asyncio.CancelledError:
            pass

def main():
    if AUTH_TOKEN == "changeme":
        logger.warning("AUTH_TOKEN is not set. Using 'changeme'.")
        
    app = web.Application()
    app.router.add_get("/", index)
    app.router.add_get("/ws", websocket_handler)
    app.on_startup.append(start_capture)   # <<< start capture here
    app.on_shutdown.append(on_shutdown)
    app.on_cleanup.append(cleanup_capture) # <<< cleanup capture task

    web.run_app(app, host=HOST, port=PORT)

if __name__ == "__main__": main()