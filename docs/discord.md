# Discord setup

## Recommended: Share window

1. Start **kynxShare** and click **Start** (or finish the welcome wizard).
2. Confirm the status pill shows **Output live**.
3. A window named **kynxShare Output** appears with your composite.
4. In Discord: **Share Your Screen** → **Application / Window** → select **kynxShare Output**.
5. Arrange monitors in the kynxShare preview; the shared window updates live.

Tips:

- Use **Static layout** to show all enabled monitors at once.
- Use **Mouse follow** for a fixed viewport (e.g. 1920×1080) that tracks the cursor across monitors.
- Lower **Max width/height** if Discord struggles with ultra-wide canvases.

## Optional: Virtual Display Driver (true “Screen” source)

Install the signed open-source driver:

- GitHub: https://github.com/VirtualDrivers/Virtual-Display-Driver  
- Winget: `winget install --id=VirtualDrivers.Virtual-Display-Driver -e`

Then:

1. Set the virtual display resolution to match your kynxShare canvas (or follow viewport).
2. In Discord choose **Screen** and pick the virtual monitor.
3. Present / fullscreen the **kynxShare Output** window onto that virtual display (Windows display settings → rearrange / set as extended display, then move the output window there and maximize).

kynxShare detects common VDD adapter names and shows status in the Channels panel.

## Optional: Virtual camera

Enable **Virtual camera** in Channels. Frames are published to shared memory for a companion DirectShow/MF filter (see [virtual-camera.md](virtual-camera.md)). Until a filter is installed, prefer the share-window path for Discord.
