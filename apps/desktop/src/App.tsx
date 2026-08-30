import { useEffect, useMemo, useState } from "react";
import { applyConfig, snapshot as loadSnapshot, subscribe } from "./api";
import type {
  Config, EndpointInfo, EngineSnapshot, MessageType, MidiEffect, Route, TransformRule, VirtualDevice
} from "./types";

type Page = "dashboard" | "devices" | "virtual" | "routes" | "settings";
const pages: Array<[Page, string, string]> = [
  ["dashboard", "Übersicht", "⌁"], ["devices", "MIDI-Geräte", "⌨"],
  ["virtual", "Virtuelle Ports", "◫"], ["routes", "Patchbay", "↝"],
  ["settings", "Einstellungen", "⚙"]
];

const filterNames: Record<MessageType, string> = {
  pitch_bend: "Pitch Bend", poly_aftertouch: "Poly Aftertouch",
  channel_aftertouch: "Channel Aftertouch", midi_clock: "MIDI Clock",
  system_realtime: "System Realtime", sys_ex: "SysEx"
};

function cloneConfig(config: Config): Config {
  return structuredClone(config);
}

export default function App() {
  const [page, setPage] = useState<Page>("dashboard");
  const [state, setState] = useState<EngineSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = async (quiet = false) => {
    try {
      const next = await loadSnapshot();
      setState(next);
      if (!quiet) setError(null);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    }
  };

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        let current = await loadSnapshot();
        if (!active) return;
        setState(current);
        while (active) {
          const next = await subscribe(current.revision);
          if (active && next.revision !== current.revision) setState(next);
          current = next;
        }
      } catch (caught) {
        if (active) setError(caught instanceof Error ? caught.message : String(caught));
      }
    })();
    return () => { active = false; };
  }, []);

  const update = async (mutate: (config: Config) => void) => {
    if (!state) return;
    const config = cloneConfig(state.config);
    mutate(config);
    setBusy(true);
    try {
      setState(await applyConfig(config));
      setError(null);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally { setBusy(false); }
  };

  return <div className="shell">
    <aside>
      <div className="brand"><div className="logo">VM</div><div><strong>Virtual MIDI</strong><span>MASTER</span></div></div>
      <nav>{pages.map(([id, label, icon]) => <button className={page === id ? "active" : ""} onClick={() => setPage(id)} key={id}><i>{icon}</i>{label}</button>)}</nav>
      <div className="engine-state"><span className={state?.running ? "dot online" : "dot"}/><div><strong>{state?.running ? "Engine aktiv" : "Engine offline"}</strong><small>{state?.platform.platform ?? "Verbindung wird hergestellt"}</small></div></div>
    </aside>
    <main>
      <header><div><h1>{pages.find(([id]) => id === page)?.[1]}</h1><p>MIDI-Routing ohne Umwege.</p></div><span className="pill">MIDI 1.0</span></header>
      {error && <div className="alert"><strong>Fehler</strong><span>{error}</span><button onClick={() => setError(null)}>×</button></div>}
      {!state ? <div className="empty">Engine wird gestartet …</div> : <>
        {state.last_error && <div className="alert"><strong>MIDI-Fehler</strong><span>{state.last_error}</span></div>}
        {page === "dashboard" && <Dashboard state={state}/>}
        {page === "devices" && <Devices state={state}/>}
        {page === "virtual" && <VirtualDevices state={state} update={update}/>}
        {page === "routes" && <Routes state={state} update={update}/>}
        {page === "settings" && <Settings state={state} update={update}/>}
      </>}
      {busy && <div className="saving">Konfiguration wird angewendet …</div>}
    </main>
  </div>;
}

