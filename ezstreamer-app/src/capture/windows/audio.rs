//! WASAPI audio capture (design.md §3.2.1).
//!
//! - system: render-device loopback (polling)
//! - mic:    capture device (polling)
//! - per-app: process loopback via ActivateAudioInterfaceAsync (Win10 2004+)

use super::{co_init, err, AudioCapture, Result};
use crate::events::UiSink;
use ezstreamer_core::audio::{resample_stereo, AudioSink, MIC_ID};
use ezstreamer_core::ipc_types::AudioSelection;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TARGET_RATE: u32 = 48_000;
const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 0x2;

pub fn start_audio(
    selection: &AudioSelection,
    sink: AudioSink,
    ui: Option<UiSink>,
) -> super::Result<AudioCapture> {
    let mut cap = AudioCapture::new(sink.clone());

    // F-AU-05: a saved-but-disconnected mic must fail loudly instead of
    // silently falling back to the default endpoint.
    if selection.mic.enabled {
        ensure_mic_device(&selection.mic.device)?;
    }
    let valid_apps: Vec<u32> = if selection.mode == "apps" {
        selection
            .apps
            .iter()
            .filter_map(|a| a.rsplit(':').next().and_then(|s| s.parse::<u32>().ok()))
            .collect()
    } else {
        Vec::new()
    };
    // PIDs go stale: `selected_apps` persists across restarts while process
    // ids change on every launch (0x80070002 at activation). Drop dead PIDs
    // loudly instead of capturing silence or erroring per-thread.
    let mut live_apps: Vec<u32> = Vec::with_capacity(valid_apps.len());
    for pid in valid_apps {
        if pid_alive(pid) {
            live_apps.push(pid);
        } else {
            let msg = format!("audio app (pid:{pid}) is no longer running; reselect it");
            crate::logging::error(&msg);
            if let Some(ui) = ui.as_ref() {
                ui.error("audio", &msg);
            }
        }
    }
    let valid_apps = live_apps;
    if selection.mode != "system" && !selection.mic.enabled && valid_apps.is_empty() {
        return Err(super::CaptureError::Failed(
            "no audio source selected (selected apps are no longer running?)".into(),
        ));
    }
    crate::logging::info(&format!(
        "wasapi start: mode={} mic_enabled={} mic_device={} apps={:?}",
        selection.mode, selection.mic.enabled, selection.mic.device, valid_apps
    ));

    if selection.mode == "system" {
        let sink2 = sink.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let ui2 = ui.clone();
        let handle = std::thread::Builder::new()
            .name("wasapi-system".into())
            .spawn(move || {
                if let Err(e) = run_system_loopback(sink2, stop2) {
                    crate::logging::error(&format!("system loopback ended: {e}"));
                    if let Some(ui2) = ui2.as_ref() {
                        ui2.error("audio", &e);
                    }
                }
            })
            .map_err(err)?;
        cap.add(stop, handle);
    }

    if selection.mic.enabled {
        let sink2 = sink.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let device = selection.mic.device.clone();
        let ui2 = ui.clone();
        let handle = std::thread::Builder::new()
            .name("wasapi-mic".into())
            .spawn(move || {
                if let Err(e) = run_mic(sink2, stop2, &device) {
                    crate::logging::error(&format!("mic capture ended: {e}"));
                    if let Some(ui2) = ui2.as_ref() {
                        ui2.error("audio", &e);
                    }
                }
            })
            .map_err(err)?;
        cap.add(stop, handle);
    }

    for pid in valid_apps {
        let sink2 = sink.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let ui2 = ui.clone();
        let handle = std::thread::Builder::new()
            .name(format!("wasapi-app-{pid}"))
            .spawn(move || {
                if let Err(e) = run_process_loopback(pid, sink2, stop2) {
                    crate::logging::error(&format!("process loopback ({pid}) ended: {e}"));
                    if let Some(ui2) = ui2.as_ref() {
                        ui2.error("audio", &e);
                    }
                }
            })
            .map_err(err)?;
        cap.add(stop, handle);
    }

    Ok(cap)
}

/// True when a process with this PID currently exists. Used to drop stale
/// app selections (persisted PIDs from previous launches) before attempting
/// process-loopback activation, which would otherwise fail with 0x80070002.
fn pid_alive(pid: u32) -> bool {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                let _ = windows::Win32::Foundation::CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}

