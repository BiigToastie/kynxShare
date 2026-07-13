import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppConfig, EngineSnapshot, OutputMode } from "./types";

export default function App() {
  const [snap, setSnap] = useState<EngineSnapshot | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [wizard, setWizard] = useState(false);
  const stageRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ id: string; ox: number; oy: number } | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [s, c, p] = await Promise.all([
        invoke<EngineSnapshot>("get_snapshot"),
        invoke<AppConfig>("get_config"),
        invoke<string | null>("get_preview"),
      ]);
      setSnap(s);
      setConfig(c);
      setPreview(p);
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
        const [s, p] = await Promise.all([
          invoke<EngineSnapshot>("get_snapshot"),
          invoke<string | null>("get_preview"),
        ]);
        setSnap(s);
        setPreview(p);
      } catch {
        /* ignore poll errors */
      }
    }, 500);
    return () => window.clearInterval(id);
  }, [refresh]);

  const save = async (next: AppConfig) => {
    setConfig(next);
    await invoke("save_config", { config: next });
    await refresh();
  };

  const canvas = useMemo(() => {
    if (!snap) return { w: 1920, h: 1080 };
    return {
      w: snap.status.canvas_width || 1920,
      h: snap.status.canvas_height || 1080,
    };
  }, [snap]);

  const scale = useMemo(() => {
    const el = stageRef.current;
    if (!el) return 0.25;
    const rect = el.getBoundingClientRect();
    return Math.min(rect.width / canvas.w, rect.height / canvas.h, 1);
  }, [canvas, preview, snap]);

  const onPointerDown = (e: React.PointerEvent, id: string) => {
    if (!config) return;
    const p = config.layout.placements.find((x) => x.monitor_id === id);
    if (!p) return;
    dragRef.current = {
      id,
      ox: e.clientX - p.x * scale,
      oy: e.clientY - p.y * scale,
    };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    if (!dragRef.current || !config) return;
    const { id, ox, oy } = dragRef.current;
    let x = Math.round((e.clientX - ox) / scale);
    let y = Math.round((e.clientY - oy) / scale);
    const grid = 16;
    x = Math.round(x / grid) * grid;
    y = Math.round(y / grid) * grid;
    const placements = config.layout.placements.map((p) =>
      p.monitor_id === id ? { ...p, x, y } : p
    );
    setConfig({ ...config, layout: { ...config.layout, placements } });
  };

  const onPointerUp = async () => {
    if (!dragRef.current || !config) return;
    dragRef.current = null;
    await save(config);
  };

  const toggleMonitor = async (id: string) => {
    if (!config) return;
    const placements = config.layout.placements.map((p) =>
      p.monitor_id === id ? { ...p, enabled: !p.enabled } : p
    );
    const selected = placements.filter((p) => p.enabled).map((p) => p.monitor_id);
    await save({ ...config, layout: { ...config.layout, placements }, selected_monitor_ids: selected });
  };

  const setMode = async (mode: OutputMode) => {
    if (!config) return;
    await save({ ...config, layout: { ...config.layout, mode } });
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
                  invoke("set_output_active", { active: !snap.status.output_active }).then(refresh)
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
            <h2>Live preview</h2>
            <div className="preview-meta">
              {snap.status.canvas_width}×{snap.status.canvas_height} ·{" "}
              {snap.status.fps.toFixed(0)} fps ·{" "}
              {config.layout.mode === "mouse_follow" ? "Mouse follow" : "Static layout"}
            </div>
          </div>
          <div
            className="preview-stage"
            ref={stageRef}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
          >
            {preview ? <img src={preview} alt="Composite preview" /> : null}
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
                      width: Math.max(48, w),
                      height: Math.max(36, h),
                    }}
                    onPointerDown={(e) => onPointerDown(e, p.monitor_id)}
                  >
                    {mon.name}
                  </div>
                );
              })}
            </div>
          </div>
          {error ? <p className="hint" style={{ color: "var(--danger)" }}>{error}</p> : null}
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
            <p className="hint" style={{ marginTop: "0.75rem" }}>
              Static streams the full arrangement. Mouse follow crops a viewport that tracks your cursor across monitors.
            </p>
          </div>

          <div className="panel">
            <h3>Monitors</h3>
            <div className="monitor-list">
              {snap.monitors.map((m) => {
                const p = config.layout.placements.find((x) => x.monitor_id === m.id);
                return (
                  <div className="monitor-item" key={m.id}>
                    <div>
                      <strong>{m.name}</strong>
                      <small>
                        {m.width}×{m.height}@{m.refresh_hz}Hz
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
            <h3>Output size</h3>
            <div className="row">
              <label>Max width</label>
              <input
                type="number"
                value={config.layout.max_width}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    layout: { ...config.layout, max_width: Number(e.target.value) || 0 },
                  })
                }
                onBlur={() => save(config)}
              />
            </div>
            <div className="row">
              <label>Max height</label>
              <input
                type="number"
                value={config.layout.max_height}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    layout: { ...config.layout, max_height: Number(e.target.value) || 0 },
                  })
                }
                onBlur={() => save(config)}
              />
            </div>
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
            {config.layout.mode === "mouse_follow" ? (
              <>
                <div className="row">
                  <label>Follow W</label>
                  <input
                    type="number"
                    value={config.layout.follow.width}
                    onChange={(e) =>
                      setConfig({
                        ...config,
                        layout: {
                          ...config.layout,
                          follow: {
                            ...config.layout.follow,
                            width: Number(e.target.value) || 1920,
                          },
                        },
                      })
                    }
                    onBlur={() => save(config)}
                  />
                </div>
                <div className="row">
                  <label>Follow H</label>
                  <input
                    type="number"
                    value={config.layout.follow.height}
                    onChange={(e) =>
                      setConfig({
                        ...config,
                        layout: {
                          ...config.layout,
                          follow: {
                            ...config.layout.follow,
                            height: Number(e.target.value) || 1080,
                          },
                        },
                      })
                    }
                    onBlur={() => save(config)}
                  />
                </div>
              </>
            ) : null}
          </div>

          <div className="panel">
            <h3>Channels</h3>
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
              <label>Virtual camera</label>
              <button
                className={config.outputs.virtual_camera ? "active" : "ghost"}
                onClick={() =>
                  save({
                    ...config,
                    outputs: {
                      ...config.outputs,
                      virtual_camera: !config.outputs.virtual_camera,
                    },
                    virtual_camera: {
                      ...config.virtual_camera,
                      enabled: !config.outputs.virtual_camera,
                    },
                  })
                }
              >
                {config.outputs.virtual_camera ? "On" : "Off"}
              </button>
            </div>
            <p className="hint">
              In Discord: Share screen → Application → <strong>kynxShare Output</strong>.
              Optional VDD: {snap.vdd.installed ? "detected" : "not installed"}.
            </p>
          </div>
        </aside>
      </div>

      {wizard ? (
        <div className="wizard">
          <div className="wizard-card">
            <h2>Welcome to kynxShare</h2>
            <p className="hint">
              Bundle any monitors into one streamable output. Drag tiles in the preview to arrange them.
            </p>
            <ol>
              <li>Enable the monitors you want to include.</li>
              <li>Arrange them in the live preview (snap-to-grid).</li>
              <li>
                Start output, then in Discord pick window <em>kynxShare Output</em>.
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
