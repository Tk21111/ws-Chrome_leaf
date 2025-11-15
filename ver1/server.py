import os
import asyncio
import json
import logging
from aiohttp import web, WSMsgType
import mss
import numpy as np
import cv2
from aiortc import RTCPeerConnection, RTCSessionDescription, RTCIceCandidate, VideoStreamTrack
from av import VideoFrame
import pyautogui
from typing import Dict, Any

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("webrtc-server")

# Config
HOST = "0.0.0.0"
PORT = int(os.getenv("PORT", 3030))
AUTH_TOKEN = os.getenv("AUTH_TOKEN", "changeme")  # set a strong token in env for real use
FPS = 20
WIDTH = 1280
HEIGHT = 720
MONITOR_INDEX = 1  # primary monitor usually 1

clients : Dict[str , Dict[str , Any]] = {}
pcs = set()
lastest_frame = None
frame_lock = asyncio.Lock()

async def capture_loop():
    global lastest_frame
    sct = mss.mss()
    monitor = sct.monitors[MONITOR_INDEX]
    interval = 1.0 / FPS
    
    while True:
        try :
            shot = sct.grab(monitor)
            #RBGA -> RGB
            img = np.array(shot)[:,:,:3] #drop A
            img = cv2.resize(img , (WIDTH , HEIGHT) , interpolation=cv2.INTER_AREA)
            #it return BGR so have to convert back to rgb
            img = cv2.cvtColor(img, cv2.COLOR_BGR2RGB)
            
            async with frame_lock:
                lastest_frame = img
        except Exception as e:
            logger.exception("capture err : %s" , e)
        
        await asyncio.sleep(interval)
        
class ScreenTrack(VideoStreamTrack):
    def __init__(self):
        super().__init__()
        
    async def recv(self):
        
        pts , time_base = await self.next_timestamp()
        async with frame_lock:
            #why copy if not it will fucking lock
            img = None if lastest_frame is None else lastest_frame.copy()
        
        if img is None:
            await asyncio.sleep(1 /FPS)
            return await self.recv()
        
        frame = VideoFrame.from_ndarray(img , format="rgb24")
        frame.pts = pts
        frame.time_base = time_base
        await asyncio.sleep(0) #yield
        return frame

async def websocket_handler(request):
    ws = web.WebSocketResponse()
    await ws.prepare(request)
    
    # simple auth step: client MUST send {"type":"auth","token":"..."} first
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
    
    peer_id = id(ws)
    clients[peer_id] = {"ws":ws , "pc" : None}
    
    #fucking ws handler offer , ice , control
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

                    # Add the single shared screen track (same track for all)
                    pc.addTrack(ScreenTrack())

                    # send ICE candidates from server to client
                    @pc.on("icecandidate")
                    async def on_icecandidate(event):
                        if event:
                            # convert candidate object to serializable dict
                            cand = {
                                "candidate": event.component,  # Sometimes aiortc provides component - be safe and send as stringified event
                                "raw": event.to_sdp() if hasattr(event, "to_sdp") else str(event),
                            }
                            # send as generic candidate payload so client can handle
                            try:
                                await ws.send_json({"type": "candidate", "candidate": cand})
                            except Exception:
                                pass

                    @pc.on("connectionstatechange")
                    async def on_connstatechange():
                        logger.info("PC %s state: %s", peer_id, pc.connectionState)
                        if pc.connectionState in ("failed", "closed"):
                            await pc.close()
                            pcs.discard(pc)

                    # Set remote and create answer
                    await pc.setRemoteDescription(RTCSessionDescription(sdp=sdp, type="offer"))
                    answer = await pc.createAnswer()
                    await pc.setLocalDescription(answer)
                    # Return answer
                    await ws.send_json({"type": "answer", "sdp": pc.localDescription.sdp})
                    logger.info("Answered offer for peer %s", peer_id)

                # --- ICE candidate from client (optional) ---
                elif t == "candidate":
                    candidate = data.get("candidate")
                    pc = clients[peer_id].get("pc")
                    if pc and candidate:
                        try:
                            # client may send as dict or SDP string; try to handle both
                            if isinstance(candidate, dict) and "candidate" in candidate:
                                raw = candidate.get("candidate")
                                # Attempt to create RTCIceCandidate (aiortc expects dict with 'candidate','sdpMid','sdpMLineIndex')
                                # We'll try best-effort; if fails ignore.
                                try:
                                    ice = RTCIceCandidate(
                                        sdpMid=candidate.get("sdpMid"),
                                        sdpMLineIndex=candidate.get("sdpMLineIndex"),
                                        candidate=raw,
                                    )
                                    await pc.addIceCandidate(ice)
                                except Exception:
                                    pass
                            elif isinstance(candidate, str):
                                # Not much we can do; skip
                                pass
                        except Exception:
                            logger.exception("adding candidate failed")

                # --- Remote control messages: mouse / scroll / keyboard ---
                elif t == "control":
                    # Expected data: {type:"control", action:"mouse"|"scroll"|"key", payload: {...}}
                    action = data.get("action")
                    payload = data.get("payload", {})
                    await handle_control(action, payload, request.app)
                elif t == "bye":
                    # client wants to disconnect
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

async def handle_control(action: str, payload: dict, app):
    """
    action: "mouse", "scroll", "key"
    mouse payload: {"event": "click"|"move"|"down"|"up", "x": <0..1>, "y": <0..1>, "button": "left"|"right"}
    scroll payload: {"dx": <float>, "dy": <float>}
    key payload: {"event":"down"|"up"|"press", "key": "a" or "enter"}
    Coordinates are normalized (0..1) relative to displayed WIDTHxHEIGHT.
    """
    try :
        if action == "mouse":
            en = payload.get("event")
            nx = payload.get("x")
            ny = payload.get("y")
            btn = payload.get("button" , "left")
            
            if nx is None or ny is None:
                return
            
            # map normalized coords to real monitor coords
            # get monitor size from mss (note: MONITOR_INDEX)
            with mss.mss() as sct:
                mon = sct.monitors[MONITOR_INDEX]
                real_w = mon["width"]
                real_h = mon["height"]
                rx = int(nx * real_w)
                ry = int(ny * real_h)