/// True when `device_id` is "default"/empty or an active capture endpoint.
/// Runs its own COM init so callers on any thread can use it.
fn ensure_mic_device(device_id: &str) -> super::Result<()> {
    use windows::Win32::Media::Audio::{eCapture, DEVICE_STATE_ACTIVE};
    if device_id.is_empty() || device_id == "default" {
        return Ok(());
    }
    co_init();
    unsafe {
        let enumerator: windows::Win32::Media::Audio::IMMDeviceEnumerator =
            windows::Win32::System::Com::CoCreateInstance(
                &windows::Win32::Media::Audio::MMDeviceEnumerator,
                None,
                windows::Win32::System::Com::CLSCTX_ALL,
            )
            .map_err(err)?;
        let collection = enumerator
            .EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)
            .map_err(err)?;
        let count = collection.GetCount().map_err(err)?;
        for i in 0..count {
            let Ok(dev) = collection.Item(i) else {
                continue;
            };
            let id = dev
                .GetId()
                .map(|w| w.to_string().unwrap_or_default())
                .unwrap_or_default();
            if id == device_id {
                return Ok(());
            }
        }
    }
    Err(super::CaptureError::Failed(format!(
        "microphone device not found: {device_id} (F-AU-05)"
    )))
}

fn run_system_loopback(sink: AudioSink, stop: Arc<AtomicBool>) -> Result<()> {
    use windows::Win32::Media::Audio::{eMultimedia, eRender};
    co_init();
    unsafe {
        let enumerator: windows::Win32::Media::Audio::IMMDeviceEnumerator =
            windows::Win32::System::Com::CoCreateInstance(
                &windows::Win32::Media::Audio::MMDeviceEnumerator,
                None,
                windows::Win32::System::Com::CLSCTX_ALL,
            )
            .map_err(err)?;
        let dev = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(err)?;
        let client: windows::Win32::Media::Audio::IAudioClient = dev
            .Activate(windows::Win32::System::Com::CLSCTX_ALL, None)
            .map_err(err)?;
        let fmt = client.GetMixFormat().map_err(err)?;
        let desc = describe_format(fmt);
        crate::logging::info(&format!("wasapi system loopback mix format: {desc}"));
        check_float_format(fmt, &desc)?;
        let (rate, channels) = ((*fmt).nSamplesPerSec, (*fmt).nChannels.max(1) as usize);
        wasapi_polling(
            client,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            "system".into(),
            sink,
            stop,
            fmt,
            rate,
            channels,
        )
    }
}

fn run_mic(sink: AudioSink, stop: Arc<AtomicBool>, device_id: &str) -> Result<()> {
    use windows::Win32::Media::Audio::{eCapture, eMultimedia};
    co_init();
    unsafe {
        let enumerator: windows::Win32::Media::Audio::IMMDeviceEnumerator =
            windows::Win32::System::Com::CoCreateInstance(
                &windows::Win32::Media::Audio::MMDeviceEnumerator,
                None,
                windows::Win32::System::Com::CLSCTX_ALL,
            )
            .map_err(err)?;
        // F-AU-03/F-AU-05: honor the selected capture endpoint, not just the
        // system default.
        let dev = if device_id.is_empty() || device_id == "default" {
            enumerator
                .GetDefaultAudioEndpoint(eCapture, eMultimedia)
                .map_err(err)?
        } else {
            enumerator
                .GetDevice(&windows::core::HSTRING::from(device_id))
                .map_err(|e| {
                    super::CaptureError::Failed(format!(
                        "microphone device unavailable: {device_id} ({e})"
                    ))
                })?
        };
        let client: windows::Win32::Media::Audio::IAudioClient = dev
            .Activate(windows::Win32::System::Com::CLSCTX_ALL, None)
            .map_err(err)?;
        let fmt = client.GetMixFormat().map_err(err)?;
        let desc = describe_format(fmt);
        crate::logging::info(&format!(
            "wasapi mic mix format: {desc} (device={device_id})"
        ));
        check_float_format(fmt, &desc)?;
        let (rate, channels) = ((*fmt).nSamplesPerSec, (*fmt).nChannels.max(1) as usize);
        wasapi_polling(client, 0, MIC_ID.into(), sink, stop, fmt, rate, channels)
    }
}

use windows::Win32::Media::Audio::AUDCLNT_STREAMFLAGS_LOOPBACK;

