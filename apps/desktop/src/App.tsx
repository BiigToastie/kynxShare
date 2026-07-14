import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppConfig, EngineSnapshot, OutputMode } from "./types";

type DragMode = "move" | "resize";

const OUTPUT_PRESETS = [
  { id: "native", label: "Native", w: 0, h: 0 },
  { id: "1080", label: "1080p", w: 1920, h: 1080 },
  { id: "1440", label: "1440p", w: 2560, h: 1440 },
  { id: "4k", label: "4K", w: 3840, h: 2160 },
] as const;

export default function App() {
  const [snap, setSnap] = useState<EngineSnapshot | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [layoutPreview, setLayoutPreview] = useState<string | null>(null);
  const [outputPreview, setOutputPreview] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [wizard, setWizard] = useState(false);
  const [fitScale, setFitScale] = useState(0.15);
  const [zoom, setZoom] = useState(1); // multiplier on fit
  const stageRef = useRef<HTMLDivElement>(null);
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

  const canvasW = Math.max(
    1,
    snap?.status.canvas_width || config?.layout.canvas_width || 1920
  );
  const canvasH = Math.max(
    1,
    snap?.status.canvas_height || config?.layout.canvas_height || 1080
  );

  const scale = fitScale * zoom;

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
      const pad = 32;
      const s = Math.min((rect.width - pad) / canvasW, (rect.height - pad) / canvasH);
      setFitScale(Number.isFinite(s) && s > 0 ? s : 0.1);
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
      // Uniform scale from bottom-right — keeps aspect ratio
      const baseW = mon.width;
      const newW = Math.max(240, baseW * drag.origScale + dx);
      const scaleVal = Math.max(0.35, Math.min(2.5, newW / baseW));
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
    await save({
      ...config,
      layout: { ...config.layout, placements },
      selected_monitor_ids: placements.filter((p) => p.enabled).map((p) => p.monitor_id),
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
          height: Math.max(180, Math.round((radius * 2 * 9) / 16)),
        },
      },
    };
    setConfig(next);
    await invoke("save_config", { config: next });
  };

  const applyOutputPreset = async (w: number, h: number) => {
    if (!config) return;
    const next = {
      ...config,
      layout: {
        ...config.layout,
        max_width: w === 0 ? 16384 : w,
        max_height: h === 0 ? 16384 : h,
      },
    };
    await save(next);
    await refresh();
  };

  const setOutputSize = async (field: "max_width" | "max_height", value: number) => {
    if (!config) return;
    const next = {
      ...config,
      layout: { ...config.layout, [field]: Math.max(0, value) },
    };
    setConfig(next);
  };

  const activePreset = useMemo(() => {
    if (!config) return "custom";
    const { max_width: w, max_height: h } = config.layout;
    if (w >= 16000 && h >= 16000) return "native";
    const hit = OUTPUT_PRESETS.find((p) => p.w === w && p.h === h);
    return hit?.id ?? "custom";
  }, [config]);

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

  const resetLayout = async () => {
    if (!config || !snap) return;
    let x = 0;
    const placements = [...snap.monitors]
      .sort((a, b) => a.x - b.x || a.y - b.y)
      .map((m) => {
        const p = {
          monitor_id: m.id,
          enabled: true,
          x,
          y: 0,
          scale: 1,
        };
        x += m.width;
        return p;
      });
    await save({
      ...config,
      layout: { ...config.layout, placements },
      selected_monitor_ids: placements.map((p) => p.monitor_id),
    });
    setZoom(1);
    await refresh();
  };

  if (!config || !snap) {
    return (
      <div className="app" style={{ placeItems: "center", display: "grid" }}>
        <p className="empty-hint">kynxShare wird geladen…</p>
      </div>
    );
  }

  const radius = config.layout.follow.radius || Math.round((config.layout.follow.width || 1920) / 2);
  const outW = snap.status.output_width || canvasW;
  const outH = snap.status.output_height || canvasH;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <h1>kynxShare</h1>
          <span>Multi-Monitor Stream Output</span>
        </div>
        <div className="top-actions">
          <span className={`status-pill ${snap.status.output_active ? "on" : ""}`}>
            <span className="dot" />
            {snap.status.running
              ? snap.status.output_active
                ? "Live"
                : "Pausiert"
              : "Bereit"}
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
                {snap.status.output_active ? "Pause" : "Resume"}
              </button>
              <button onClick={stop}>Stop</button>
            </>
          )}
        </div>
      </header>

      <div className="main">
        <section className="preview-pane">
          {/* Layout — top half */}
          <div className="stage-block">
            <div className="stage-header">
              <div className="stage-title">
                <h2>Layout</h2>
                <span className="badge">
                  {canvasW}×{canvasH}
                </span>
                <span className="badge">{snap.monitors.length} Displays</span>
              </div>
              <div className="stage-tools">
                <span className="meta">{Math.round(zoom * 100)}%</span>
                <button className="chip ghost" onClick={() => setZoom((z) => Math.max(0.5, z - 0.25))}>
                  −
                </button>
                <button className="chip ghost" onClick={() => setZoom(1)}>
                  Fit
                </button>
                <button className="chip ghost" onClick={() => setZoom((z) => Math.min(4, z + 0.25))}>
                  +
                </button>
                <button className="chip ghost" onClick={resetLayout}>
                  Reset
                </button>
              </div>
            </div>
            <div
              className="preview-stage"
              ref={stageRef}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerCancel={onPointerUp}
            >
              <div
                className="canvas-world"
                style={{
                  width: canvasW * scale,
                  height: canvasH * scale,
                }}
              >
                {layoutPreview ? (
                  <img className="canvas-image" src={layoutPreview} alt="Layout" draggable={false} />
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
                          width: w,
                          height: h,
                        }}
                        onPointerDown={(e) => onPointerDown(e, p.monitor_id, "move")}
                      >
                        <span className="tile-label">
                          {mon.name}
                          <small>
                            {Math.round(mon.width * p.scale)}×{Math.round(mon.height * p.scale)}
                            {p.scale !== 1 ? ` · ${Math.round(p.scale * 100)}%` : ""}
                          </small>
                        </span>
                        <div
                          className="resize-handle"
                          onPointerDown={(e) => onPointerDown(e, p.monitor_id, "resize")}
                          title="Skalieren (Seitenverhältnis bleibt)"
                        />
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>
          </div>

          {/* Output — bottom half, same size */}
          <div className="stage-block">
            <div className="stage-header">
              <div className="stage-title">
                <h2>Output</h2>
                <span className={`badge ${snap.status.output_active ? "live" : ""}`}>
                  {outW}×{outH}
                </span>
                <span className="badge">
                  {config.layout.mode === "mouse_follow" ? "Mouse Follow" : "Static"}
                </span>
                {snap.status.running ? (
                  <span className="badge">{snap.status.fps.toFixed(0)} fps</span>
                ) : null}
              </div>
            </div>
            <div className="output-stage">
              <div className="output-frame">
                {outputPreview ? (
                  <img src={outputPreview} alt="Stream output" draggable={false} />
                ) : (
                  <p className="empty-hint">
                    Starte die Capture-Engine, um den Stream-Output live zu sehen.
                  </p>
                )}
              </div>
            </div>
          </div>

          {error ? <p className="error-line">{error}</p> : null}
        </section>

        <aside className="sidebar">
          <div className="panel">
            <h3>Modus</h3>
            <div className="mode-toggle">
              <button
                className={config.layout.mode === "static_layout" ? "active" : ""}
                onClick={() => setMode("static_layout")}
              >
                Static
              </button>
              <button
                className={config.layout.mode === "mouse_follow" ? "active" : ""}
                onClick={() => setMode("mouse_follow")}
              >
                Follow
              </button>
            </div>
            {config.layout.mode === "mouse_follow" ? (
              <div className="slider-block">
                <div className="row">
                  <label>Sichtfeld-Radius</label>
                  <span className="meta" style={{ fontFamily: "var(--mono)", fontSize: "0.75rem", color: "var(--muted)" }}>
                    {radius * 2}×{Math.round(radius * 2 * 9 / 16)}
                  </span>
                </div>
                <input
                  type="range"
                  min={320}
                  max={1920}
                  step={16}
                  value={radius}
                  onChange={(e) => setRadius(Number(e.target.value))}
                />
                <p className="hint">
                  Ausschnitt um den Cursor — live unten im Output sichtbar.
                </p>
              </div>
            ) : (
              <p className="hint">
                Monitore oben anordnen. Unten siehst du exakt den Stream.
              </p>
            )}
          </div>

          <div className="panel">
            <h3>Output-Größe</h3>
            <div className="chip-row">
              {OUTPUT_PRESETS.map((p) => (
                <button
                  key={p.id}
                  className={`chip ${activePreset === p.id ? "active" : "ghost"}`}
                  onClick={() => applyOutputPreset(p.w, p.h)}
                >
                  {p.label}
                </button>
              ))}
            </div>
            <div className="size-grid">
              <label>
                Max Breite
                <input
                  type="number"
                  value={config.layout.max_width}
                  onChange={(e) => setOutputSize("max_width", Number(e.target.value) || 0)}
                  onBlur={() => save(config).then(refresh)}
                />
              </label>
              <label>
                Max Höhe
                <input
                  type="number"
                  value={config.layout.max_height}
                  onChange={(e) => setOutputSize("max_height", Number(e.target.value) || 0)}
                  onBlur={() => save(config).then(refresh)}
                />
              </label>
            </div>
            <p className="hint">
              Native = volle Layout-Auflösung. Presets skalieren proportional in die Box.
              Aktuell: <strong style={{ color: "var(--text)" }}>{outW}×{outH}</strong>
            </p>
          </div>

          <div className="panel">
            <h3>Monitore</h3>
            <div className="monitor-list">
              {snap.monitors.map((m) => {
                const p = config.layout.placements.find((x) => x.monitor_id === m.id);
                return (
                  <div className="monitor-item" key={m.id}>
                    <div>
                      <strong>{m.name}</strong>
                      <small>
                        {m.width}×{m.height}
                        {p ? ` · ${Math.round((p.scale || 1) * 100)}%` : ""}
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
              style={{ marginTop: "0.55rem", width: "100%" }}
              onClick={() => invoke("refresh_monitors").then(refresh)}
            >
              Displays aktualisieren
            </button>
          </div>

          <div className="panel">
            <h3>Stream</h3>
            <div className="row">
              <label>FPS</label>
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
              <label>Share-Fenster</label>
              <button
                className={config.outputs.show_share_window ? "active" : "ghost"}
                onClick={() =>
                  save({
                    ...config,
                    outputs: {
                      ...config.outputs,
                      share_window: true,
                      show_share_window: !config.outputs.show_share_window,
                    },
                  }).then(refresh)
                }
              >
                {config.outputs.show_share_window ? "Sichtbar" : "Versteckt"}
              </button>
            </div>
            <p className="hint">
              Für Discord: Fenster kurz sichtbar machen und{" "}
              <em>kynxShare Output</em> wählen.
            </p>
          </div>
        </aside>
      </div>

      {wizard ? (
        <div className="wizard">
          <div className="wizard-card">
            <h2>Willkommen bei kynxShare</h2>
            <p className="hint">
              Oben arrangierst du deine Monitore, unten siehst du den fertigen Stream — gleich groß.
            </p>
            <ol>
              <li>Monitore per Drag anordnen, Zoom mit +/− bei Bedarf.</li>
              <li>Output-Größe wählen (Native / 1080p / …).</li>
              <li>Start → in Discord Fenster „kynxShare Output“ teilen.</li>
            </ol>
            <div className="wizard-actions">
              <button className="primary" onClick={finishWizard}>
                Los geht’s
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
