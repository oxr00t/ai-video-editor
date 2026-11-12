import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useStore } from "../store/project";
import type { AgentEvent } from "../types";

export function Chat() {
  const events = useStore((s) => s.agentEvents);
  const busy = useStore((s) => s.agentBusy);
  const statusText = useStore((s) => s.agentStatusText);
  const clearEvents = useStore((s) => s.clearAgentEvents);
  const [input, setInput] = useState("");
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    listRef.current?.scrollTo({ top: listRef.current.scrollHeight });
  }, [events]);

  const send = async () => {
    const msg = input.trim();
    if (!msg || busy) return;
    setInput("");
    // echo the user's own message into the transcript
    useStore.getState().pushAgentEvent({ type: "user", text: msg });
    try {
      await invoke("agent_send", { message: msg });
    } catch (e) {
      useStore.getState().pushAgentEvent({ type: "error", text: String(e) });
    }
  };

  return (
    <div className="chat">
      <div className="panel-header">
        Agent
        <button className="mini" onClick={clearEvents} title="Clear conversation">
          clear
        </button>
      </div>
      <div className="chat-list" ref={listRef}>
        {events.length === 0 && (
          <div className="empty-hint">
            Talk to the editing agent. It runs on <b>your</b> model key
            (configure in ⚙).
            <br />
            <br />
            Try: “import these two clips and make a 10s sequence with a title
            at the start”.
          </div>
        )}
        {events.map((e, i) => (
          <ChatEvent key={i} e={e} />
        ))}
        {busy && <div className="chat-status">● {statusText || "thinking"}</div>}
      </div>
      <div className="chat-input-row">
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
          placeholder={busy ? "agent is working…" : "Ask the agent to edit…"}
          disabled={busy}
        />
        <button onClick={send} disabled={busy || !input.trim()} className="primary">
          Send
        </button>
      </div>
    </div>
  );
}

function ChatEvent({ e }: { e: AgentEvent }) {
  if (e.type === "user") {
    return <div className="chat-msg user">{e.text}</div>;
  }
  if (e.type === "message") {
    return <div className="chat-msg assistant">{e.text}</div>;
  }
  if (e.type === "error") {
    return <div className="chat-msg error">⚠ {e.text}</div>;
  }
  if (e.type === "tool") {
    return (
      <div className="chat-tool">
        <span className="tool-icon">🔧</span> {e.name}
        {e.args != null && Object.keys(e.args as object).length > 0 && (
          <div className="chat-tool-args">{compact(e.args)}</div>
        )}
      </div>
    );
  }
  if (e.type === "tool_result") {
    const detail = resultDetail(e.result);
    return (
      <div className={`chat-tool-result ${e.ok ? "ok" : "fail"}`}>
        {e.ok ? "✓" : "✗"} {e.name}
        {detail && <div className="chat-tool-detail">{detail}</div>}
      </div>
    );
  }
  return null;
}

/** one compact line for tool arguments */
function compact(v: unknown): string {
  const parts: string[] = [];
  const obj = v as Record<string, unknown>;
  for (const [k, val] of Object.entries(obj)) {
    let s: string;
    if (val === null || val === undefined) s = "null";
    else if (typeof val === "object") s = JSON.stringify(val);
    else s = String(val);
    if (s.length > 60) s = s.slice(0, 57) + "…";
    parts.push(`${k}=${s}`);
  }
  let out = parts.join("  ");
  if (out.length > 200) out = out.slice(0, 197) + "…";
  return out;
}

/** short human-readable summary of a tool result */
function resultDetail(r: unknown): string | null {
  if (!r || typeof r !== "object") return null;
  const o = r as Record<string, unknown>;
  if (typeof o.error === "string") return o.error.slice(0, 240);
  const bits: string[] = [];
  if (typeof o.project_duration_ms === "number")
    bits.push(`duration ${(o.project_duration_ms / 1000).toFixed(1)}s`);
  if (typeof o.removed_ms === "number") bits.push(`removed ${(o.removed_ms / 1000).toFixed(1)}s`);
  if (Array.isArray(o.assets)) bits.push(`${o.assets.length} assets`);
  if (Array.isArray(o.frames)) bits.push(`${o.frames.length} frames`);
  if (Array.isArray(o.lines)) bits.push(`${o.lines.length} lines`);
  if (Array.isArray(o.clip_ids)) bits.push(`${o.clip_ids.length} clips`);
  if (typeof o.word_count === "number") bits.push(`${o.word_count} words`);
  if (typeof o.path === "string") bits.push(o.path.split(/[\\/]/).pop() ?? "");
  if (!bits.length) return null;
  return bits.join(" · ").slice(0, 200);
}