/// One-line description of a WASAPI mix format for the log (tag/bits/rate/ch).
/// `fmt` comes from `IAudioClient::GetMixFormat` (possibly WAVEFORMATEXTENSIBLE).
/// NOTE: `WAVEFORMATEX` is a packed struct in the `windows` crate, so every
/// field access must go through `addr_of!` + `read_unaligned` (E0793).
unsafe fn describe_format(fmt: *const windows::Win32::Media::Audio::WAVEFORMATEX) -> String {
    if fmt.is_null() {
        return "null".into();
    }
    let tag: u16 = std::ptr::addr_of!((*fmt).wFormatTag).read_unaligned();
    let ch: u16 = std::ptr::addr_of!((*fmt).nChannels).read_unaligned();
    let rate: u32 = std::ptr::addr_of!((*fmt).nSamplesPerSec).read_unaligned();
    let bits: u16 = std::ptr::addr_of!((*fmt).wBitsPerSample).read_unaligned();
    let cb: u16 = std::ptr::addr_of!((*fmt).cbSize).read_unaligned();
    let mut s = format!("tag={tag:#06x} ch={ch} rate={rate} bits={bits} cbSize={cb}");
    // WAVE_FORMAT_EXTENSIBLE (0xFFFE): the float/PCM discriminator lives in
    // the trailing SubFormat GUID (data1 3 = float, 1 = PCM).
    if tag == 0xFFFE && cb >= 22 {
        let base = fmt as *const u8;
        let data1 = (base.add(24) as *const u32).read_unaligned();
        s.push_str(&format!(" subfmt_data1={data1}"));
    }
    s
}

/// The polling loop below reinterprets capture buffers as f32. Shared-mode
/// WASAPI almost always mixes float, but a driver reporting integer PCM would
/// otherwise decode as near-zero garbage with no error. Fail loudly instead.
unsafe fn check_float_format(
    fmt: *const windows::Win32::Media::Audio::WAVEFORMATEX,
    desc: &str,
) -> Result<()> {
    if fmt.is_null() {
        return Err(err("WASAPI mix format is null"));
    }
    let tag: u16 = std::ptr::addr_of!((*fmt).wFormatTag).read_unaligned();
    let bits: u16 = std::ptr::addr_of!((*fmt).wBitsPerSample).read_unaligned();
    let cb: u16 = std::ptr::addr_of!((*fmt).cbSize).read_unaligned();
    const IEEE_FLOAT: u16 = 3;
    const EXTENSIBLE: u16 = 0xFFFE;
    if tag == IEEE_FLOAT && bits == 32 {
        return Ok(());
    }
    if tag == EXTENSIBLE && bits == 32 && cb >= 22 {
        let base = fmt as *const u8;
        let data1 = (base.add(24) as *const u32).read_unaligned();
        if data1 == 3 {
            return Ok(());
        }
    }
    Err(err(format!(
        "unsupported WASAPI mix format ({desc}): expected 32-bit float"
    )))
}

