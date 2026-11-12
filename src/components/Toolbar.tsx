import { useStore } from "../store/project";
import { open } from "@tauri-apps/plugin-dialog";
import { formatTime } from "../types";

export function Toolbar() {
  const project = useStore((s) => s.project);
  const tool = useStore((s) => s.tool);
  const setSettingsOpen = useStore((s) => s.setSettingsOpen);
  const selectedClipId = useStore((s) => s.selectedClipId);
  const selectedTextId = useStore((s) => s.selectedTextId);

  const importMedia = async () => {
    const picked = await open({
      multiple: true,
      filters: [
        { name: "Media", extensions: ["mp4", "mov", "mkv", "avi", "webm"] },
      ],
    });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    await tool("asset_import", { paths });
  };

  const cutSelected = async () => {
    if (!selectedClipId) return;
    const clip = project.clips.find((c) => c.id === selectedClipId);
    if (!clip) return;
    // split at playhead if inside, otherwise at midpoint
    const ph = useStore.getState().playhead.ms;
    const at = ph > clip.start_ms && ph < clip.start_ms + (clip.out_ms - clip.in_ms)
      ? ph
      : clip.start_ms + Math.floor((clip.out_ms - clip.in_ms) / 2);
    await tool("clip_cut", { clip_id: selectedClipId, at_ms: at });
  };

  const deleteSelected = async () => {
    if (selectedClipId) await tool("clip_remove", { clip_id: selectedClipId });
    if (selectedTextId) await tool("text_remove", { id: selectedTextId });
  };

  const exportVideo = async () => {
    try {
      const res = (await tool("export_video", {})) as { path: string };
      alert(`Exported: ${res.path}`);
    } catch (e) {
      alert(`Export failed: ${e}`);
    }
  };

  const duration = project.clips.reduce(
    (acc, c) => Math.max(acc, c.start_ms + (c.out_ms - c.in_ms)),
    0,
  );

  return (
    <div className="toolbar">
      <div className="toolbar-title">
        <span className="logo" />
        Media Agent
      </div>
      <button onClick={importMedia} title="Import media files">
        Import
      </button>
      <button onClick={cutSelected} disabled={!selectedClipId} title="Split selected clip (at playhead or midpoint)">
        ✂ Split
      </button>
      <button onClick={deleteSelected} disabled={!selectedClipId && !selectedTextId} title="Delete selection">
        🗑 Delete
      </button>
      <button
        onClick={() => tool("undo")}
        title="Undo last edit (including agent edits)"
      >
        ↶
      </button>
      <button
        onClick={() => tool("redo")}
        title="Redo"
      >
        ↷
      </button>
      <div className="toolbar-spacer" />
      <div className="toolbar-info">
        {project.name} · {project.width}×{project.height} @ {project.fps}fps ·{" "}
        {formatTime(duration)}
      </div>
      <button onClick={exportVideo} className="primary" title="Render mp4 with ffmpeg">
        Export
      </button>
      <button onClick={() => setSettingsOpen(true)} title="Model provider settings">
        ⚙
      </button>
    </div>
  );
}
