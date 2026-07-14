import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppConfig, EngineSnapshot, OutputMode } from "./types";

type DragMode = "move" | "resize";

export default function App() {
  const [snap, setSnap] = useState<EngineSnapshot | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [layoutPreview, setLayoutPreview] = useState<string | null>(null);
  const [outputPreview, setOutputPreview] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [wizard, setWizard] = useState(false);
  const [scale, setScale] = useState(0.2);
  const stageRef = useRef<HTMLDivElement>(null);
  const worldRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{
    id: string;
    mode: DragMode;
    startX: number;
    startY: number;
    origX: number;
    origY: number;
    origScale: number;
  } | null>(null);
  const configRef = useRef(config);
  configRef.current = config;

  const canvasW = snap?.status.canvas_width || config?.layout.canvas_width || 1920;
  const canvasH = snap?.status.canvas_height || config?.layout.canvas_height || 1080;

  const refresh = useCallback(async () => {
    try {
      const [s, c, lp, op] = await Promise.all([
        invoke<EngineSnapshot>("get_snapshot"),
        invoke<AppConfig>("get_config"),
        invoke<string | null>("get_preview"),
        invoke<string | null>("get_output_preview"),
      ]);
      setSnap(s);
      setConfig({
        ...c,
        outputs: {
          ...c.outputs,
          show_share_window: c.outputs.show_share_window ?? false,
        },
        layout: {
          ...c.layout,
          follow: {
            ...c.layout.follow,
            radius: c.layout.follow.radius ?? 960,
          },
        },
      });
      setLayoutPreview(lp);
      setOutputPreview(op);
      setWizard(!c.onboarding_done);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    const id = window.setInterval(async () => {
      try {
        const [s, lp, op] = await Promise.all([
          invoke<EngineSnapshot>("get_snapshot"),
          invoke<string | null>("get_preview"),
          invoke<string | null>("get_output_preview"),
        ]);
        setSnap(s);
        setLayoutPreview(lp);
        setOutputPreview(op);
      } catch {
        /* ignore */
      }
    }, 200);
    return () => window.clearInterval(id);
  }, [refresh]);

  useLayoutEffect(() => {
    const el = stageRef.current;
    if (!el) return;
    const update = () => {
      const rect = el.getBoundingClientRect();
      const pad = 24;
      const s = Math.min(
        (rect.width - pad) / canvasW,
        (rect.height - pad) / canvasH,
        1
      );
      setScale(Number.isFinite(s) && s > 0 ? s : 0.2);
    };
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => ro.disconnect();
  }, [canvasW, canvasH]);

  const save = async (next: AppConfig) => {
    setConfig(next);
    await invoke("save_config", { config: next });
  };

  const onPointerDown = (e: React.PointerEvent, id: string, mode: DragMode) => {
    e.preventDefault();
    e.stopPropagation();
    if (!config) return;
    const p = config.layout.placements.find((x) => x.monitor_id === id);
    if (!p) return;
    dragRef.current = {
      id,
      mode,
      startX: e.clientX,
      startY: e.clientY,
      origX: p.x,
      origY: p.y,
      origScale: p.scale,
    };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const drag = dragRef.current;
    const cfg = configRef.current;
    if (!drag || !cfg || !snap) return;
    const dx = (e.clientX - drag.startX) / scale;
    const dy = (e.clientY - drag.startY) / scale;
    const mon = snap.monitors.find((m) => m.id === drag.id);
    if (!mon) return;

    const placements = cfg.layout.placements.map((p) => {
      if (p.monitor_id !== drag.id) return p;
      if (drag.mode === "move") {
        let x = Math.round(drag.origX + dx);
        let y = Math.round(drag.origY + dy);
        const grid = 8;
        x = Math.round(x / grid) * grid;
        y = Math.round(y / grid) * grid;
        return { ...p, x, y };
      }
      // resize from bottom-right: change scale
      const baseW = mon.width;
      const newW = Math.max(160, baseW * drag.origScale + dx);
      const scaleVal = Math.max(0.25, Math.min(3, newW / baseW));
      return { ...p, scale: Math.round(scaleVal * 100) / 100 };
    });
    setConfig({ ...cfg, layout: { ...cfg.layout, placements } });
  };

  const onPointerUp = async () => {
    if (!dragRef.current || !configRef.current) return;
    dragRef.current = null;
    await save(configRef.current);
    await refresh();
  };

  const toggleMonitor = async (id: string) => {
    if (!config) return;
    const placements = config.layout.placements.map((p) =>
      p.monitor_id === id ? { ...p, enabled: !p.enabled } : p
    );
    const selected = placements.filter((p) => p.enabled).map((p) => p.monitor_id);
    await save({
      ...config,
      layout: { ...config.layout, placements },
      selected_monitor_ids: selected,
    });
    await refresh();
  };

  const setMode = async (mode: OutputMode) => {
    if (!config) return;
    await save({ ...config, layout: { ...config.layout, mode } });
    await refresh();
  };

  const setRadius = async (radius: number) => {
    if (!config) return;
    const next = {
      ...config,
      layout: {
        ...config.layout,
        follow: {
          ...config.layout.follow,
          radius,
          width: radius * 2,
          height: Math.max(180, Math.round(radius * 2 * 9 / 16)),
        },
      },
    };
    setConfig(next);
    await invoke("save_config", { config: next });
  };

  const start = async () => {
    try {
      await invoke("start_engine");
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const stop = async () => {
    await invoke("stop_engine");
    await refresh();
  };

  const finishWizard = async () => {
    if (!config) return;
    await save({ ...config, onboarding_done: true });
    setWizard(false);
    await start();
  };

  if (!config || !snap) {
    return (
      <div className="app" style={{ placeItems: "center", display: "grid" }}>
        <p className="hint">Loading kynxShare…</p>
      </div>
    );
  }

  const follow = config.layout.follow;
  const radius = follow.radius || Math.round((follow.width || 1920) / 2);

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <h1>kynxShare</h1>
          <span>Multi-monitor composite for Discord &amp; OBS</span>
        </div>
        <div className="top-actions">
          <span className={`status-pill ${snap.status.output_active ? "on" : ""}`}>
            <span className="dot" />
            {snap.status.running
              ? snap.status.output_active
                ? "Output live"
                : "Paused"
              : "Idle"}
          </span>
          {!snap.status.running ? (
            <button className="primary" onClick={start}>
              Start
            </button>
          ) : (
            <>
              <button
                onClick={() =>
                  invoke("set_output_active", {
                    active: !snap.status.output_active,
                  }).then(refresh)
                }
              >
                {snap.status.output_active ? "Pause output" : "Resume output"}
              </button>
              <button onClick={stop}>Stop</button>
            </>
          )}
        </div>
      </header>

      <div className="main">
        <section className="preview-pane">
          <div className="preview-header">
            <h2>Layout editor</h2>
            <div className="preview-meta">
              Canvas {canvasW}×{canvasH} · {snap.status.fps.toFixed(0)} fps ·{" "}
              {snap.monitors.length} monitors
            </div>
          </div>

          <div
            className="preview-stage"
            ref={stageRef}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerLeave={onPointerUp}
          >
            <div
              className="canvas-world"
              ref={worldRef}
              style={{
                width: canvasW * scale,
                height: canvasH * scale,
              }}
            >
              {layoutPreview ? (
                <img
                  className="canvas-image"
                  src={layoutPreview}
                  alt="Layout"
                  draggable={false}
                />
              ) : (
                <div className="canvas-placeholder" />
              )}
              <div className="overlay-tiles">
                {config.layout.placements.map((p) => {
                  const mon = snap.monitors.find((m) => m.id === p.monitor_id);
                  if (!mon) return null;
                  const w = mon.width * p.scale * scale;
                  const h = mon.height * p.scale * scale;
                  return (
                    <div
                      key={p.monitor_id}
                      className={`tile ${p.enabled ? "" : "disabled"}`}
                      style={{
                        left: p.x * scale,
                        top: p.y * scale,
                        width: Math.max(40, w),
                        height: Math.max(28, h),
                      }}
                      onPointerDown={(e) => onPointerDown(e, p.monitor_id, "move")}
                    >
                      <span className="tile-label">
                        {mon.name}
                        <small>
                          {Math.round(mon.width * p.scale)}×
                          {Math.round(mon.height * p.scale)}
                        </small>
                      </span>
                      <div
                        className="resize-handle"
                        onPointerDown={(e) => onPointerDown(e, p.monitor_id, "resize")}
                        title="Resize"
                      />
                    </div>
                  );
                })}
              </div>
            </div>
          </div>

          <div className="output-dock">
            <div className="output-dock-header">
              <h2>Output preview</h2>
              <span className="preview-meta">
                {snap.status.output_width || "—"}×{snap.status.output_height || "—"}
                {config.layout.mode === "mouse_follow" ? " · mouse follow" : " · static"}
              </span>
            </div>
            <div className="output-frame">
              {outputPreview ? (
                <img src={outputPreview} alt="Stream output" draggable={false} />
              ) : (
                <p className="hint">Start capture to see the stream view</p>
              )}
            </div>
          </div>

          {error ? (
            <p className="hint" style={{ color: "var(--danger)" }}>
              {error}
            </p>
          ) : null}
        </section>

        <aside className="sidebar">
          <div className="panel">
            <h3>Mode</h3>
            <div className="stack">
              <button
                className={config.layout.mode === "static_layout" ? "active" : ""}
                onClick={() => setMode("static_layout")}
              >
                Static layout
              </button>
              <button
                className={config.layout.mode === "mouse_follow" ? "active" : ""}
                onClick={() => setMode("mouse_follow")}
              >
                Mouse follow
              </button>
            </div>
            {config.layout.mode === "mouse_follow" ? (
              <div className="slider-block">
                <div className="row">
                  <label>Maus-Radius</label>
                  <span className="preview-meta">{radius}px</span>
                </div>
                <input
                  type="range"
                  min={240}
                  max={1920}
                  step={16}
                  value={radius}
                  onChange={(e) => setRadius(Number(e.target.value))}
                />
                <p className="hint">
                  Sichtfeld um den Cursor ({radius * 2}×
                  {Math.round(radius * 2 * 9 / 16)}). Live in der Output-Preview.
                </p>
              </div>
            ) : (
              <p className="hint" style={{ marginTop: "0.75rem" }}>
                Ziehe Monitore im Layout-Editor. Rechts unten siehst du den Stream-Output.
              </p>
            )}
          </div>

          <div className="panel">
            <h3>Monitors ({snap.monitors.length})</h3>
            <div className="monitor-list">
              {snap.monitors.map((m) => {
                const p = config.layout.placements.find((x) => x.monitor_id === m.id);
                return (
                  <div className="monitor-item" key={m.id}>
                    <div>
                      <strong>{m.name}</strong>
                      <small>
                        {m.width}×{m.height} · ({m.x},{m.y})
                      </small>
                    </div>
                    <button
                      className={p?.enabled ? "active" : "ghost"}
                      onClick={() => toggleMonitor(m.id)}
                    >
                      {p?.enabled ? "On" : "Off"}
                    </button>
                  </div>
                );
              })}
            </div>
            <button
              className="ghost"
              style={{ marginTop: "0.65rem", width: "100%" }}
              onClick={() => invoke("refresh_monitors").then(refresh)}
            >
              Refresh displays
            </button>
          </div>

          <div className="panel">
            <h3>Output</h3>
            <div className="row">
              <label>Target FPS</label>
              <input
                type="number"
                value={config.target_fps}
                onChange={(e) =>
                  setConfig({ ...config, target_fps: Number(e.target.value) || 30 })
                }
                onBlur={() => save(config)}
              />
            </div>
            <div className="row">
              <label>Share window</label>
              <button
                className={config.outputs.share_window ? "active" : "ghost"}
                onClick={() =>
                  save({
                    ...config,
                    outputs: {
                      ...config.outputs,
                      share_window: !config.outputs.share_window,
                    },
                  })
                }
              >
                {config.outputs.share_window ? "On" : "Off"}
              </button>
            </div>
            <div className="row">
              <label>Fenster zeigen</label>
              <button
                className={config.outputs.show_share_window ? "active" : "ghost"}
                onClick={() =>
                  save({
                    ...config,
                    outputs: {
                      ...config.outputs,
                      show_share_window: !config.outputs.show_share_window,
                    },
                  }).then(refresh)
                }
              >
                {config.outputs.show_share_window ? "Sichtbar" : "Versteckt"}
              </button>
            </div>
            <p className="hint">
              Output-Fenster bleibt standardmäßig versteckt. Für Discord kurz auf
              „Sichtbar“ stellen und <em>kynxShare Output</em> als Fenster teilen.
            </p>
          </div>
        </aside>
      </div>

      {wizard ? (
        <div className="wizard">
          <div className="wizard-card">
            <h2>Welcome to kynxShare</h2>
            <p className="hint">
              Monitore im Layout-Editor verschieben und skalieren. Die Output-Preview
              zeigt live, was gestreamt wird.
            </p>
            <ol>
              <li>Monitore aktivieren und anordnen.</li>
              <li>Optional: Mouse-Follow mit Radius-Slider.</li>
              <li>
                Start → Discord: Fenster <em>kynxShare Output</em> (Fenster zeigen falls nötig).
              </li>
            </ol>
            <div className="wizard-actions">
              <button className="primary" onClick={finishWizard}>
                Got it — start
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
