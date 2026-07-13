# Virtual camera bridge

kynxShare publishes frames to a Windows file mapping:

- Name: `Local\KynxShareVirtualCam`
- Header size: 256 bytes
- Payload: tightly packed **BGRA8** immediately after the header

## Header (`VirtualCamHeader`)

| Offset | Type | Field |
|---|---|---|
| 0 | u32 | `magic` = `0x584E594B` (`KYNX`) |
| 4 | u32 | `version` = `1` |
| 8 | u32 | `width` |
| 12 | u32 | `height` |
| 16 | u32 | `stride` (bytes/row) |
| 20 | u32 | `format` (`0` = BGRA8) |
| 24 | u64 | `frame_id` (monotonic) |
| 32 | u64 | `timestamp_ms` |
| 40 | u32 | `data_offset` (usually 256) |
| 44 | u32 | `data_size` |

A DirectShow / Media Foundation filter can open this mapping and expose a webcam device named e.g. **kynxShare Cam** for Discord’s camera picker.

## Status

v0.1 ships the producer side. A signed virtual-cam filter may be added as a companion binary in a later release. Until then, use the **kynxShare Output** window in Discord.
