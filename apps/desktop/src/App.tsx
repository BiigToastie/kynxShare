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

function normalizeConfig(c: AppConfig): AppConfig {
  return {
    ...c,
    outputs: {
      ...c.outputs,
      show_share_window: c.outputs.show_share_window ?? false,
      virtual_display: c.outputs.virtual_display ?? true,
      ui_live_preview: c.outputs.ui_live_preview ?? true,
    },
    layout: {
      ...c.layout,
      max_width: c.layout.max_width ?? 0,
      max_height: c.layout.max_height ?? 0,
      follow: {
        ...c.layout.follow,
        radius: c.layout.follow.radius ?? 960,
      },
    },
  };
}

export default function App() {
  const [snap, setSnap] = useState<EngineSnapshot | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [savedJson, setSavedJson] = useState<string>("");
  const [layoutPreview, setLayoutPreview] = useState<string | null>(null);
  const [outputPreview, setOutputPreview] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [wizard, setWizard] = useState(false);
  const [fitScale, setFitScale] = useState(0.15);
  const [zoom, setZoom] = useState(1);
  const [saving, setSaving] = useState(false);
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
  const applyTimer = useRef<number | null>(null);

  const canvasW = Math.max(1, snap?.status.canvas_width || config?.layout.canvas_width || 1920);
  const canvasH = Math.max(1, snap?.status.canvas_height || config?.layout.canvas_height || 1080);
  const scale = fitScale * zoom;

  const dirty = useMemo(() => {
    if (!config || !savedJson) return false;
    return JSON.stringify(config) !== savedJson;
  }, [config, savedJson]);

  const applyLive = useCallback(async (next: AppConfig) => {
    setConfig(next);
    if (applyTimer.current) window.clearTimeout(applyTimer.current);
    applyTimer.current = window.setTimeout(async () => {
      try {
        await invoke("apply_config", { config: next });
      } catch (e) {
        setError(String(e));
      }
    }, 80);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [s, c, lp, op] = await Promise.all([
        invoke<EngineSnapshot>("get_snapshot"),
        invoke<AppConfig>("get_config"),
        invoke<string | null>("get_preview"),
        invoke<string | null>("get_output_preview"),
      ]);
      const normalized = normalizeConfig(c);
      setSnap(s);
      setConfig(normalized);
      setSavedJson(JSON.stringify(normalized));
      setLayoutPreview(lp);
      setOutputPreview(op);
      setWizard(!c.onboarding_done);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    (async () => {
      try {
        await invoke("ensure_preview");
      } catch (e) {
        setError(String(e));
      }
      await refresh();
    })();
    const id = window.setInterval(async () => {
      try {
        const previewOn = configRef.current?.outputs.ui_live_preview !== false;
        if (previewOn) {
          const [s, lp, op] = await Promise.all([
            invoke<EngineSnapshot>("get_snapshot"),
            invoke<string | null>("get_preview"),
            invoke<string | null>("get_output_preview"),
          ]);
          setSnap(s);
          setLayoutPreview(lp);
          setOutputPreview(op);
        } else {
          const s = await invoke<EngineSnapshot>("get_snapshot");
          setSnap(s);
        }
      } catch {
        /* ignore */
      }
    }, configRef.current?.outputs.ui_live_preview === false ? 500 : 250);
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

  const saveToDisk = async () => {
    if (!config) return;
    setSaving(true);
    try {
      await invoke("save_config", { config });
      setSavedJson(JSON.stringify(config));
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
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
    await applyLive(configRef.current);
  };

  const toggleMonitor = async (id: string) => {
    if (!config) return;
    const placements = config.layout.placements.map((p) =>
      p.monitor_id === id ? { ...p, enabled: !p.enabled } : p
    );
    await applyLive({
      ...config,
      layout: { ...config.layout, placements },
      selected_monitor_ids: placements.filter((p) => p.enabled).map((p) => p.monitor_id),
    });
  };

  const setMode = async (mode: OutputMode) => {
    if (!config) return;
    await applyLive({ ...config, layout: { ...config.layout, mode } });
  };

  const setRadius = async (radius: number) => {
    if (!config) return;
    await applyLive({
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
    });
  };

  const applyOutputPreset = async (w: number, h: number) => {
    if (!config) return;
    await applyLive({
      ...config,
      layout: {
        ...config.layout,
        max_width: w,
        max_height: h,
      },
    });
  };

  const setOutputSize = (field: "max_width" | "max_height", value: number) => {
    if (!config) return;
    const next = {
      ...config,
      layout: { ...config.layout, [field]: Math.max(0, value) },
    };
    setConfig(next);
  };

  const commitOutputSize = async () => {
    if (!config) return;
    await applyLive(config);
  };

  const activePreset = useMemo(() => {
    if (!config) return "custom";
    const { max_width: w, max_height: h } = config.layout;
    if (!w && !h) return "native";
    const hit = OUTPUT_PRESETS.find((p) => p.w === w && p.h === h);
    return hit?.id ?? "custom";
  }, [config]);

  const startStream = async () => {
    try {
      await invoke("start_engine");
      await refresh();
    } catch (e) {
      setError(String(e));
      await refresh();
    }
  };

  const stopAll = async () => {
    await invoke("stop_engine");
    try {
      await invoke("ensure_preview");
    } catch {
      /* ignore */
    }
    await refresh();
  };

  const finishWizard = async () => {
    if (!config) return;
    const next = { ...config, onboarding_done: true };
    await invoke("save_config", { config: next });
    setWizard(false);
    await invoke("apply_desktop_layout");
    await invoke("ensure_preview");
    await refresh();
  };

  const windowsLayout = async () => {
    try {
      await invoke("apply_desktop_layout");
      const [s, c, lp, op] = await Promise.all([
        invoke<EngineSnapshot>("get_snapshot"),
        invoke<AppConfig>("get_config"),
        invoke<string | null>("get_preview"),
        invoke<string | null>("get_output_preview"),
      ]);
      const normalized = normalizeConfig(c);
      setSnap(s);
      setConfig(normalized);
      // savedJson unverändert → Speichern-Button bleibt aktiv
      setLayoutPreview(lp);
      setOutputPreview(op);
    } catch (e) {
      setError(String(e));
    }
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
          {dirty ? <span className="badge">Ungespeichert</span> : null}
          <button
            className={dirty ? "primary" : "ghost"}
            disabled={!dirty || saving}
            onClick={saveToDisk}
          >
            {saving ? "Speichern…" : "Speichern"}
          </button>
          <span className={`status-pill ${snap.status.output_active ? "on" : ""}`}>
            <span className="dot" />
            {snap.status.output_active
              ? "Stream live"
              : snap.status.running
                ? "Preview"
                : "Bereit"}
          </span>
          {!snap.status.output_active ? (
            <button className="primary" onClick={startStream}>
              Stream starten
            </button>
          ) : (
            <>
              <button
                onClick={() =>
                  invoke("set_output_active", { active: false }).then(refresh)
                }
              >
                Stream pausieren
              </button>
              <button onClick={stopAll}>Stop</button>
            </>
          )}
        </div>
      </header>

      <div className="main">
        <section className="preview-pane">
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
                <button className="chip ghost" onClick={windowsLayout}>
                  Windows-Anordnung
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
                style={{ width: canvasW * scale, height: canvasH * scale }}
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
                          </small>
                        </span>
                        <div
                          className="resize-handle"
                          onPointerDown={(e) => onPointerDown(e, p.monitor_id, "resize")}
                          title="Skalieren"
                        />
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>
          </div>

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
                {config.outputs.ui_live_preview === false ? (
                  <p className="empty-hint">
                    Live-Preview aus — mehr Performance für den Stream.
                    {snap.status.output_active
                      ? " Discord nutzt den virtuellen Bildschirm / Output-Fenster."
                      : ""}
                  </p>
                ) : outputPreview ? (
                  <img src={outputPreview} alt="Stream output" draggable={false} />
                ) : (
                  <p className="empty-hint">Preview wird vorbereitet…</p>
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
                    {radius * 2}×{Math.round((radius * 2 * 9) / 16)}
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
              </div>
            ) : (
              <p className="hint">Output-Größe unten wählen — Preview aktualisiert sofort.</p>
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
                  onBlur={commitOutputSize}
                />
              </label>
              <label>
                Max Höhe
                <input
                  type="number"
                  value={config.layout.max_height}
                  onChange={(e) => setOutputSize("max_height", Number(e.target.value) || 0)}
                  onBlur={commitOutputSize}
                />
              </label>
            </div>
            <p className="hint">
              0 = Native (volle Layout-Auflösung). Presets skalieren proportional.
              Ergebnis: <strong style={{ color: "var(--text)" }}>{outW}×{outH}</strong>
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
                        {m.width}×{m.height} @ ({m.x},{m.y})
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
            <div className="stack" style={{ marginTop: "0.55rem" }}>
              <button className="ghost" onClick={windowsLayout}>
                Wie Windows anordnen
              </button>
              <button
                className="ghost"
                onClick={() => invoke("refresh_monitors").then(refresh)}
              >
                Displays aktualisieren
              </button>
            </div>
          </div>

          <div className="panel">
            <h3>Stream</h3>
            <div className="row">
              <label>FPS</label>
              <input
                type="number"
                value={config.target_fps}
                onChange={(e) =>
                  setConfig({ ...config, target_fps: Number(e.target.value) || 60 })
                }
                onBlur={() => config && applyLive(config)}
              />
            </div>
            <div className="row">
              <label>Live-Preview</label>
              <button
                className={config.outputs.ui_live_preview ? "active" : "ghost"}
                onClick={() =>
                  applyLive({
                    ...config,
                    outputs: {
                      ...config.outputs,
                      ui_live_preview: !config.outputs.ui_live_preview,
                    },
                  })
                }
              >
                {config.outputs.ui_live_preview ? "An" : "Aus"}
              </button>
            </div>
            <div className="row">
              <label>Virtueller Bildschirm</label>
              <button
                className={config.outputs.virtual_display ? "active" : "ghost"}
                onClick={() =>
                  applyLive({
                    ...config,
                    outputs: {
                      ...config.outputs,
                      virtual_display: !config.outputs.virtual_display,
                    },
                  })
                }
              >
                {config.outputs.virtual_display ? "An" : "Aus"}
              </button>
            </div>
            <div className="row">
              <label>Fenster zeigen</label>
              <button
                className={config.outputs.show_share_window ? "active" : "ghost"}
                onClick={() =>
                  applyLive({
                    ...config,
                    outputs: {
                      ...config.outputs,
                      share_window: true,
                      show_share_window: !config.outputs.show_share_window,
                    },
                  })
                }
              >
                {config.outputs.show_share_window ? "Sichtbar" : "Versteckt"}
              </button>
            </div>
            <div className="stack" style={{ marginTop: "0.45rem" }}>
              <p className="hint" style={{ margin: 0 }}>
                VDD:{" "}
                {snap.vdd.driver_ok || snap.vdd.installed ? (
                  <strong style={{ color: "var(--ok, #6dcc8d)" }}>Treiber bereit</strong>
                ) : (
                  <strong style={{ color: "var(--warn, #e0a35c)" }}>nicht installiert</strong>
                )}
                {snap.vdd.active_index != null
                  ? ` · aktiv #${snap.vdd.active_index}`
                  : ""}
              </p>
              {!snap.vdd.driver_ok ? (
                <button className="ghost" onClick={() => invoke("open_vdd_installer")}>
                  Parsec-VDD Installer öffnen
                </button>
              ) : null}
            </div>
            <p className="hint">
              <strong>Discord Bildschirm:</strong> Virtueller Bildschirm an → Stream starten → in
              Discord Tab <em>Bildschirm</em> den neuen Monitor wählen (Performance-Optionen).
              Live-Preview wird beim Start ausgeschaltet (weniger CPU).
              <br />
              Ohne VDD-Treiber: Discord → <em>Fenster</em> → <em>kynxShare Output</em>.
            </p>
          </div>
        </aside>
      </div>

      {wizard ? (
        <div className="wizard">
          <div className="wizard-card">
            <h2>Willkommen bei kynxShare</h2>
            <p className="hint">
              Monitore werden wie in den Windows-Anzeigeoptionen angeordnet. Layout &amp; Output
              siehst du sofort — Speichern hält alles fest.
            </p>
            <ol>
              <li>Anordnung prüfen / bei Bedarf „Windows-Anordnung“.</li>
              <li>Output-Größe wählen (Native / 1080p / …).</li>
              <li>Parsec-VDD installieren (für Discord-Bildschirm).</li>
              <li>Speichern → Stream starten → Discord: neuer Monitor.</li>
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
