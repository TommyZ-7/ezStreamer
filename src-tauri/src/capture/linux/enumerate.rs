//! Linux device enumeration: PipeWire registry probe for audio apps.
//!
//! Screen targets are not enumerated: on Wayland/Portal the OS picker is the
//! selection surface (design §3.1.2).

use super::{CaptureError, Result};
use ezstreamer_core::ipc_types::{AppAudio, AudioDevices, DeviceInfo, Display, WindowInfo};
use pipewire as pw;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn err<E: std::fmt::Display>(e: E) -> CaptureError {
    CaptureError::Failed(e.to_string())
}

// ---------------------------------------------------------------------------
// enumeration

pub fn list_displays() -> Result<Vec<Display>> {
    // Wayland/Portal: the OS picker is the selection surface (design §3.1.2)
    Ok(Vec::new())
}

pub fn list_windows() -> Result<Vec<WindowInfo>> {
    // Wayland/Portal: the OS picker is the selection path (design §3.1.2)
    Ok(Vec::new())
}

pub fn list_audio_devices() -> Result<AudioDevices> {
    let mut inputs: Vec<DeviceInfo> = vec![DeviceInfo {
        id: "default".into(),
        label: "Default Microphone".into(),
        is_default: true,
    }];

    let tl = match unsafe { pw::thread_loop::ThreadLoopBox::new(Some("ezstreamer-probe"), None) } {
        Ok(t) => t,
        Err(e) => return Err(err(e)),
    };
    let context = match pw::context::ContextBox::new(tl.loop_(), None) {
        Ok(c) => c,
        Err(e) => return Err(err(e)),
    };
    let Ok(core) = context.connect(None) else {
        return Ok(AudioDevices {
            inputs,
            outputs: Vec::new(),
            apps: Vec::new(),
        });
    };
    let registry = core.get_registry().map_err(err)?;

    struct Ud {
        apps: Vec<AppAudio>,
        sources: Vec<DeviceInfo>,
    }
    let ud = Arc::new(Mutex::new(Ud { apps: Vec::new(), sources: Vec::new() }));
    let ud2 = ud.clone();
    let _listener = registry
        .add_listener_local()
        .global(move |global| {
            if global.type_ != pw::types::ObjectType::Node {
                return;
            }
            let Some(props) = global.props else { return };
            let Some(class) = props.get("media.class") else {
                return;
            };
            let label = props
                .get("node.description")
                .or_else(|| props.get("application.process.binary"))
                .unwrap_or("unknown")
                .to_string();
            match class {
                "Stream/Output/Audio" => {
                    ud2.lock().unwrap().apps.push(AppAudio {
                        id: format!("pw:{}", global.id),
                        label,
                    });
                }
                // F-AU-03: real capture endpoints for the mic selector.
                "Audio/Source" | "Audio/Source/Virtual" => {
                    ud2.lock().unwrap().sources.push(DeviceInfo {
                        id: format!("pw:{}", global.id),
                        label,
                        is_default: false,
                    });
                }
                _ => {}
            }
        })
        .register();

    tl.start();
    std::thread::sleep(Duration::from_millis(500)); // ponytail: fixed probe window; sync-callback when it matters
                                                    // stop() blocks until the loop thread has exited: never wait() after it.
                                                    // Nobody will ever signal that waiter again, so waiting here parks the
                                                    // caller forever (this hung boot-time getAudioDevices under a live
                                                    // daemon; without a daemon the early return above masked it).
    tl.stop();
    // Same locked teardown as the capture threads: dropping proxies without
    // the loop lock trips "called from wrong context" warnings.
    {
        let _guard = tl.lock();
        drop(_listener);
        drop(registry);
        drop(core);
        drop(context);
    }

    let (apps, sources) = {
        let u = ud.lock().unwrap();
        (u.apps.clone(), u.sources.clone())
    };
    inputs.extend(sources);
    Ok(AudioDevices {
        inputs,
        outputs: Vec::new(),
        apps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Device probe must return promptly: it runs on the boot path
    /// (getAudioDevices gates the splash screen). Regression: a tl.wait()
    /// after tl.stop() parked forever — nothing signals a stopped loop —
    /// hanging startup whenever a live daemon made connect() succeed
    /// (without a daemon the early return masked it, so CI stayed green).
    /// The probe runs on a thread with a timeout so a regression fails the
    /// test instead of hanging the suite (process exit cleans the stray thread).
    #[test]
    fn probe_returns_promptly() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(list_audio_devices().map(|d| d.apps.len()));
        });
        match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(n)) => eprintln!("probe ok: {n} apps"),
            Ok(Err(e)) => panic!("probe failed: {e}"),
            Err(_) => panic!("probe hung: never wait() on a stopped loop"),
        }
    }
}