function Dashboard({ state }: { state: EngineSnapshot }) {
  return <div className="stack">
    <section className="stats">
      <Stat label="Online-Endpunkte" value={state.endpoints.filter(e => e.online).length}/>
      <Stat label="Empfangen" value={state.stats.received}/>
      <Stat label="Gesendet" value={state.stats.sent}/>
      <Stat label="Routing-Latenz" value={`${state.stats.last_internal_latency_micros} µs`}/>
    </section>
    <section className="panel"><PanelTitle title="Engine und Runtime" subtitle="Aktueller Zustand der Hintergrundkomponente"/>
      <dl className="details"><dt>Plattform</dt><dd>{state.platform.platform}</dd><dt>Runtime</dt><dd className={state.platform.runtime_available ? "good" : "bad"}>{state.platform.runtime_available ? "Verfügbar" : "Fehlt"}</dd><dt>Details</dt><dd>{state.platform.detail}</dd><dt>Letzter Fehler</dt><dd className={state.last_error ? "bad" : "good"}>{state.last_error ?? "Keiner"}</dd><dt>Sendefehler</dt><dd>{state.stats.send_errors}</dd></dl>
    </section>
    <Monitor state={state}/>
  </div>;
}

function Devices({ state }: { state: EngineSnapshot }) {
  return <div className="stack"><section className="panel"><PanelTitle title="Erkannte Endpunkte" subtitle="Quellen liefern Daten; Ziele empfangen Daten"/>
    <div className="endpoint-list">{state.endpoints.map(endpoint => <div className="endpoint" key={endpoint.id}><span className={`dot ${endpoint.online ? "online" : ""}`}/><div><strong>{endpoint.name}</strong><small>{endpoint.id}</small></div><span className="tag">{endpoint.kind}</span><span className="tag accent">{endpoint.direction === "source" ? "Quelle" : "Ziel"}</span></div>)}</div>
  </section><Monitor state={state}/></div>;
}

function VirtualDevices({ state, update }: ViewProps) {
  const [name, setName] = useState("");
  const add = () => {
    const trimmed = name.trim(); if (!trimmed) return;
    void update(config => config.virtual_devices.push({ id: crypto.randomUUID(), name: trimmed, enabled: true, effects: [] }));
    setName("");
  };
  return <section className="panel"><PanelTitle title="Virtuelle MIDI-Geräte" subtitle="DAW-Ausgang → MIDI In · geladene Effekte → Routing · MIDI Out → DAW-Eingang"/>
    <div className="add-row"><input value={name} onChange={event => setName(event.target.value)} onKeyDown={event => event.key === "Enter" && add()} placeholder="Name des neuen Portpaars"/><button className="primary" onClick={add}>Portpaar hinzufügen</button></div>
    <div className="cards">{state.config.virtual_devices.map(device => <div className="device-block" key={device.id}><div className="device-card"><div className="device-icon">◫</div><div className="grow"><input className="name-input" defaultValue={device.name} onBlur={event => { const next = event.target.value.trim(); if (next && next !== device.name) void update(config => { config.virtual_devices.find(item => item.id === device.id)!.name = next; }); }}/><small><code>{device.name} MIDI In</code> · <code>{device.name} MIDI Out</code></small></div><label className="switch"><input type="checkbox" checked={device.enabled} onChange={event => void update(config => { config.virtual_devices.find(item => item.id === device.id)!.enabled = event.target.checked; })}/><span/></label><button className="danger-text" onClick={() => void update(config => { config.virtual_devices = config.virtual_devices.filter(item => item.id !== device.id); config.routes = config.routes.filter(route => !route.source.includes(device.id) && !route.target.includes(device.id)); config.known_endpoints = config.known_endpoints.filter(endpoint => !endpoint.id.includes(device.id)); })}>Entfernen</button></div><DeviceEffects device={device} update={update}/></div>)}</div>
  </section>;
}

