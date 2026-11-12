import { useStore } from "../store/project";
import type { Asset } from "../types";
import { formatTime } from "../types";

export function AssetBin() {
  const assets = useStore((s) => s.project.assets);
  const tool = useStore((s) => s.tool);

  const addToTimeline = async (asset: Asset) => {
    await tool("clip_add", { asset_id: asset.id });
  };

  return (
    <div className="asset-bin">
      <div className="panel-header">Assets</div>
      {assets.length === 0 && (
        <div className="empty-hint">
          Import media with the toolbar, or ask the agent on the right:
          <br />
          <i>“import D:\clips\a.mp4 and b.mp4 and build a sequence”</i>
        </div>
      )}
      <div className="asset-list">
        {assets.map((a) => (
          <div key={a.id} className="asset-item" title={a.path}>
            <div className="asset-thumb">
              <div className="asset-glyph">▶</div>
            </div>
            <div className="asset-meta">
              <div className="asset-name">{a.name}</div>
              <div className="asset-sub">
                {formatTime(a.duration_ms)} · {a.width}×{a.height}
                {a.proxy_path ? " · ✓" : " · proxy…"}
              </div>
            </div>
            <button
              className="asset-add"
              onClick={() => addToTimeline(a)}
              title="Add to timeline (end of V1)"
            >
              +
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
