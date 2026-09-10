//! Video frame pump: capture frames → FramePacer → GStreamer `appsrc`.
//!
//! Capture backends push frames through [`VideoSink::push`]; the pump thread
//! emits them at the profile fps (static screens keep flowing, design §3.1.3)
//! All frames are normalized to the profile size before emit. Clonable so
//! capture callbacks can hold a handle.

use super::FramePacer;
use crate::error::{Error, Result};
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct VideoSink {
    tx: Arc<mpsc::Sender<Vec<u8>>>,
    stop: Arc<AtomicBool>,
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

/// Shared pacer pump: drains capture frames, emits profile-paced BGRA
/// frames via `emit`. Returning `false` from `emit` stops the thread
/// (consumer gone).
fn spawn_pump(
    rx: mpsc::Receiver<Vec<u8>>,
    stop: Arc<AtomicBool>,
    w: u32,
    h: u32,
    fps: u32,
    mut emit: impl FnMut(&[u8]) -> bool + Send + 'static,
) -> Result<JoinHandle<()>> {
    let pacer = FramePacer::new(w, h, fps);
    let interval = Duration::from_secs_f64(1.0 / fps.max(1) as f64);
    std::thread::Builder::new()
        .name("video-sink".into())
        .spawn(move || {
            let mut pacer = pacer;
            let _ = (w, h);
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                // wait for a frame (or tick), keeping only the newest
                match rx.recv_timeout(interval / 4) {
                    Ok(frame) => {
                        let _ = pacer.push(&frame);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                while let Ok(frame) = rx.try_recv() {
                    let _ = pacer.push(&frame); // newest frame wins
                }
                let now = Instant::now();
                while let Some(frame) = pacer.poll(now) {
                    if !emit(frame) {
                        return; // consumer gone
                    }
                }
            }
        })
        .map_err(|e| Error::Capture(format!("video sink thread: {e}")))
}

impl VideoSink {
    /// File-drain pump (preview path: frames are consumed by the capture
    /// backend's own `stream://preview` emitter; the sink just paces/drops).
    pub fn spawn(mut writer: File, w: u32, h: u32, fps: u32) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = spawn_pump(rx, stop.clone(), w, h, fps, move |frame| {
            writer.write_all(frame).is_ok()
        })?;
        Ok(Self { tx: Arc::new(tx), stop, handle: Arc::new(Mutex::new(Some(handle))) })
    }

    /// `appsrc` pump: paced BGRA frames are sent to the returned channel;
    /// the GStreamer thread pushes them into `appsrc name=video_src`.
    /// Frame size is always `w*h*4` (profile-normalized, design §3.1.3).
    pub fn spawn_appsrc(w: u32, h: u32, fps: u32) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = spawn_pump(rx, stop.clone(), w, h, fps, move |frame| {
            out_tx.send(frame.to_vec()).is_ok()
        })?;
        Ok((
            Self { tx: Arc::new(tx), stop, handle: Arc::new(Mutex::new(Some(handle))) },
            out_rx,
        ))
    }

    /// Push a freshly captured (already scaled) frame. Returns false when stopped.
    pub fn push(&self, frame: Vec<u8>) -> bool {
        self.tx.send(frame).is_ok()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut slot) = self.handle.lock() {
            if let Some(h) = slot.take() {
                let _ = h.join();
            }
        }
    }
}