/// Shared-mode WASAPI capture (polling). System loopback passes
/// AUDCLNT_STREAMFLAGS_LOOPBACK, mic passes 0. `fmt` is the format passed to
/// Initialize (mix format for devices; our float48k format for process loopback).
unsafe fn wasapi_polling(
    client: windows::Win32::Media::Audio::IAudioClient,
    extra_flags: u32,
    id: String,
    sink: AudioSink,
    stop: Arc<AtomicBool>,
    fmt: *const windows::Win32::Media::Audio::WAVEFORMATEX,
    rate: u32,
    channels: usize,
) -> Result<()> {
    use windows::Win32::Media::Audio::{
        IAudioCaptureClient, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    };

    client
        .Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            extra_flags
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            1_000_000, // 100ms
            0,
            fmt,
            None,
        )
        .map_err(|e| err(format!("wasapi {id}: Initialize failed: {e}")))?;
    let capture: IAudioCaptureClient = client
        .GetService()
        .map_err(|e| err(format!("wasapi {id}: GetService failed: {e}")))?;
    client
        .Start()
        .map_err(|e| err(format!("wasapi {id}: Start failed: {e}")))?;
    crate::logging::info(&format!(
        "wasapi {id} started: rate={rate} channels={channels}"
    ));

    // Diagnosis counters: a thread that never sees packets (wrong endpoint,
    // exclusive-mode holder, privacy block) and one that only sees SILENT
    // packets (loopback with nothing playing) both surface as "VU frozen at
    // 0", so log the split every 5s plus the first signal/silence transition.
    let started = std::time::Instant::now();
    let mut next_stats = started + Duration::from_secs(5);
    let mut polls: u64 = 0;
    let mut empty_polls: u64 = 0;
    let mut signal_blocks: u64 = 0;
    let mut silent_blocks: u64 = 0;
    let mut signal_frames: u64 = 0;
    let mut saw_signal = false;
    let mut saw_silent = false;
    // Peak (max |sample|) of signal content in the current 5s window.
    // A "signal" flag with peak 0 means the device feeds digital zeros
    // (OS-level mute, privacy block, or nothing rendered) — distinct from
    // SILENT-flagged packets and from an empty queue.
    let mut peak_max: f32 = 0.0;

    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = client.Stop();
            crate::logging::info(&format!(
                "wasapi {id} stopped after {:?}: polls={polls} empty={empty_polls} signal_blocks={signal_blocks} silent_blocks={silent_blocks} signal_frames={signal_frames}",
                started.elapsed()
            ));
            return Ok(());
        }
        let mut packet = capture
            .GetNextPacketSize()
            .map_err(|e| err(format!("wasapi {id}: GetNextPacketSize failed: {e}")))?;
        polls += 1;
        if packet == 0 {
            empty_polls += 1;
        }
        while packet > 0 {
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            if capture
                .GetBuffer(&mut ptr, &mut frames, &mut flags, None, None)
                .is_err()
            {
                break;
            }
            if frames > 0 {
                if (flags & AUDCLNT_BUFFERFLAGS_SILENT) == 0 && !ptr.is_null() {
                    let raw =
                        std::slice::from_raw_parts(ptr as *const f32, frames as usize * channels);
                    let mut stereo = Vec::with_capacity(frames as usize * 2);
                    for i in 0..frames as usize {
                        // Mono mics (channels == 1) duplicate the channel;
                        // >2 channels fold down to the first two.
                        let l = raw[i * channels];
                        let r = if channels >= 2 {
                            raw[i * channels + 1]
                        } else {
                            l
                        };
                        stereo.push(l);
                        stereo.push(r);
                    }
                    let block = resample_stereo(&stereo, rate, TARGET_RATE);
                    signal_frames += (block.len() / 2) as u64;
                    signal_blocks += 1;
                    let peak = block.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
                    if peak > peak_max {
                        peak_max = peak;
                    }
                    if !saw_signal {
                        saw_signal = true;
                        crate::logging::info(&format!(
                            "wasapi {id}: first signal block ({frames} frames, peak={peak:.6})"
                        ));
                    }
                    if !sink.push(&id, block) {
                        let _ = client.Stop();
                        return Ok(());
                    }
                } else {
                    // Silence resampled to 48k like signal so block durations
                    // stay consistent across rates (zeros resample to zeros).
                    let silent: Vec<f32> = vec![0.0; frames as usize * 2];
                    let block = resample_stereo(&silent, rate, TARGET_RATE);
                    silent_blocks += 1;
                    if !saw_silent {
                        saw_silent = true;
                        crate::logging::info(&format!(
                            "wasapi {id}: first silent block ({frames} frames)"
                        ));
                    }
                    if !sink.push(&id, block) {
                        let _ = client.Stop();
                        return Ok(());
                    }
                }
            }
            let _ = capture.ReleaseBuffer(frames);
            packet = capture
                .GetNextPacketSize()
                .map_err(|e| err(format!("wasapi {id}: GetNextPacketSize failed: {e}")))?;
        }
        if std::time::Instant::now() >= next_stats {
            crate::logging::info(&format!(
                "wasapi {id} stats: polls={polls} empty={empty_polls} signal_blocks={signal_blocks} silent_blocks={silent_blocks} signal_frames={signal_frames} peak_max={peak_max:.6}"
            ));
            next_stats = std::time::Instant::now() + Duration::from_secs(5);
            peak_max = 0.0;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

// --- per-app process loopback ------------------------------------------------

#[windows::core::implement(
    windows::Win32::Media::Audio::IActivateAudioInterfaceCompletionHandler,
    windows::Win32::System::Com::IAgileObject
)]
struct LoopbackActivation {
    result: Arc<Mutex<Option<windows::core::IUnknown>>>,
    hr: Arc<Mutex<windows::core::HRESULT>>,
    ready: Arc<(Mutex<bool>, std::sync::Condvar)>,
}

