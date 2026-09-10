import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useStore } from "../store";
import { api } from "../lib/api";
import { MAX_AUDIO_KBPS, MAX_VIDEO_KBPS, type ProfilesConfig } from "../lib/types";

export function SettingsModal({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const profiles = useStore((s) => s.profiles);
  const setProfiles = useStore((s) => s.setProfiles);
  const showToast = useStore((s) => s.showToast);
  const [dirty, setDirty] = useState<ProfilesConfig | null>(null);
  const cfg = dirty ?? profiles;

  const update = (fn: (draft: ProfilesConfig) => void) => {
    if (!cfg) return;
    const next = structuredClone(cfg);
    fn(next);
    setDirty(next);
  };

  const save = async () => {
    if (!dirty) return;
    try {
      await api.saveProfiles(dirty);
      setProfiles(dirty);
      setDirty(null);
      showToast(t("app.saved"));
    } catch (e) {
      showToast(String(e));
    }
  };

  const addProfile = () => {
    update((d) => {
      let n = 1;
      while (d.profiles[`custom${n}`]) n++;
      d.profiles[`custom${n}`] = {
        name: `Custom ${n}`,
        w: 1280,
        h: 720,
        fps: 30,
        v_kbps: 1500,
        a_kbps: 192,
        encoder: "auto",
      };
    });
  };

  // F-CF-03: export/import as JSON. WebView download/file-input behavior is
  // verified on Windows (WebView2) and Linux (WebKitGTK) during E2E.
  const exportJson = () => {
    if (!cfg) return;
    const blob = new Blob([JSON.stringify(cfg, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "ezstreamer-profiles.json";
    a.click();
    URL.revokeObjectURL(url);
  };

  const importJson = async (file: File) => {
    try {
      const parsed = JSON.parse(await file.text()) as ProfilesConfig;
      if (!parsed || typeof parsed !== "object" || !parsed.profiles) {
        throw new Error("profiles field missing");
      }
      setDirty({ ...parsed, version: parsed.version ?? 2 });
      showToast(t("settings.importLoaded"));
    } catch (e) {
      showToast(`${t("settings.importError")}: ${String(e)}`);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="max-h-[85vh] w-[640px] overflow-auto rounded-lg border border-zinc-700 bg-zinc-900 p-4">
        <div className="mb-3 flex items-center">
          <h2 className="font-bold">{t("settings.title")}</h2>
          <button className="ml-auto text-zinc-400 hover:text-white" onClick={onClose}>
            ✕
          </button>
        </div>

        <div className="mb-1 flex items-center gap-2">
          <h3 className="text-sm font-semibold text-zinc-400">{t("settings.profiles")}</h3>
          <button
            className="rounded border border-zinc-600 px-2 py-0.5 text-xs hover:border-zinc-400"
            onClick={addProfile}
          >
            + {t("settings.add")}
          </button>
          <button
            className="ml-auto rounded border border-zinc-600 px-2 py-0.5 text-xs hover:border-zinc-400"
            onClick={exportJson}
          >
            {t("settings.export")}
          </button>
          <label className="cursor-pointer rounded border border-zinc-600 px-2 py-0.5 text-xs hover:border-zinc-400">
            {t("settings.import")}
            <input
              type="file"
              accept="application/json,.json"
              className="hidden"
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) void importJson(f);
                e.target.value = "";
              }}
            />
          </label>
        </div>
        <table className="w-full text-sm">
          <thead className="text-left text-xs text-zinc-500">
            <tr>
              <th className="pr-2">{t("settings.name")}</th>
              <th className="pr-2">{t("settings.resolution")}</th>
              <th className="pr-2">{t("settings.fps")}</th>
              <th className="pr-2">{t("settings.vKbps")}</th>
              <th className="pr-2">{t("settings.aKbps")}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {Object.entries(cfg?.profiles ?? {}).map(([id, p]) => (
              <tr key={id} className="border-t border-zinc-800">
                <td className="py-1 pr-2">
                  {BUILTIN_IDS.includes(id) ? (
                    p.name.startsWith("profile.") ? (
                      t(p.name)
                    ) : (
                      p.name
                    )
                  ) : (
                    <input
                      className="w-28 rounded border border-zinc-700 bg-zinc-800 px-1 py-0.5"
                      value={p.name}
                      onChange={(e) => update((d) => void (d.profiles[id].name = e.target.value))}
                    />
                  )}
                </td>
                <td className="pr-2">
                  <span className="flex items-center gap-1">
                    <NumberCell
                      value={p.w}
                      max={7680}
                      className="w-16"
                      onChange={(v) => update((d) => void (d.profiles[id].w = v))}
                    />
                    ×
                    <NumberCell
                      value={p.h}
                      max={4320}
                      className="w-16"
                      onChange={(v) => update((d) => void (d.profiles[id].h = v))}
                    />
                  </span>
                </td>
                <td className="pr-2">
                  <NumberCell
                    value={p.fps}
                    max={240}
                    className="w-14"
                    onChange={(v) => update((d) => void (d.profiles[id].fps = v))}
                  />
                </td>
                <td className="pr-2">
                  <NumberCell
                    value={p.v_kbps}
                    max={MAX_VIDEO_KBPS}
                    onChange={(v) => update((d) => void (d.profiles[id].v_kbps = v))}
                  />
                </td>
                <td className="pr-2">
                  <NumberCell
                    value={p.a_kbps}
                    max={MAX_AUDIO_KBPS}
                    onChange={(v) => update((d) => void (d.profiles[id].a_kbps = v))}
                  />
                </td>
                <td className="text-right">
                  <button
                    className="mr-1 text-xs text-zinc-400 hover:text-white"
                    onClick={() =>
                      update((d) => {
                        let n = 1;
                        while (d.profiles[`${id}-copy${n}`]) n++;
                        d.profiles[`${id}-copy${n}`] = { ...p, name: `${p.name}-copy${n}` };
                      })
                    }
                  >
                    {t("settings.duplicate")}
                  </button>
                  <button
                    className="text-xs text-zinc-400 hover:text-red-400 disabled:opacity-30"
                    disabled={BUILTIN_IDS.includes(id)}
                    onClick={() => update((d) => void delete d.profiles[id])}
                  >
                    {t("settings.delete")}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {dirty && (
          <div className="mt-2 flex justify-end">
            <button
              className="rounded bg-sky-600 px-3 py-1 text-sm font-semibold hover:bg-sky-500"
              onClick={() => void save()}
            >
              {t("app.save")}
            </button>
          </div>
        )}

        <div className="mt-4 border-t border-zinc-800 pt-3 text-sm">
          <button
            className="text-sky-400 hover:underline"
            onClick={() => void api.openLogsDir().catch(() => undefined)}
          >
            {t("app.logs")}
          </button>
          <p className="mt-2 text-xs text-zinc-500">{t("app.licenses")}</p>
          <p className="mt-1 text-xs text-zinc-600">
            ezStreamer: MIT — GStreamer: LGPL 2.1+. Sources:
            https://gstreamer.freedesktop.org/download/
          </p>
        </div>
      </div>
    </div>
  );
}

const BUILTIN_IDS = ["low", "mid", "high", "1080p"];

function NumberCell({
  value,
  max,
  onChange,
  className = "w-20",
}: {
  value: number;
  max: number;
  onChange: (v: number) => void;
  className?: string;
}) {
  const over = value > max;
  return (
    <input
      type="number"
      className={`${className} rounded border bg-zinc-800 px-1 py-0.5 ${over ? "border-red-600" : "border-zinc-700"}`}
      value={value}
      onChange={(e) => onChange(Number(e.target.value))}
    />
  );
}
