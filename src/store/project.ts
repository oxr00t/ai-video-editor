import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Project, AgentEvent } from "../types";

interface Playhead {
  ms: number;
}

interface Store {
  project: Project;
  selectedClipId: string | null;
  selectedTextId: string | null;
  playhead: Playhead;
  agentEvents: AgentEvent[];
  agentBusy: boolean;
  agentStatusText: string;
  playing: boolean;
  settingsOpen: boolean;
  // actions
  init: () => Promise<() => void>;
  setProject: (p: Project) => void;
  select: (clipId: string | null, textId: string | null) => void;
  setPlayhead: (ms: number) => void;
  setPlaying: (v: boolean) => void;
  setSettingsOpen: (v: boolean) => void;
  pushAgentEvent: (e: AgentEvent) => void;
  clearAgentEvents: () => void;
  tool: (name: string, args?: object) => Promise<unknown>;
}

const emptyProject: Project = {
  id: "",
  name: "Untitled",
  fps: 30,
  width: 1920,
  height: 1080,
  assets: [],
  tracks: [],
  clips: [],
  texts: [],
};

export const useStore = create<Store>((set, get) => ({
  project: emptyProject,
  selectedClipId: null,
  selectedTextId: null,
  playhead: { ms: 0 },
  agentEvents: [],
  agentBusy: false,
  agentStatusText: "",
  playing: false,
  settingsOpen: false,

  init: async () => {
    // NOTE: the returned cleanup unsubscribes both listeners — React
    // StrictMode double-mounts effects in dev, and without cleanup every
    // event would arrive twice (duplicate chat messages).
    const un1 = await listen<Project>("project_changed", (e) => {
      get().setProject(e.payload);
    });
    const un2 = await listen<AgentEvent>("agent_event", (e) => {
      const ev = e.payload;
      get().pushAgentEvent(ev);
      if (ev.type === "status") {
        const t = ev.text ?? "";
        const busyText = t === "thinking" || t.startsWith("running");
        set({ agentBusy: busyText, agentStatusText: busyText ? t : "" });
      } else if (ev.type === "message" || ev.type === "error") {
        set({ agentBusy: false, agentStatusText: "" });
      }
    });
    // initial project state (full EDL via internal command; the model-facing
    // project_get tool returns a compact form instead)
    try {
      const p = await invoke<Project>("get_project");
      set({ project: p });
    } catch (err) {
      console.error("get_project failed", err);
    }
    return () => {
      un1();
      un2();
    };
  },

  setProject: (p) => set({ project: p }),

  select: (clipId, textId) =>
    set({ selectedClipId: clipId, selectedTextId: textId }),

  setPlayhead: (ms) => set({ playhead: { ms } }),
  setPlaying: (v) => set({ playing: v }),
  setSettingsOpen: (v) => set({ settingsOpen: v }),

  pushAgentEvent: (e) =>
    set((st) => ({ agentEvents: [...st.agentEvents, e] })),
  clearAgentEvents: () => set({ agentEvents: [] }),

  tool: async (name, args = {}) => {
    return invoke("tool_invoke", { name, args });
  },
}));
