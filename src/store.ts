import { create } from "zustand";
import i18n from "./i18n";
import { api } from "./lib/api";
import type {
  AppMixEntry,
  AudioDevices,
  Display,
  EncoderInfo,
  MicSource,
  ProfilesConfig,
  ScreenTarget,
  StreamStatus,
  VuMeter,
  WindowInfo,
} from "./lib/types";

export type Tab = "screen" | "audio" | "profile";

interface AppState {
  tab: Tab;
  setTab: (t: Tab) => void;

  // data from backend
  displays: Display[];
  windows: WindowInfo[];
  audioDevices: AudioDevices | null;
  encoders: EncoderInfo[];
  profiles: ProfilesConfig | null;

  // selections (persisted via save_profiles)
  screen: ScreenTarget;
  audioMode: "system" | "apps";
  selectedApps: string[];
  mic: MicSource;
  /** F-SC-04: cursor capture toggle (initial ON, persisted). */
  cursor: boolean;
  /** Per-app live gain/mute (F-AU-04; session-only, not persisted). */
  appMix: Record<string, AppMixEntry>;
  /** F-CF-05: UI language; persisted to profiles.json. */
  locale: "ja" | "en";
  profileId: string;
  encoderOverride: string;
  ingestUrl: string;
  streamKey: string;

  // runtime
  status: StreamStatus;
  isLive: boolean;
  previewing: boolean;
  toast: string | null;
  backendError: string | null;
  preview: string | null;
  vu: VuMeter;
  // F-1: startup phases. `booted` gates the splash screen (fast base data),
  // `encodersLoading` tracks the slow background encoder probe.
  booted: boolean;
  encodersLoading: boolean;
  setPreview: (url: string | null) => void;
  refreshVu: () => Promise<void>;

  loadAll: () => Promise<void>;
  loadBase: () => Promise<void>;
  loadEncoders: () => Promise<void>;
  setScreen: (s: ScreenTarget) => void;
  setAudioMode: (m: "system" | "apps") => void;
  toggleApp: (id: string) => void;
  setMic: (m: Partial<MicSource>) => void;
  setCursor: (v: boolean) => void;
  setAppMix: (id: string, patch: Partial<AppMixEntry>) => void;
  pushMix: () => void;
  setLocale: (l: "ja" | "en") => void;
  setProfileId: (id: string) => void;
  setEncoderOverride: (e: string) => void;
  setIngestUrl: (u: string) => void;
  setStreamKey: (k: string) => void;
  setProfiles: (p: ProfilesConfig) => void;
  showToast: (msg: string) => void;
  refreshStatus: () => Promise<void>;
  startStream: () => Promise<void>;
  stopStream: () => Promise<void>;
  startPreview: () => Promise<void>;
  stopPreview: () => Promise<void>;
}

const emptyStatus: StreamStatus = {
  isLive: false,
  durationSec: 0,
  bitrateKbps: 0,
  droppedFrames: 0,
  retrying: null,
};

// Preview screen-switch restart (fixes "stuck on previous screen").
// setScreen() clears the stale frame immediately and re-issues start_preview
// with the new target. Rapid switches are debounced so only the last
// selection hits the backend; the generation counter drops stale callbacks
// (e.g. user pressed stop while a restart was in flight).
let previewRestartTimer: ReturnType<typeof setTimeout> | null = null;
let previewRestartGen = 0;

function cancelPendingPreviewRestart() {
  previewRestartGen += 1;
  if (previewRestartTimer !== null) {
    clearTimeout(previewRestartTimer);
    previewRestartTimer = null;
  }
}

// F-CF-02: selections, stream key, ingest URL and locale are auto-saved
// (debounced) so the next launch restores them. Profile list edits go
// through SettingsModal, which saves explicitly.
let persistTimer: ReturnType<typeof setTimeout> | null = null;
let mixTimer: ReturnType<typeof setTimeout> | null = null;

function configFromState(s: AppState): ProfilesConfig | null {
  if (!s.profiles) return null;
  return {
    ...s.profiles,
    locale: s.locale,
    ingestUrl: s.ingestUrl,
    activeProfile: s.profileId,
    encoderOverride: s.encoderOverride,
    lastStreamKey: s.streamKey,
    lastSources: {
      screen: s.screen,
      includeApps: s.selectedApps,
      mic: s.mic,
      cursor: s.cursor,
    },
  };
}

