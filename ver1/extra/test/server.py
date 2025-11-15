"""
WebRTC Screen Capture Server in Python

Install dependencies:
pip install aiohttp aiortc mss pillow av numpy

Run:
python server.py

Then open the HTML viewer in your browser and connect to http://localhost:3030/offer
"""

import asyncio
import json
import logging
from aiohttp import web
import mss
import av
from aiortc import RTCPeerConnection, RTCSessionDescription, VideoStreamTrack
from aiortc.contrib.media import MediaBlackhole
from av import VideoFrame
import numpy as np
from PIL import Image

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)


class ScreenCaptureTrack(VideoStreamTrack):
    """
    A video track that captures the screen
    """
    
    def __init__(self):
        super().__init__()
        self.sct = mss.mss()
        self.monitor = self.sct.monitors[1]  # Primary monitor
        self.target_width = 1920
        self.target_height = 1080
        logger.info(f"Screen resolution: {self.monitor['width']}x{self.monitor['height']}")
        logger.info(f"Streaming at: {self.target_width}x{self.target_height}")
    
    async def recv(self):
        """
        Capture screen frame and return as VideoFrame
        """
        pts, time_base = await self.next_timestamp()
        
        # Capture screen
        screenshot = self.sct.grab(self.monitor)
        
        # Convert to PIL Image
        img = Image.frombytes('RGB', screenshot.size, screenshot.bgra, 'raw', 'BGRX')
        
        # Resize if needed
        if img.size != (self.target_width, self.target_height):
            img = img.resize((self.target_width, self.target_height), Image.NEAREST)
        
        # Convert to numpy array
        img_array = np.array(img)
        
        # Create VideoFrame
        frame = VideoFrame.from_ndarray(img_array, format='rgb24')
        frame.pts = pts
        frame.time_base = time_base
        
        return frame


# Store peer connections
pcs = set()


async def offer(request):
    """
    Handle WebRTC offer from browser
    """
    params = await request.json()
    offer_sdp = params.get("sdp")
    
    if not offer_sdp:
        return web.Response(status=400, text="Missing SDP")
    
    # Create RTCPeerConnection
    pc = RTCPeerConnection()
    pcs.add(pc)
    
    @pc.on("connectionstatechange")
    async def on_connectionstatechange():
        logger.info(f"Connection state: {pc.connectionState}")
        if pc.connectionState == "failed" or pc.connectionState == "closed":
            await pc.close()
            pcs.discard(pc)
    
    # Add screen capture track
    screen_track = ScreenCaptureTrack()
    pc.addTrack(screen_track)
    
    # Set remote description (offer from browser)
    await pc.setRemoteDescription(RTCSessionDescription(sdp=offer_sdp, type="offer"))
    
    # Create answer
    answer = await pc.createAnswer()
    await pc.setLocalDescription(answer)
    
    logger.info("Answer created successfully!")
    
    return web.Response(
        content_type="application/json",
        text=json.dumps({"sdp": pc.localDescription.sdp}),
    )


async def on_shutdown(app):
    """
    Close all peer connections on shutdown
    """
    coros = [pc.close() for pc in pcs]
    await asyncio.gather(*coros)
    pcs.clear()


def main():
    """
    Start the web server
    """
    app = web.Application()
    
    # Enable CORS
    app.router.add_post("/offer", offer)
    
    # Add CORS middleware
    async def cors_middleware(app, handler):
        async def middleware_handler(request):
            if request.method == "OPTIONS":
                response = web.Response()
            else:
                response = await handler(request)
            
            response.headers['Access-Control-Allow-Origin'] = '*'
            response.headers['Access-Control-Allow-Methods'] = 'POST, GET, OPTIONS'
            response.headers['Access-Control-Allow-Headers'] = 'Content-Type'
            return response
        return middleware_handler
    
    app.middlewares.append(cors_middleware)
    app.on_shutdown.append(on_shutdown)
    
    logger.info("Server running on http://0.0.0.0:3030")
    logger.info("Open the HTML viewer and connect to http://localhost:3030/offer")
    
    web.run_app(app, host="0.0.0.0", port=3030)


if __name__ == "__main__":
    main()