function DeviceEffects({ device, update }: { device: VirtualDevice; update: ViewProps["update"] }) {
  const change = (index: number, effect: MidiEffect) => void update(config => { config.virtual_devices.find(item => item.id === device.id)!.effects[index] = effect; });
  const add = (type: "scale" | "sustain") => void update(config => config.virtual_devices.find(item => item.id === device.id)!.effects.push(type === "scale"
    ? { type: "scale_helper", enabled: true, root: 0, scale: "major" }
    : { type: "sustain_helper", enabled: true, mode: "both", min_sustain_notes: 2, release_time_ms: 45, dissonance: "both", auto_sustain: true }));
  return <div className="effects"><div className="effects-title"><span>MIDI-Effektkette</span><div><button onClick={() => add("scale")}>+ Scale Helper</button><button onClick={() => add("sustain")}>+ Sustain Helper</button></div></div>
    {device.effects.map((effect, index) => <div className="effect" key={index}><span className="handle">{index + 1}</span><label className="switch"><input type="checkbox" checked={effect.enabled} onChange={event => change(index, { ...effect, enabled: event.target.checked })}/><span/></label><EffectControls effect={effect} onChange={next => change(index, next)}/><button disabled={index === 0} onClick={() => void update(config => { const effects = config.virtual_devices.find(item => item.id === device.id)!.effects; [effects[index - 1], effects[index]] = [effects[index], effects[index - 1]]; })}>↑</button><button disabled={index === device.effects.length - 1} onClick={() => void update(config => { const effects = config.virtual_devices.find(item => item.id === device.id)!.effects; [effects[index + 1], effects[index]] = [effects[index], effects[index + 1]]; })}>↓</button><button onClick={() => void update(config => { config.virtual_devices.find(item => item.id === device.id)!.effects.splice(index, 1); })}>×</button></div>)}
    {device.effects.length === 0 && <small className="no-effects">Keine Effekte geladen. Eingehende MIDI-Daten bleiben unverändert.</small>}
  </div>;
}

function EffectControls({ effect, onChange }: { effect: MidiEffect; onChange: (effect: MidiEffect) => void }) {
  if (effect.type === "scale_helper") return <div className="effect-controls"><b>Scale Helper</b><label>Grundton <select value={effect.root} onChange={event => onChange({ ...effect, root: Number(event.target.value) })}>{["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"].map((name, index) => <option value={index} key={name}>{name}</option>)}</select></label><label>Skala <select value={effect.scale} onChange={event => onChange({ ...effect, scale: event.target.value as typeof effect.scale })}><option value="major">Dur</option><option value="natural_minor">Natürlich Moll</option><option value="harmonic_minor">Harmonisch Moll</option><option value="melodic_minor">Melodisch Moll</option></select></label></div>;
  return <div className="effect-controls"><b>Sustain Helper</b><label>Modus <select value={effect.mode} onChange={event => onChange({ ...effect, mode: event.target.value as typeof effect.mode })}><option value="chord_gesture">Akkordgeste</option><option value="dissonance">Dissonanz</option><option value="both">Beides</option></select></label><label>Pool <input type="number" min="1" max="6" value={effect.min_sustain_notes} onChange={event => onChange({ ...effect, min_sustain_notes: Math.max(1, Math.min(6, Number(event.target.value))) })}/></label><label>Pause ms <input type="number" min="5" max="250" value={effect.release_time_ms} onChange={event => onChange({ ...effect, release_time_ms: Math.max(5, Math.min(250, Number(event.target.value))) })}/></label><label>Dissonanz <select value={effect.dissonance} onChange={event => onChange({ ...effect, dissonance: event.target.value as typeof effect.dissonance })}><option value="off">Aus</option><option value="semitone">Halbton</option><option value="tritone">Tritonus</option><option value="both">Beides</option></select></label><label className="check"><input type="checkbox" checked={effect.auto_sustain} onChange={event => onChange({ ...effect, auto_sustain: event.target.checked })}/> Auto-Sustain</label></div>;
}

interface ViewProps { state: EngineSnapshot; update: (mutate: (config: Config) => void) => Promise<void> }

