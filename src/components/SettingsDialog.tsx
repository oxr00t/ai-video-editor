import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useStore } from "../store/project";

interface Cfg {
  kind: string;
  base_url: string;
  api_key: string;
  model: string;
}

const DEFAULT: Cfg = {
  kind: "openai",
  base_url: "https://api.openai.com/v1",
  api_key: "",
  model: "",
};

const PRESETS: { label: string; kind: string; base_url: string; model: string }[] = [
  { label: "OpenAI", kind: "openai", base_url: "https://api.openai.com/v1", model: "gpt-4o-mini" },
  { label: "Anthropic", kind: "anthropic", base_url: "https://api.anthropic.com", model: "claude-sonnet-4-5" },
  { label: "OpenRouter", kind: "openai", base_url: "https://openrouter.ai/api/v1", model: "anthropic/claude-sonnet-4.5" },
  { label: "9Router", kind: "openai", base_url: "http://localhost:20128/v1", model: "" },
  { label: "Ollama (local)", kind: "openai", base_url: "http://localhost:11434/v1", model: "qwen2.5:7b" },
  { label: "LM Studio (local)", kind: "openai", base_url: "http://localhost:1234/v1", model: "" },
];

export function SettingsDialog() {
  const openState = useStore((s) => s.settingsOpen);
  const setOpen = useStore((s) => s.setSettingsOpen);
  const [cfg, setCfg] = useState<Cfg>(DEFAULT);
  const [saved, setSaved] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [fetchErr, setFetchErr] = useState("");
  const [fetching, setFetching] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    if (openState) {
      invoke<Cfg>("get_provider_config").then(setCfg).catch(console.error);
    }
  }, [openState]);

  if (!openState) return null;

  const applyPreset = (p: (typeof PRESETS)[number]) => {
    setCfg((c) => ({ ...c, kind: p.kind, base_url: p.base_url, model: p.model || c.model }));
    setModels([]);
    setFetchErr("");
  };

  const fetchModels = async () => {
    setFetching(true);
    setFetchErr("");
    try {
      const list = await invoke<string[]>("list_models", {
        kind: cfg.kind,
        baseUrl: cfg.base_url,
        apiKey: cfg.api_key,
      });
      setModels(list);
      if (list.length === 0) setFetchErr("router returned no models");
    } catch (e) {
      setFetchErr(String(e));
    } finally {
      setFetching(false);
    }
  };

  const testConn = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const reply = await invoke<string>("test_provider", { config: cfg });
      setTestResult({ ok: true, text: `model replied: “${reply.slice(0, 80)}”` });
    } catch (e) {
      setTestResult({ ok: false, text: String(e).slice(0, 300) });
    } finally {
      setTesting(false);
    }
  };

  const save = async () => {
    await invoke("set_provider_config", { config: cfg });
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  };

  return (
    <div className="dialog-backdrop" onClick={() => setOpen(false)}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <div className="panel-header">Model settings (BYOM — your key stays local)</div>

        <div className="presets">
          {PRESETS.map((p) => (
            <button key={p.label} className="chip" onClick={() => applyPreset(p)}>
              {p.label}
            </button>
          ))}
        </div>

        <label>
          Type
          <select value={cfg.kind} onChange={(e) => setCfg({ ...cfg, kind: e.target.value })}>
            <option value="openai">OpenAI-compatible (9Router/OpenAI/Ollama/…)</option>
            <option value="anthropic">Anthropic</option>
          </select>
        </label>
        <label>
          Base URL {cfg.kind === "openai" && <i>(include /v1)</i>}
          <input
            value={cfg.base_url}
            onChange={(e) => setCfg({ ...cfg, base_url: e.target.value })}
            spellCheck={false}
          />
        </label>
        <label>
          API key {cfg.base_url.includes("localhost") && <i>(not needed for local)</i>}
          <input
            type="password"
            value={cfg.api_key}
            onChange={(e) => setCfg({ ...cfg, api_key: e.target.value })}
            spellCheck={false}
          />
        </label>
        <label>
          Model
          <div className="model-row">
            <input
              value={cfg.model}
              onChange={(e) => setCfg({ ...cfg, model: e.target.value })}
              placeholder="pick from the list or type e.g. gpt-4o-mini"
              spellCheck={false}
              list="model-options"
            />
            <datalist id="model-options">
              {models.map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
            <button onClick={fetchModels} disabled={fetching || !cfg.base_url.trim()}>
              {fetching ? "…" : "Fetch models"}
            </button>
          </div>
        </label>
        {models.length > 0 && (
          <div className="model-count">{models.length} models available — click the field to pick one</div>
        )}
        {fetchErr && <div className="fetch-error">⚠ {fetchErr}</div>}

        <div className="dialog-actions">
          <button onClick={testConn} disabled={testing || !cfg.model.trim() || !cfg.base_url.trim()}>
            {testing ? "Testing…" : "Test connection"}
          </button>
          <div className="toolbar-spacer" />
          <button onClick={() => setOpen(false)}>Close</button>
          <button className="primary" onClick={save} disabled={!cfg.model.trim()}>
            {saved ? "Saved ✓" : "Save"}
          </button>
        </div>
        {testResult && (
          <div className={testResult.ok ? "model-count" : "fetch-error"}>
            {testResult.ok ? "✓ " : "⚠ "}
            {testResult.text}
          </div>
        )}
        <div className="dialog-note">
          Small local models work too — the editor ships one-call recipes like{" "}
          <code>assemble_sequence</code> designed for them.
        </div>
      </div>
    </div>
  );
}
