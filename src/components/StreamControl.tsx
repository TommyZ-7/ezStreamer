import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useStore } from "../store";
import { api } from "../lib/api";
import { genericKeyWarning, playbackUrls, validateStreamKey } from "../lib/urls";

export function StreamControl() {
  const { t } = useTranslation();
  const streamKey = useStore((s) => s.streamKey);
  const ingestUrl = useStore((s) => s.ingestUrl);
  const isLive = useStore((s) => s.isLive);
  const status = useStore((s) => s.status);
  const backendError = useStore((s) => s.backendError);
  const screenId = useStore((s) => s.screen.id);
  const previewing = useStore((s) => s.previewing);
  const showToast = useStore((s) => s.showToast);
  const setIngestUrl = useStore((s) => s.setIngestUrl);
  const setStreamKey = useStore((s) => s.setStreamKey);
  const startStream = useStore((s) => s.startStream);
  const stopStream = useStore((s) => s.stopStream);
  const startPreview = useStore((s) => s.startPreview);
  const stopPreview = useStore((s) => s.stopPreview);
  const keyError = validateStreamKey(streamKey);
  const genericWarn = streamKey.length > 0 && genericKeyWarning(streamKey);
  const [backendMissing, setBackendMissing] = useState(false);
  // F-ST-04: while a reconnect is pending the stream is neither idle nor
  // live; the button must cancel it (stop_stream cancels the retry).
  const retrying = status.retrying != null;
  const active = isLive || retrying;

  const urls = playbackUrls(ingestUrl, streamKey || "your-key");
  const canStart = !active && streamKey.length > 0 && keyError === null && !backendMissing;

  useEffect(() => {
    // surface the start error once (backend without capture)
    if (backendError?.includes("not implemented") || backendError?.includes("NotImplemented")) {
      setBackendMissing(true);
    }
  }, [backendError]);

  const copy = async (text: string) => {
    try {
      await api.copyToClipboard(text);
    } catch {
      await navigator.clipboard.writeText(text).catch(() => undefined);
    }
    showToast(t("stream.copied"));
  };

  return (
    <section className="space-y-2">
      <label className="flex items-center gap-2 text-sm">
        {t("stream.ingest")}
        <input
          className="flex-1 rounded border border-zinc-700 bg-zinc-800 px-2 py-1"
          value={ingestUrl}
          onChange={(e) => setIngestUrl(e.target.value)}
        />
      </label>
      <label className="block text-sm">
        {t("stream.key")}
        <input
          className={`mt-1 w-full rounded border bg-zinc-800 px-2 py-1 ${
            keyError ? "border-red-600" : "border-zinc-700"
          }`}
          value={streamKey}
          onChange={(e) => setStreamKey(e.target.value)}
          placeholder="my-event-123"
        />
        {keyError && <span className="text-xs text-red-500">{t("stream.keyInvalid")}</span>}
        {!keyError && genericWarn && (
          <span className="text-xs text-amber-500">{t("stream.keyGeneric")}</span>
        )}
      </label>

      <div className="space-y-1 text-sm">
        <CopyRow label={t("stream.copyPc")} url={urls.pc} onCopy={() => copy(urls.pc)} />
        <CopyRow label={t("stream.copyQuest")} url={urls.quest} onCopy={() => copy(urls.quest)} />
      </div>

      {isLive && (
        <div className="flex justify-between text-xs text-zinc-400">
          <span>
            {t("stream.bitrate")}: {Math.round(status.bitrateKbps)} kbps
          </span>
          <span>
            {t("stream.dropped")}: {status.droppedFrames}
          </span>
        </div>
      )}
      {retrying && (
        <p className="text-center text-xs text-amber-400">
          {t("stream.retrying", { n: status.retrying })}
        </p>
      )}

      <button
        className={`w-full rounded py-3 text-lg font-bold ${
          active
            ? "bg-red-600 hover:bg-red-500 text-white"
            : canStart
              ? "bg-zinc-200 text-zinc-900 hover:bg-white"
              : "cursor-not-allowed bg-zinc-700 text-zinc-500"
        }`}
        disabled={!canStart && !active}
        onClick={() => (active ? void stopStream() : void startStream())}
      >
        {active ? `■ ${t("stream.stop")}` : `● ${t("stream.start")}`}
      </button>
      {!active && (
        <button
          className={`w-full rounded border py-2 text-sm ${
            previewing
              ? "border-sky-500 text-sky-400 hover:border-sky-400"
              : screenId
                ? "border-zinc-600 text-zinc-300 hover:border-zinc-400"
                : "cursor-not-allowed border-zinc-800 text-zinc-600"
          }`}
          disabled={!screenId && !previewing}
          onClick={() => (previewing ? void stopPreview() : void startPreview())}
        >
          {previewing ? t("stream.previewStop") : t("stream.previewStart")}
        </button>
      )}
      {!active && !screenId && !previewing && (
        <p className="text-center text-xs text-zinc-500">{t("stream.previewNeedsSource")}</p>
      )}
      {backendMissing && (
        <p className="text-center text-xs text-amber-500">{t("stream.notAvailable")}</p>
      )}
      {!active && backendError && !backendMissing && (
        <p className="text-center text-xs text-red-400 break-all">{backendError}</p>
      )}
    </section>
  );
}

function CopyRow({ label, url, onCopy }: { label: string; url: string; onCopy: () => void }) {
  return (
    <div className="flex items-center gap-2">
      <span className="w-20 shrink-0 text-zinc-500">{label}</span>
      <code className="flex-1 truncate rounded bg-zinc-800 px-2 py-1 text-xs">{url}</code>
      <button
        className="rounded border border-zinc-600 px-2 py-0.5 text-xs hover:border-zinc-400"
        onClick={onCopy}
      >
        copy
      </button>
    </div>
  );
}