function Routes({ state, update }: ViewProps) {
  const sources = state.endpoints.filter(e => e.direction === "source");
  const targets = state.endpoints.filter(e => e.direction === "target");
  const [source, setSource] = useState(""); const [target, setTarget] = useState("");
  useEffect(() => { if (!source && sources[0]) setSource(sources[0].id); if (!target && targets[0]) setTarget(targets[0].id); }, [state.endpoints]);
  const add = () => source && target && void update(config => config.routes.push({ id: crypto.randomUUID(), source, target, enabled: true, rules: [] }));
  return <div className="stack"><section className="panel"><PanelTitle title="Neue Route" subtitle="Keyboard/Quelle → MIDI Out → DAW · DAW → MIDI In → Zielgerät"/><div className="route-builder"><EndpointSelect endpoints={sources} value={source} onChange={setSource}/><span className="arrow">→</span><EndpointSelect endpoints={targets} value={target} onChange={setTarget}/><button className="primary" onClick={add} disabled={!source || !target}>Verbinden</button></div></section>
    {state.config.routes.map(route => <RouteEditor key={route.id} route={route} endpoints={state.endpoints} update={update}/>) }
    {state.config.routes.length === 0 && <div className="empty">Noch keine Route konfiguriert.</div>}
  </div>;
}

