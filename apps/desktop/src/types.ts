export type OutputMode = "static_layout" | "mouse_follow";

export interface MonitorInfo {
  id: string;
  name: string;
  device_name: string;
  adapter_index: number;
  output_index: number;
  x: number;
  y: number;
  width: number;
  height: number;
  refresh_hz: number;
  is_primary: boolean;
  scale_percent: number;
}

export interface MonitorPlacement {
  monitor_id: string;
  enabled: boolean;
  x: number;
  y: number;
  scale: number;
}

export interface MouseFollowConfig {
  width: number;
  height: number;
  edge_padding: number;
  smoothing: number;
  radius: number;
}

export interface LayoutConfig {
  placements: MonitorPlacement[];
  canvas_width: number | null;
  canvas_height: number | null;
  max_width: number;
  max_height: number;
  mode: OutputMode;
  follow: MouseFollowConfig;
  background_bgra: [number, number, number, number];
}

export interface OutputChannels {
  share_window: boolean;
  virtual_camera: boolean;
  always_on_top: boolean;
  show_share_window: boolean;
  virtual_display: boolean;
  ui_live_preview: boolean;
}

export interface AppConfig {
  layout: LayoutConfig;
  outputs: OutputChannels;
  share_window: { title: string; always_on_top: boolean; visible: boolean };
  virtual_camera: { enabled: boolean; max_width: number; max_height: number };
  target_fps: number;
  start_with_windows: boolean;
  onboarding_done: boolean;
  selected_monitor_ids: string[];
}

export interface EngineStatus {
  running: boolean;
  output_active: boolean;
  mode: OutputMode;
  fps: number;
  canvas_width: number;
  canvas_height: number;
  output_width: number;
  output_height: number;
  monitor_count: number;
}

export interface VirtualDisplayStatus {
  installed: boolean;
  adapter_name: string | null;
  guidance: string;
  driver_ok: boolean;
  active_index: number | null;
  monitor_device: string | null;
  plug_disabled: boolean;
}

export interface EngineSnapshot {
  status: EngineStatus;
  monitors: MonitorInfo[];
  layout: LayoutConfig;
  vdd: VirtualDisplayStatus;
  layout_preview_jpeg_base64: string | null;
  output_preview_jpeg_base64: string | null;
  preview_jpeg_base64: string | null;
}
