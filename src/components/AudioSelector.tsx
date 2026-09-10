import { useTranslation } from "react-i18next";
import { useStore } from "../store";

function VuBar({ level }: { level: number }) {
  const pct = Math.min(100, Math.round(level * 100));
  return (
    <div className="h-2 w-20 overflow-hidden rounded bg-zinc-700">
      <div
        className={`h-full ${pct > 90 ? "bg-red-500" : "bg-emerald-500"}`}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

export function AudioSelector() {
  const { t } = useTranslation();
  const audioMode = useStore((s) => s.audioMode);
  const setAudioMode = useStore((s) => s.setAudioMode);
  const audioDevices = useStore((s) => s.audioDevices);
  const selectedApps = useStore((s) => s.selectedApps);
  const toggleApp = useStore((s) => s.toggleApp);
  const appMix = useStore((s) => s.appMix);
  const setAppMix = useStore((s) => s.setAppMix);
  const mic = useStore((s) => s.mic);
  const setMic = useStore((s) => s.setMic);
  const vu = useStore((s) => s.vu);
  const isLive = useStore((s) => s.isLive);

  return (
    <section className="space-y-2">
      <h2 className="text-sm font-semibold text-zinc-400">{t("audio.tab")}</h2>

      {/* F-ST-03: master VU over the mixed output */}
      <div className="flex items-center gap-2 text-sm">
        <span className="w-20 shrink-0 text-zinc-500">{t("audio.master")}</span>
        <VuBar level={vu.master.rms} />
      </div>

      <div className="flex gap-4 text-sm">
        <label className="flex items-center gap-1">
          <input
            type="radio"
            checked={audioMode === "system"}
            onChange={() => setAudioMode("system")}
          />
          {t("audio.system")}
        </label>
        <label className="flex items-center gap-1">
          <input type="radio" checked={audioMode === "apps"} onChange={() => setAudioMode("apps")} />
          {t("audio.apps")}
        </label>
      </div>

      {audioMode === "apps" && (
        <div className="space-y-1 text-sm">
          {(audioDevices?.apps ?? []).map((a) => {
            const selected = selectedApps.includes(a.id);
            const mix = appMix[a.id] ?? { gain: 1, muted: false };
            return (
              <div key={a.id} className="flex items-center gap-2">
                <label className="flex items-center gap-1">
                  <input
                    type="checkbox"
                    checked={selected}
                    onChange={() => toggleApp(a.id)}
                  />
                  {a.label}
                </label>
                {/* F-AU-04: per-source VU / mute / gain (session-only) */}
                {selected && (
                  <>
                    <VuBar level={vu.apps[a.id]?.rms ?? 0} />
                    <label className="flex items-center gap-1 text-xs text-zinc-400">
                      <input
                        type="checkbox"
                        checked={mix.muted}
                        onChange={(e) => setAppMix(a.id, { muted: e.target.checked })}
                      />
                      {t("audio.mute")}
                    </label>
                    <input
                      type="range"
                      min={0}
                      max={2}
                      step={0.05}
                      value={mix.gain}
                      onChange={(e) => setAppMix(a.id, { gain: Number(e.target.value) })}
                      title={t("audio.gain")}
                      className="w-24"
                    />
                  </>
                )}
              </div>
            );
          })}
          {audioDevices === null && (
            <p className="text-xs text-zinc-500">{t("screen.notAvailable")}</p>
          )}
          {audioDevices !== null && (audioDevices.apps ?? []).length === 0 && (
            <p className="text-xs text-zinc-500">{t("audio.noApps")}</p>
          )}
        </div>
      )}

      <div className="flex items-center gap-3 text-sm">
        <label className="flex items-center gap-1">
          <input
            type="checkbox"
            checked={mic.enabled}
            onChange={(e) => setMic({ enabled: e.target.checked })}
          />
          {t("audio.mic")}
        </label>
        <select
          className="rounded border border-zinc-700 bg-zinc-800 px-2 py-1"
          value={mic.device}
          onChange={(e) => setMic({ device: e.target.value })}
        >
          <option value="default">default</option>
          {(audioDevices?.inputs ?? []).map((d) => (
            <option key={d.id} value={d.id}>
              {d.label}
            </option>
          ))}
        </select>
        <label className="flex items-center gap-1">
          <input
            type="checkbox"
            checked={mic.muted}
            onChange={(e) => setMic({ muted: e.target.checked })}
          />
          {t("audio.mute")}
        </label>
        <input
          type="range"
          min={0}
          max={2}
          step={0.05}
          value={mic.gain}
          onChange={(e) => setMic({ gain: Number(e.target.value) })}
          title={t("audio.gain")}
          className="w-24"
        />
        {isLive && mic.enabled && <VuBar level={vu.mic?.rms ?? 0} />}
      </div>
      {isLive && (
        <p className="text-xs text-zinc-500">{t("audio.liveHint")}</p>
      )}
    </section>
  );
}