function schedulePersist() {
  if (persistTimer !== null) clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    persistTimer = null;
    const next = configFromState(useStore.getState());
    if (!next) return;
    void api.saveProfiles(next).catch(() => undefined);
  }, 400);
}

function scheduleMix() {
  if (mixTimer !== null) clearTimeout(mixTimer);
  mixTimer = setTimeout(() => {
    mixTimer = null;
    useStore.getState().pushMix();
  }, 100);
}

export const useStore = create<AppState>((set, get) => ({
  tab: "screen",
  setTab: (t) => set({ tab: t }),

  displays: [],
  windows: [],
  audioDevices: null,
  encoders: [],
  profiles: null,

  screen: { type: "display", id: "" },
  audioMode: "system",
  selectedApps: [],
  mic: { device: "default", enabled: true, muted: false, gain: 1.0 },
  cursor: true,
  appMix: {},
  locale: i18n.language === "ja" ? "ja" : "en",
  profileId: "mid",
  encoderOverride: "auto",
  ingestUrl: "rtmp://topaz.chat/live",
  streamKey: "",

  status: emptyStatus,
  isLive: false,
  previewing: false,
  toast: null,
  backendError: null,
  preview: null,
  vu: { apps: {}, mic: null, master: { peak: 0, rms: 0 } },
  booted: false,
  encodersLoading: true,

  async loadBase() {
    const [displays, windows, audioDevices, profiles] = await Promise.allSettled([
      api.getDisplays(),
      api.getWindows(),
      api.getAudioDevices(),
      api.getProfiles(),
    ]);
    const backendError =
      displays.status === "rejected" ? String(displays.reason) : null;
    const cfg = profiles.status === "fulfilled" ? profiles.value : null;
    // F-CF-05: an explicit saved locale wins; otherwise follow the webview.
    const savedLocale = cfg?.locale;
    const locale: "ja" | "en" =
      savedLocale === "ja" || savedLocale === "en"
        ? savedLocale
        : i18n.language === "ja"
          ? "ja"
          : "en";
    void i18n.changeLanguage(locale);
    set({
      displays: displays.status === "fulfilled" ? displays.value : [],
      windows: windows.status === "fulfilled" ? windows.value : [],
      audioDevices: audioDevices.status === "fulfilled" ? audioDevices.value : null,
      profiles: cfg,
      backendError,
      booted: true,
      locale,
      ...(cfg
        ? {
            ingestUrl: cfg.ingestUrl,
            streamKey: cfg.lastStreamKey,
            profileId: cfg.activeProfile,
            encoderOverride: cfg.encoderOverride,
            screen: cfg.lastSources.screen,
            selectedApps: cfg.lastSources.includeApps,
            mic: cfg.lastSources.mic,
            cursor: cfg.lastSources.cursor ?? true,
            audioMode: cfg.lastSources.includeApps.length > 0 ? "apps" : "system",
          }
        : {}),
    });
  },

  async loadEncoders() {
    set({ encodersLoading: true });
    try {
      const encoders = await api.probeEncoders();
      set({ encoders });
    } catch {
      set({ encoders: [] });
    } finally {
      set({ encodersLoading: false });
    }
  },

  async loadAll() {
    await get().loadBase();
    await get().loadEncoders();
  },

  setScreen: (screen) => {
    const prev = get().screen;
    if (prev.type === screen.type && prev.id === screen.id) return;
    const wasPreviewing = get().previewing && !get().isLive;
    // Drop the stale frame at once; otherwise the UI keeps showing the
    // previous screen until (or unless) a new frame arrives.
    set(wasPreviewing ? { screen, preview: null } : { screen });
    schedulePersist();
    if (!wasPreviewing) return;
    const gen = ++previewRestartGen;
    if (previewRestartTimer !== null) {
      clearTimeout(previewRestartTimer);
    }
    previewRestartTimer = setTimeout(() => {
      previewRestartTimer = null;
      // A newer switch / stop supersedes this one.
      if (gen !== previewRestartGen) return;
      if (!get().previewing || get().isLive) return;
      const s = get();
      void api
        .startPreview({
          ingestUrl: s.ingestUrl,
          streamKey: s.streamKey,
          screen: s.screen,
          audio: { mode: s.audioMode, apps: s.selectedApps, mic: s.mic },
          profileId: s.profileId,
          encoderOverride: s.encoderOverride,
          cursor: s.cursor,
          appMix: s.appMix,
        })
        .then(() => {
          if (gen === previewRestartGen) {
            set({ previewing: true });
          } else if (!get().previewing) {
            // Stopped while the restart was in flight: don't leave it running.
            api.stopPreview().catch(() => undefined);
          }
        })
        .catch((e) => {
          if (gen !== previewRestartGen) return;
          set({ previewing: false, backendError: String(e) });
          get().showToast(String(e));
        });
    }, 250);
  },
  setAudioMode: (audioMode) => {
    set({ audioMode });
    schedulePersist();
  },
  toggleApp: (id) => {
    set((s) => ({
      selectedApps: s.selectedApps.includes(id)
        ? s.selectedApps.filter((a) => a !== id)
        : [...s.selectedApps, id],
    }));
    schedulePersist();
  },
  setMic: (m) => {
    set((s) => ({ mic: { ...s.mic, ...m } }));
    schedulePersist();
    scheduleMix(); // F-AU-03/04: live gain/mute updates
  },
  setCursor: (cursor) => {
    set({ cursor });
    schedulePersist();
  },
  setAppMix: (id, patch) => {
    set((s) => {
      const base: AppMixEntry = s.appMix[id] ?? { gain: 1, muted: false };
      return { appMix: { ...s.appMix, [id]: { ...base, ...patch } } };
    });
    scheduleMix();
  },
  pushMix: () => {
    const s = get();
    if (!s.isLive) return;
    void api
      .updateAudioMix({
        apps: Object.fromEntries(
          Object.entries(s.appMix).map(([id, m]) => [
            id,
            { gain: m.gain, muted: m.muted },
          ])
        ),
        mic: { enabled: s.mic.enabled, muted: s.mic.muted, gain: s.mic.gain },
      })
      .catch(() => undefined);
  },
  setLocale: (locale) => {
    void i18n.changeLanguage(locale);
    set({ locale });
    schedulePersist();
  },
  setProfileId: (profileId) => {
    set({ profileId });
    schedulePersist();
  },
  setEncoderOverride: (encoderOverride) => {
    set({ encoderOverride });
    schedulePersist();
  },
  setIngestUrl: (ingestUrl) => {
    set({ ingestUrl });
    schedulePersist();
  },
  setStreamKey: (streamKey) => {
    set({ streamKey });
    schedulePersist();
  },

  // SettingsModal saves explicitly; keep the in-memory base in sync without
  // triggering a second write.
  setProfiles: (profiles) => set({ profiles }),

  setPreview: (preview) => set({ preview }),

  async refreshVu() {
    try {
      const vu = await api.getVu();
      set({ vu });
    } catch {
      /* backend absent */
    }
  },

  showToast: (msg) => {
    set({ toast: msg });
    setTimeout(() => set((s) => (s.toast === msg ? { toast: null } : {})), 2000);
  },

  async refreshStatus() {
    try {
      const status = await api.getStatus();
      set({ status, isLive: status.isLive });
    } catch {
      /* backend absent in browser dev */
    }
  },

  async startStream() {
    cancelPendingPreviewRestart();
    const s = get();
    try {
      const status = await api.startStream({
        ingestUrl: s.ingestUrl,
        streamKey: s.streamKey,
        screen: s.screen,
        audio: { mode: s.audioMode, apps: s.selectedApps, mic: s.mic },
        profileId: s.profileId,
        encoderOverride: s.encoderOverride,
        cursor: s.cursor,
        appMix: s.appMix,
      });
      set({ status, isLive: status.isLive, previewing: false });
    } catch (e) {
      set({ backendError: String(e) });
    }
  },

  async stopStream() {
    cancelPendingPreviewRestart();
    try {
      await api.stopStream();
    } finally {
      set({ isLive: false, status: emptyStatus, previewing: false });
    }
  },

  async startPreview() {
    const s = get();
    try {
      await api.startPreview({
        ingestUrl: s.ingestUrl,
        streamKey: s.streamKey,
        screen: s.screen,
        audio: { mode: s.audioMode, apps: s.selectedApps, mic: s.mic },
        profileId: s.profileId,
        encoderOverride: s.encoderOverride,
        cursor: s.cursor,
        appMix: s.appMix,
      });
      set({ previewing: true });
    } catch (e) {
      set({ backendError: String(e) });
    }
  },

  async stopPreview() {
    cancelPendingPreviewRestart();
    try {
      await api.stopPreview();
    } finally {
      set({ previewing: false, preview: null });
    }
  },
}));