impl windows::Win32::Media::Audio::IActivateAudioInterfaceCompletionHandler_Impl
    for LoopbackActivation
{
    fn ActivateCompleted(
        &self,
        activateoperation: Option<
            &windows::Win32::Media::Audio::IActivateAudioInterfaceAsyncOperation,
        >,
    ) -> windows::core::Result<()> {
        if let Some(op) = activateoperation {
            let mut hr = windows::core::HRESULT::default();
            let mut unk: Option<windows::core::IUnknown> = None;
            unsafe {
                let _ = op.GetActivateResult(&mut hr, &mut unk);
            }
            *self.result.lock().unwrap() = unk;
            *self.hr.lock().unwrap() = hr;
        }
        let (lock, cvar) = &*self.ready;
        let mut done = lock.lock().unwrap();
        *done = true;
        cvar.notify_all();
        Ok(())
    }
}

impl windows::Win32::System::Com::IAgileObject_Impl for LoopbackActivation {}

fn run_process_loopback(pid: u32, sink: AudioSink, stop: Arc<AtomicBool>) -> Result<()> {
    use windows::core::IUnknown;
    use windows::Win32::Media::Audio::{
        ActivateAudioInterfaceAsync, IActivateAudioInterfaceCompletionHandler, IAudioClient,
        AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, PROCESS_LOOPBACK_MODE,
    };
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::BLOB;
    use windows::Win32::System::Variant::VT_BLOB;

    const PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE: PROCESS_LOOPBACK_MODE =
        PROCESS_LOOPBACK_MODE(0);

    co_init();
    unsafe {
        let params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: windows::Win32::Media::Audio::AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        let blob = BLOB {
            cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
            pBlobData: &params as *const _ as *mut u8,
        };
        let mut var = PROPVARIANT::default();
        var.Anonymous = windows::Win32::System::Com::StructuredStorage::PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(
                windows::Win32::System::Com::StructuredStorage::PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: windows::Win32::System::Com::StructuredStorage::PROPVARIANT_0_0_0 {
                        blob,
                    },
                },
            ),
        };

        let result: Arc<Mutex<Option<windows::core::IUnknown>>> = Arc::new(Mutex::new(None));
        let hr: Arc<Mutex<windows::core::HRESULT>> =
            Arc::new(Mutex::new(windows::core::HRESULT::default()));
        let ready = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let handler: IActivateAudioInterfaceCompletionHandler = LoopbackActivation {
            result: result.clone(),
            hr: hr.clone(),
            ready: ready.clone(),
        }
        .into();

        ActivateAudioInterfaceAsync(
            windows::core::w!("VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK"),
            &<IAudioClient as windows::core::ComInterface>::IID,
            Some(&var),
            &handler,
        )
        .map_err(|e| {
            err(format!(
                "process loopback ({pid}) activation request failed: {e}"
            ))
        })?;

        // wait for activation (max 3s)
        let (lock, cvar) = &*ready;
        let guard = lock.lock().map_err(err)?;
        let (_guard, _timeout) = cvar
            .wait_timeout_while(guard, Duration::from_secs(3), |done| !*done)
            .map_err(err)?;
        let activate_hr = *hr.lock().map_err(err)?;
        if activate_hr.is_err() {
            return Err(err(format!(
                "process loopback ({pid}) activation failed: HRESULT 0x{:08X}",
                activate_hr.0 as u32
            )));
        }
        let unk = result.lock().map_err(err)?.take().ok_or_else(|| {
            err(format!(
                "process loopback ({pid}) activation timed out (HRESULT 0x{:08X})",
                activate_hr.0 as u32
            ))
        })?;
        crate::logging::info(&format!("wasapi pid:{pid} process loopback activated"));

        let client: IAudioClient = <IUnknown as windows::core::ComInterface>::cast(&unk)
            .map_err(|e| err(format!("process loopback ({pid}) cast failed: {e}")))?;

        // float32 48kHz stereo
        let format = windows::Win32::Media::Audio::WAVEFORMATEX {
            wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
            nChannels: 2,
            nSamplesPerSec: TARGET_RATE,
            nAvgBytesPerSec: TARGET_RATE * 8,
            nBlockAlign: 8,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        wasapi_polling(
            client,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            format!("pid:{pid}"),
            sink,
            stop,
            &format,
            TARGET_RATE,
            2,
        )
    }
}
