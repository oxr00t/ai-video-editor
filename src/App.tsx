import { useEffect } from "react";
import { useStore } from "./store/project";
import { Toolbar } from "./components/Toolbar";
import { AssetBin } from "./components/AssetBin";
import { Preview } from "./components/Preview";
import { Timeline } from "./components/Timeline";
import { Chat } from "./components/Chat";
import { SettingsDialog } from "./components/SettingsDialog";

export default function App() {
  const init = useStore((s) => s.init);

  useEffect(() => {
    // with StrictMode double-mount: subscribe → cleanup → subscribe,
    // so exactly one live listener at any time
    let disposed = false;
    let dispose: (() => void) | undefined;
    init().then((d) => {
      if (disposed) d();
      else dispose = d;
    });
    return () => {
      disposed = true;
      dispose?.();
    };
  }, [init]);

  return (
    <div className="app">
      <Toolbar />
      <div className="app-main">
        <AssetBin />
        <div className="app-center">
          <Preview />
          <Timeline />
        </div>
        <Chat />
      </div>
      <SettingsDialog />
    </div>
  );
}