function RouteEditor({ route, endpoints, update }: { route: Route; endpoints: EndpointInfo[]; update: ViewProps["update"] }) {
  const addRule = (rule: TransformRule) => void update(config => config.routes.find(item => item.id === route.id)!.rules.push(rule));
  return <section className={`panel route ${route.enabled ? "" : "disabled"}`}><div className="route-head"><div><select value={route.source} onChange={event => void update(config => { config.routes.find(item => item.id === route.id)!.source = event.target.value; })}>{endpoints.filter(endpoint => endpoint.direction === "source").map(endpoint => <option value={endpoint.id} key={endpoint.id}>{endpoint.name}</option>)}</select><span>→</span><select value={route.target} onChange={event => void update(config => { config.routes.find(item => item.id === route.id)!.target = event.target.value; })}>{endpoints.filter(endpoint => endpoint.direction === "target").map(endpoint => <option value={endpoint.id} key={endpoint.id}>{endpoint.name}</option>)}</select></div><div><label className="switch"><input type="checkbox" checked={route.enabled} onChange={event => void update(config => { config.routes.find(item => item.id === route.id)!.enabled = event.target.checked; })}/><span/></label><button className="danger-text" onClick={() => void update(config => { config.routes = config.routes.filter(item => item.id !== route.id); })}>Route löschen</button></div></div>
    <div className="rules">{route.rules.map((rule, index) => <div className="rule" key={index}><span className="handle">{index + 1}</span><RuleControls rule={rule} onChange={next => void update(config => { config.routes.find(item => item.id === route.id)!.rules[index] = next; })}/><button disabled={index === 0} title="Nach oben" onClick={() => void update(config => { const rules = config.routes.find(item => item.id === route.id)!.rules; [rules[index - 1], rules[index]] = [rules[index], rules[index - 1]]; })}>↑</button><button disabled={index === route.rules.length - 1} title="Nach unten" onClick={() => void update(config => { const rules = config.routes.find(item => item.id === route.id)!.rules; [rules[index + 1], rules[index]] = [rules[index], rules[index + 1]]; })}>↓</button><button title="Entfernen" onClick={() => void update(config => { config.routes.find(item => item.id === route.id)!.rules.splice(index, 1); })}>×</button></div>)}</div>
    <div className="rule-buttons"><select onChange={event => { const value = event.target.value as MessageType; if (value) { addRule({ type: "drop_message_type", message_type: value }); event.target.value = ""; } }} defaultValue=""><option value="" disabled>+ Nachricht filtern</option>{Object.entries(filterNames).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select><button onClick={() => addRule({ type: "map_channel", from: null, to: 0 })}>+ Kanal → 1</button><button onClick={() => addRule({ type: "map_control_change", from: 1, to: 74, channel: null })}>+ CC 1 → 74</button></div>
  </section>;
}

function RuleControls({ rule, onChange }: { rule: TransformRule; onChange: (rule: TransformRule) => void }) {
  if (rule.type === "drop_message_type") return <span><b>Filtern</b><select value={rule.message_type} onChange={event => onChange({ ...rule, message_type: event.target.value as MessageType })}>{Object.entries(filterNames).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></span>;
  if (rule.type === "map_channel") return <span><b>Kanal-Mapping</b><select value={rule.from ?? "all"} onChange={event => onChange({ ...rule, from: event.target.value === "all" ? null : Number(event.target.value) })}><option value="all">Alle</option>{Array.from({length:16},(_, index) => <option value={index} key={index}>{index + 1}</option>)}</select><em>→</em><select value={rule.to} onChange={event => onChange({ ...rule, to: Number(event.target.value) })}>{Array.from({length:16},(_, index) => <option value={index} key={index}>{index + 1}</option>)}</select></span>;
  return <span><b>CC-Mapping</b><label>CC <input type="number" min="0" max="127" value={rule.from} onChange={event => onChange({ ...rule, from: Math.max(0, Math.min(127, Number(event.target.value))) })}/></label><em>→</em><label>CC <input type="number" min="0" max="127" value={rule.to} onChange={event => onChange({ ...rule, to: Math.max(0, Math.min(127, Number(event.target.value))) })}/></label><select value={rule.channel ?? "all"} onChange={event => onChange({ ...rule, channel: event.target.value === "all" ? null : Number(event.target.value) })}><option value="all">Alle Kanäle</option>{Array.from({length:16},(_, index) => <option value={index} key={index}>Kanal {index + 1}</option>)}</select></span>;
}

function Settings({ state, update }: ViewProps) {
  return <div className="stack"><section className="panel"><PanelTitle title="Hintergrundbetrieb" subtitle="Die Engine hält virtuelle Ports auch ohne geöffnetes Fenster bereit"/><div className="setting"><div><strong>Nach Anmeldung starten</strong><small>Registriert die Engine im Benutzerkontext, ohne Administratorrechte.</small></div><label className="switch"><input type="checkbox" checked={state.config.autostart} onChange={event => void update(config => { config.autostart = event.target.checked; })}/><span/></label></div></section>
    <section className="panel"><PanelTitle title="Engine-Log" subtitle="Die letzten Status- und Fehlermeldungen"/><div className="log-list">{state.logs.slice().reverse().map((entry, index) => <code key={index}>{entry}</code>)}</div></section>
    <section className="panel"><PanelTitle title="Diagnose" subtitle="Informationen für Fehlersuche und Support"/><pre>{JSON.stringify({ platform: state.platform, lastError: state.last_error, stats: state.stats, endpoints: state.endpoints.length, configVersion: state.config.version }, null, 2)}</pre></section>
  </div>;
}

function EndpointSelect({ endpoints, value, onChange }: { endpoints: EndpointInfo[]; value: string; onChange: (value: string) => void }) {
  return <select value={value} onChange={event => onChange(event.target.value)}><option value="" disabled>Endpoint wählen</option>{endpoints.map(endpoint => <option key={endpoint.id} value={endpoint.id}>{endpoint.name} · {endpoint.kind}</option>)}</select>;
}
function Stat({ label, value }: { label: string; value: string | number }) { return <div className="stat"><span>{label}</span><strong>{value}</strong></div>; }
function PanelTitle({ title, subtitle }: { title: string; subtitle: string }) { return <div className="panel-title"><div><h2>{title}</h2><p>{subtitle}</p></div></div>; }
function Monitor({ state }: { state: EngineSnapshot }) { const recent = useMemo(() => state.monitor.slice(-12).reverse(), [state.monitor]); return <section className="panel"><PanelTitle title="Live-Monitor" subtitle="Die letzten gerouteten Nachrichten; große Nachrichten werden als Vorschau angezeigt"/><div className="monitor"><div className="monitor-row heading"><span>Zeit</span><span>Quelle</span><span>Daten</span><span>Ziele</span></div>{recent.map((item, index) => <div className="monitor-row" key={`${item.event.timestamp_micros}-${index}`}><span>{item.event.timestamp_micros}</span><span title={item.event.source}>{item.event.source.split(":").slice(-1)}</span><code>{item.event.data.map(byte => byte.toString(16).padStart(2, "0").toUpperCase()).join(" ")}{item.data_len > item.event.data.length ? ` … (${item.data_len} Bytes)` : ""}</code><span>{item.targets}</span></div>)}{recent.length === 0 && <div className="empty compact">Noch keine MIDI-Nachrichten empfangen.</div>}</div></section>; }