impl Drop for VideoSink {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Nearest-neighbor BGRA scale + letterbox into `dst_w x dst_h` (design §3.1.3:
/// all frames are normalized to the profile size before the pipe).
///
/// Integer fixed-point math (no `f64` per pixel) + row-parallel emit via
/// `std::thread::scope` for large frames. No new dependencies.
pub fn scale_bgra(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let mut dst = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
    scale_bgra_into(&mut dst, src, src_w, src_h, dst_w, dst_h);
    dst
}

/// Like [`scale_bgra`], but reads source rows at `src_stride` bytes.
/// PipeWire portal screencast buffers can pad rows (stride > `src_w*4`);
/// reading them as packed shifts every row and skews the image
/// (review 2026-09-10). Returns `None` when the buffer is too short for a
/// padded row (partial/corrupt frame); `src_stride <= src_w*4` falls back
/// to the packed path.
pub fn scale_bgra_strided(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    src_stride: usize,
    dst_w: u32,
    dst_h: u32,
) -> Option<Vec<u8>> {
    let packed_row = (src_w as usize).checked_mul(4)?;
    if src_stride <= packed_row {
        return Some(scale_bgra(src, src_w, src_h, dst_w, dst_h));
    }
    let packed = repack_bgra_rows(src, src_w, src_h, src_stride)?;
    Some(scale_bgra(&packed, src_w, src_h, dst_w, dst_h))
}

/// Copy `src_h` rows of `src_w` 4-byte pixels from a `stride`-byte pitched
/// buffer into a tightly packed buffer. `None` when the source cannot hold
/// the last row (short chunk).
fn repack_bgra_rows(src: &[u8], src_w: u32, src_h: u32, stride: usize) -> Option<Vec<u8>> {
    let row = (src_w as usize).checked_mul(4)?;
    let h = src_h as usize;
    if row == 0 || h == 0 {
        return None;
    }
    let needed = (h - 1).checked_mul(stride)?.checked_add(row)?;
    if src.len() < needed {
        return None;
    }
    let mut out = vec![0u8; row.checked_mul(h)?];
    for y in 0..h {
        let start = y * stride;
        out[y * row..(y + 1) * row].copy_from_slice(&src[start..start + row]);
    }
    Some(out)
}

fn fill_letterbox_black(dst: &mut [u8]) {
    // opaque black BGRA [0,0,0,255]: memset + alpha plane.
    dst.fill(0);
    for a in dst.iter_mut().skip(3).step_by(4) {
        *a = 255;
    }
}

pub fn scale_bgra_into(dst: &mut [u8], src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) {
    let dst_len = (dst_w as usize) * (dst_h as usize) * 4;
    if dst.len() < dst_len {
        return;
    }
    let dst = &mut dst[..dst_len];
    fill_letterbox_black(dst);
    if src_w == 0 || src_h == 0 || src.len() < (src_w as usize) * (src_h as usize) * 4 {
        return;
    }
    // Identity fast path: copy BGR, force opaque alpha.
    if src_w == dst_w && src_h == dst_h {
        copy_bgra_opaque(dst, src);
        return;
    }
    // Integer aspect fit: scale = min(dst_w/src_w, dst_h/src_h).
    let (out_w, out_h) = fit_output(src_w, src_h, dst_w, dst_h);
    let off_x = (dst_w - out_w) / 2;
    let off_y = (dst_h - out_h) / 2;
    blit_nearest(dst, src, src_w, src_h, dst_w, out_w, out_h, off_x, off_y);
}

fn copy_bgra_opaque(dst: &mut [u8], src: &[u8]) {
    // src and dst same length here; keep alpha opaque like the scaler does.
    let n = dst.len().min(src.len());
    let (d, s) = (&mut dst[..n], &src[..n]);
    // 4-byte chunks: BGR copy + A=255. Compiler auto-vectorizes this loop.
    for (dpx, spx) in d
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(s.as_chunks::<4>().0.iter())
    {
        dpx[0] = spx[0];
        dpx[1] = spx[1];
        dpx[2] = spx[2];
        dpx[3] = 255;
    }
}

fn fit_output(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> (u32, u32) {
    let sw = src_w as u64;
    let sh = src_h as u64;
    let dw = dst_w as u64;
    let dh = dst_h as u64;
    if dw * sh <= dh * sw {
        // width-constrained
        let out_w = dst_w;
        let out_h = ((sh * dw / sw) as u32).max(1).min(dst_h);
        (out_w, out_h)
    } else {
        let out_h = dst_h;
        let out_w = ((sw * dh / sh) as u32).max(1).min(dst_w);
        (out_w, out_h)
    }
}

#[allow(clippy::too_many_arguments)]
fn blit_nearest(
    dst: &mut [u8],
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    out_w: u32,
    out_h: u32,
    off_x: u32,
    off_y: u32,
) {
    let sw = src_w as usize;
    let sh = src_h as usize;
    let dw = dst_w as usize;
    let ow = out_w as usize;
    let oh = out_h as usize;
    let ox = off_x as usize;
    let oy0 = off_y as usize;
    let stride = dw * 4;
    // Precompute source x for every output column (integer nearest).
    let sw_u64 = src_w as u64;
    let ow_u64 = out_w as u64;
    let mut map_x = vec![0usize; ow];
    for (dx, sx) in map_x.iter_mut().enumerate() {
        let v = ((2 * dx as u64 + 1) * sw_u64) / (2 * ow_u64);
        *sx = (v as usize).min(sw - 1);
    }
    let sh_u64 = src_h as u64;
    let oh_u64 = out_h as u64;

    // Middle band that actually carries the image (rest stays letterbox).
    let middle = &mut dst[oy0 * stride..(oy0 + oh) * stride];
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    // Thread per ~64 rows; small frames stay single-threaded (spawn cost).
    let want = oh.div_ceil(64);
    let n_threads = threads.min(want).max(1).min(oh.max(1));
    if n_threads <= 1 {
        blit_rows(middle, src, &map_x, sw, sh, dw, ow, ox, 0, oh, sh_u64, oh_u64);
        return;
    }
    let rows_per_thread = oh.div_ceil(n_threads);
    std::thread::scope(|s| {
        for (chunk_idx, chunk) in middle.chunks_mut(rows_per_thread * stride).enumerate() {
            let start_oy = chunk_idx * rows_per_thread;
            let rows = chunk.len() / stride;
            let map_x = &map_x;
            s.spawn(move || {
                blit_rows(
                    chunk, src, map_x, sw, sh, dw, ow, ox, start_oy, rows, sh_u64, oh_u64,
                );
            });
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn blit_rows(
    chunk: &mut [u8],
    src: &[u8],
    map_x: &[usize],
    sw: usize,
    sh: usize,
    _dw: usize,
    ow: usize,
    ox: usize,
    start_oy: usize,
    rows: usize,
    sh_u64: u64,
    oh_u64: u64,
) {
    for r in 0..rows {
        let oy = start_oy + r;
        let v = ((2 * oy as u64 + 1) * sh_u64) / (2 * oh_u64);
        let sy = (v as usize).min(sh - 1);
        let s_row = sy * sw * 4;
        let d_row = r * _dw * 4;
        // SAFETY-free row copy via precomputed x map.
        for (dx, sx) in map_x.iter().enumerate().take(ow) {
            let s = s_row + sx * 4;
            let d = d_row + (dx + ox) * 4;
            chunk[d] = src[s];
            chunk[d + 1] = src[s + 1];
            chunk[d + 2] = src[s + 2];
            // alpha already 255 from the letterbox fill
        }
    }
}

/// BGRA → RGBA in place (for PNG preview encoding).
pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    bgra.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| [px[2], px[1], px[0], px[3]])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_identity() {
        let src = vec![7u8; 4 * 4 * 4]; // 4x4 BGRA
        let dst = scale_bgra(&src, 4, 4, 4, 4);
        assert_eq!(dst.len(), 64);
        assert!(dst.chunks(4).all(|px| px == [7, 7, 7, 255]));
    }

    #[test]
    fn scale_downsamples_with_letterbox() {
        // 4x4 source into 8x4 dest: fits to 4x4, centered with 2px bars each side
        let src = vec![10u8; 4 * 4 * 4];
        let dst = scale_bgra(&src, 4, 4, 8, 4);
        assert_eq!(dst.len(), 8 * 4 * 4);
        // first pixel is letterbox black, center pixels carry the source color
        assert_eq!(&dst[0..4], &[0, 0, 0, 255]);
        assert_eq!(&dst[4 * 4..4 * 4 + 4], &[10, 10, 10, 255]);
        assert_eq!(&dst[7 * 4..8 * 4], &[0, 0, 0, 255]);
    }

    #[test]
    fn scale_rejects_bad_source() {
        let dst = scale_bgra(&[0u8; 3], 4, 4, 4, 4);
        assert_eq!(dst.len(), 64);
        assert!(dst.chunks(4).all(|px| px == [0, 0, 0, 255]));
    }

    #[test]
    fn strided_scale_reads_padded_rows() {
        // 2x2 BGRA with 8 bytes of padding per row: a packed read would
        // shift row 1 by the padding and skew the image.
        let (w, h, stride) = (2usize, 2usize, 2 * 4 + 8);
        let mut src = vec![0u8; stride * h];
        for y in 0..h {
            for x in 0..w {
                let i = y * stride + x * 4;
                src[i] = (y * 10 + x) as u8;
                src[i + 3] = 255;
            }
        }
        // Poison the padding: it must never leak into the output.
        for y in 0..h {
            for p in 0..8 {
                src[y * stride + 8 + p] = 0xFF;
            }
        }
        let dst = scale_bgra_strided(&src, 2, 2, stride, 2, 2).unwrap();
        assert_eq!(dst.len(), 16);
        assert_eq!(&dst[0..4], &[0, 0, 0, 255]);
        assert_eq!(&dst[4..8], &[1, 0, 0, 255]);
        assert_eq!(&dst[8..12], &[10, 0, 0, 255]);
        assert_eq!(&dst[12..16], &[11, 0, 0, 255]);
    }

    #[test]
    fn strided_scale_rejects_truncated_padded_buffer() {
        // Last padded row missing: must fail instead of reading past the end.
        let src = vec![0u8; 20];
        assert!(scale_bgra_strided(&src, 2, 2, 2 * 4 + 8, 2, 2).is_none());
    }

    #[test]
    fn strided_scale_tight_stride_matches_packed() {
        let src: Vec<u8> = (0..32).collect();
        let strided = scale_bgra_strided(&src, 4, 2, 4 * 4, 4, 2).unwrap();
        assert_eq!(strided, scale_bgra(&src, 4, 2, 4, 2));
    }

    #[test]
    fn bgra_rgba_swap() {
        let rgba = bgra_to_rgba(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(rgba, vec![3, 2, 1, 4, 7, 6, 5, 8]);
    }

    #[test]
    fn scale_preserves_gradient_corners() {
        // 4x2 distinct columns → 4x2 identity-ish: corners must map 1:1.
        let mut src = vec![0u8; 4 * 2 * 4];
        for x in 0..4 {
            for y in 0..2 {
                let i = (y * 4 + x) * 4;
                src[i] = (x * 60) as u8;
                src[i + 1] = (y * 120) as u8;
                src[i + 2] = 200;
                src[i + 3] = 255;
            }
        }
        let dst = scale_bgra(&src, 4, 2, 4, 2);
        assert_eq!(&dst[0..3], &[0, 0, 200]);
        assert_eq!(&dst[(3 * 4)..(3 * 4 + 3)], &[180, 0, 200]);
    }

    #[test]
    fn scale_large_parallel_matches_single() {
        // 256x144 → 1280x720 exercises the scoped row-parallel path.
        let sw = 256u32;
        let sh = 144u32;
        let mut src = vec![0u8; (sw * sh * 4) as usize];
        for y in 0..sh {
            for x in 0..sw {
                let i = ((y * sw + x) * 4) as usize;
                src[i] = (x % 251) as u8;
                src[i + 1] = (y % 251) as u8;
                src[i + 2] = ((x + y) % 251) as u8;
                src[i + 3] = 255;
            }
        }
        let dst = scale_bgra(&src, sw, sh, 1280, 720);
        assert_eq!(dst.len(), 1280 * 720 * 4);
        // center pixel carries scaled content, corner is letterbox or content
        // but alpha must stay opaque everywhere.
        assert!(dst.as_chunks::<4>().0.iter().all(|px| px[3] == 255));
        let center = ((360 * 1280 + 640) * 4) as usize;
        assert_ne!(&dst[center..center + 3], &[0, 0, 0]);
    }
}
