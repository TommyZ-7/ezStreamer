import { useTranslation } from "react-i18next";
import { useStore } from "../store";
import { MAX_AUDIO_KBPS, MAX_VIDEO_KBPS } from "../lib/types";

const BUILTIN_IDS = ["low", "mid", "high", "1080p"];

export function ProfileSelector() {
  const { t } = useTranslation();
  const profiles = useStore((s) => s.profiles);
  const profileId = useStore((s) => s.profileId);
  const setProfileId = useStore((s) => s.setProfileId);
  const encoderOverride = useStore((s) => s.encoderOverride);
  const setEncoderOverride = useStore((s) => s.setEncoderOverride);
  const encoders = useStore((s) => s.encoders);
  const encodersLoading = useStore((s) => s.encodersLoading);

  const allProfiles = profiles?.profiles ?? {};
  const allIds = Object.keys(allProfiles);
  // Built-ins first, then custom profiles (F-EN-02: custom profiles must be
  // selectable here, not only editable in Settings).
  const orderedIds = [
    ...BUILTIN_IDS.filter((id) => allIds.includes(id)),
    ...allIds.filter((id) => !BUILTIN_IDS.includes(id)),
  ];
  const profile = allProfiles[profileId];
  const overBitrate =
    profile != null && (profile.v_kbps > MAX_VIDEO_KBPS || profile.a_kbps > MAX_AUDIO_KBPS);
  const warn = profile?.warn
    ? t(profile.warn, { defaultValue: t("profile.warn1080p") })
    : profileId === "1080p"
      ? t("profile.warn1080p")
      : null;

  return (
    <section className="space-y-2">
      <h2 className="text-sm font-semibold text-zinc-400">{t("profile.tab")}</h2>
      <div className="flex flex-wrap gap-2 text-sm">
        {orderedIds.map((id) => {
          const p = allProfiles[id];
          const label = p.name.startsWith("profile.") ? t(p.name) : p.name;
          const highRes = id === "1080p" || p.w >= 1920;
          return (
            <button
              key={id}
              title={p.warn ? t(p.warn, { defaultValue: t("profile.warn1080p") }) : undefined}
              className={`rounded border px-3 py-1 ${
                profileId === id
                  ? "border-sky-500 bg-sky-900/40"
                  : "border-zinc-700 hover:border-zinc-500"
              }`}
              onClick={() => setProfileId(id)}
            >
              {label}
              {highRes && <span className="ml-1 text-amber-400">⚠</span>}
            </button>
          );
        })}
        {profile && (
          <span className="self-center text-xs text-zinc-500">
            {profile.w}x{profile.h} {profile.fps}fps / {profile.v_kbps}k / {profile.a_kbps}k
          </span>
        )}
      </div>
      {warn && <p className="text-xs text-amber-500">{warn}</p>}
      {overBitrate && <p className="text-xs font-semibold text-red-500">{t("profile.overBitrate")}</p>}

      <label className="flex items-center gap-2 text-sm">
        {t("profile.encoder")}
        <select
          className="rounded border border-zinc-700 bg-zinc-800 px-2 py-1"
          value={encoderOverride}
          onChange={(e) => setEncoderOverride(e.target.value)}
        >
          <option value="auto">{t("profile.auto")}</option>
          {encoders.map((e) => (
            <option key={e.name} value={e.name} disabled={!e.usable}>
              {e.name}
              {e.reason ? ` (${e.reason})` : ""}
            </option>
          ))}
          {/* Software/HW ids stay selectable even when the probe has not run
              or failed; a missing element fails at start with a clear error. */}
          {!encoders.some((e) => e.name === "libx264") && (
            <option value="libx264">libx264</option>
          )}
          {!encoders.some((e) => e.name === "h264_vulkan") && (
            <option value="h264_vulkan">h264_vulkan</option>
          )}
        </select>
      </label>
      {encodersLoading && (
        <p className="text-xs text-zinc-500">{t("profile.checkingEncoders")}</p>
      )}
    </section>
  );
}
