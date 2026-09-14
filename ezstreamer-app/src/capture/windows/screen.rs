//! WGC screen/window capture (design.md §3.1.1).
//!
//! A dedicated thread owns the D3D11 device and the capture pool/session; the
//! FrameArrived handler (free-threaded) copies the surface to a staging
//! texture, scales to the profile size and pushes into the shared pump via
//! the [`VideoSource`] handle. Preview frames (5fps, 640x360 RGBA) are sent
//! straight to the UI (no GStreamer involved).

use super::{co_init, err};
use crate::events::UiSink;
use ezstreamer_core::config::{Profile, ScreenTarget, ScreenTargetKind};
use ezstreamer_core::video::{bgra_to_rgba, scale_bgra, VideoSource};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PREVIEW_W: u32 = 640;
const PREVIEW_H: u32 = 360;

pub struct ScreenCapture {
    /// Push-only handle to the session-owned pump (see `VideoSource`): a
    /// stopped or dropped capture must never stop frame pacing — live source
    /// switches reuse the pump across captures.
    pub source: VideoSource,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ScreenCapture {
    /// Stop the capture thread and join it. The video pump is owned by the
    /// backend session (`backend::SessionState::video_pump`), not by
    /// captures, so this never stops frame pacing.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn start_screen(
    ui: UiSink,
    target: &ScreenTarget,
    profile: &Profile,
    source: VideoSource,
    cursor: bool,
) -> super::Result<ScreenCapture> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let target = target.clone();
    let dst_w = profile.w;
    let dst_h = profile.h;
    let ui2 = ui.clone();

    let source_for_capture = source.clone();
    let handle = std::thread::Builder::new()
        .name("wgc-capture".into())
        .spawn(move || {
            if let Err(e) =
                run_capture(&ui, &target, dst_w, dst_h, cursor, &source_for_capture, stop2)
            {
                crate::logging::error(&format!("wgc capture: {e}"));
                ui2.error("capture", &e);
            }
        })
        .map_err(err)?;

    Ok(ScreenCapture {
        source,
        stop,
        handle: Some(handle),
    })
}

fn run_capture(
    ui: &UiSink,
    target: &ScreenTarget,
    dst_w: u32,
    dst_h: u32,
    cursor: bool,
    source: &VideoSource,
    stop: Arc<AtomicBool>,
) -> super::Result<()> {
    use windows::core::ComInterface;
    use windows::Foundation::TypedEventHandler;
    use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
    use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
        D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_SDK_VERSION,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows::Win32::System::WinRT::Direct3D11::{
        CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
    };
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

    co_init();

    // D3D11 device
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .map_err(err)?;
    }
    let device = device.ok_or_else(|| err("no D3D11 device"))?;
    let context = context.ok_or_else(|| err("no D3D11 context"))?;

    let dxgi = device.cast::<IDXGIDevice>().map_err(err)?;
    let insp = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.map_err(err)?;
    let d3ddevice: IDirect3DDevice = insp.cast().map_err(err)?;

    // capture item via interop
    let interop: IGraphicsCaptureItemInterop =
        windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .map_err(err)?;
    let item: GraphicsCaptureItem = match target.kind {
        ScreenTargetKind::Display => {
            let hmon = super::enumerate::monitor_by_index(
                target
                    .id
                    .rsplit(':')
                    .next()
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0),
            )?;
            unsafe { interop.CreateForMonitor(hmon) }.map_err(err)?
        }
        ScreenTargetKind::Window => {
            let raw: usize = target
                .id
                .rsplit(':')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let hwnd = windows::Win32::Foundation::HWND(raw as isize);
            unsafe { interop.CreateForWindow(hwnd) }.map_err(err)?
        }
    };
    let src_size = item.Size().map_err(err)?;
    let (_src_w, _src_h) = (src_size.Width.max(1) as u32, src_size.Height.max(1) as u32);

    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &d3ddevice,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        windows::Graphics::SizeInt32 {
            Width: src_size.Width,
            Height: src_size.Height,
        },
    )
    .map_err(err)?;
    let session = pool.CreateCaptureSession(&item).map_err(err)?;
    session.SetIsCursorCaptureEnabled(cursor).map_err(err)?;

    let source2 = source.clone();
    // The handler must observe the same stop flag as the capture loop;
    // this used to be a fresh never-set AtomicBool (dead check).
    let handler_stop = stop.clone();
    let preview_last = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
    let preview_last2 = preview_last.clone();
    let ui2 = ui.clone();

    let handler = TypedEventHandler::new(
        move |pool: &Option<Direct3D11CaptureFramePool>,
              _args: &Option<windows::core::IInspectable>| {
            let Some(pool) = pool else { return Ok(()) };
            let ctx = context.clone();
            loop {
                if handler_stop.load(Ordering::Relaxed) {
                    break;
                }
                let frame = match pool.TryGetNextFrame() {
                    Ok(f) => f,
                    Err(_) => break,
                };
                let size = match frame.ContentSize() {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let w = size.Width.max(1) as u32;
                let h = size.Height.max(1) as u32;
                let Ok(surface) = frame.Surface() else { break };
                let Ok(access) = surface.cast::<IDirect3DDxgiInterfaceAccess>() else {
                    break;
                };
                let Ok(tex) = (unsafe { access.GetInterface::<ID3D11Texture2D>() }) else {
                    break;
                };

                let mut desc = Default::default();
                unsafe { tex.GetDesc(&mut desc) };
                let staging_desc = D3D11_TEXTURE2D_DESC {
                    Width: desc.Width,
                    Height: desc.Height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: desc.Format,
                    SampleDesc: desc.SampleDesc,
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: windows::Win32::Graphics::Direct3D11::D3D11_BIND_FLAG(0).0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };
                unsafe {
                    let mut staging: Option<ID3D11Texture2D> = None;
                    if device
                        .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                        .is_err()
                    {
                        break;
                    }
                    let Some(staging) = staging else { break };
                    ctx.CopyResource(&staging, &tex);
                    let mut mapped = Default::default();
                    if ctx
                        .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                        .is_err()
                    {
                        break;
                    }
                    let width = desc.Width as usize;
                    let height = desc.Height as usize;
                    let row_pitch = mapped.RowPitch as usize;
                    let mut buf = vec![0u8; width * height * 4];
                    let src = mapped.pData as *const u8;
                    for row in 0..height {
                        std::ptr::copy_nonoverlapping(
                            src.add(row * row_pitch),
                            buf.as_mut_ptr().add(row * width * 4),
                            width * 4,
                        );
                    }
                    ctx.Unmap(&staging, 0);

                    source2.push(scale_bgra(&buf, w, h, dst_w, dst_h));

                    // 5fps preview (F-SC-03): raw RGBA straight to the UI.
                    let mut last = preview_last2.lock().unwrap();
                    if last.elapsed() >= crate::capture::PREVIEW_INTERVAL {
                        *last = Instant::now();
                        let small = scale_bgra(&buf, w, h, PREVIEW_W, PREVIEW_H);
                        let rgba = bgra_to_rgba(&small);
                        ui2.preview(rgba, PREVIEW_W, PREVIEW_H);
                    }
                }
            }
            Ok(())
        },
    );
    pool.FrameArrived(&handler).map_err(err)?;
    session.StartCapture().map_err(err)?;

    // keep objects alive on this thread; close them on stop
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
    {
        let _ = session.Close();
        let _ = pool.Close();
    }
    Ok(())
}
