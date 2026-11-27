//! Custom GPUI audio canvas rendering cached spectrogram tiles, downsampled waveform peaks,
//! bounded selection overlays, and lightweight playhead cursors.
//!
//! Performance invariant:
//! - Playhead movement only repaints the lightweight cursor overlay and NEVER triggers FFT
//!   recomputation or tile regeneration.
//! - Spectrogram GPU `RenderImage` handles are cached by `SpectrogramTileKey` in a small
//!   bounded LRU map matching the worker cache.

use gpui::{
    Bounds, Corners, IntoElement, Pixels, RenderImage, Window, canvas, point, px, quad, rgb, rgba,
    size,
};
use image::{Frame, RgbaImage};
use smallvec::smallvec;
use spectracraft_mono::visualization::{Peaks, SpectrogramTile, SpectrogramTileKey};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Lightweight snapshot of audio visualization data passed into the GPUI `canvas` paint closure.
#[derive(Clone)]
pub struct AudioCanvasData {
    pub peaks: Option<Arc<Peaks>>,
    pub tile_image: Option<Arc<RenderImage>>,
    pub show_waveform: bool,
    pub show_spectrogram: bool,
    pub duration_secs: f64,
    pub view_range: [f64; 2],
    pub playhead_secs: f64,
    pub selection: Option<[f64; 2]>,
    pub accent_hex: u32,
}

/// Bounded GPU `RenderImage` cache keyed by `SpectrogramTileKey` so tile RGBA->BGRA conversion
/// and `RenderImage` creation happen once per cached tile rather than every frame.
pub struct TileTextureCache {
    max_entries: usize,
    entries: HashMap<SpectrogramTileKey, Arc<RenderImage>>,
    order: VecDeque<SpectrogramTileKey>,
}

impl TileTextureCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(4),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    pub fn get_or_build(&mut self, tile: &SpectrogramTile) -> Option<Arc<RenderImage>> {
        if let Some(existing) = self.entries.get(&tile.key) {
            return Some(existing.clone());
        }
        let img = build_render_image_from_tile(tile)?;
        self.entries.insert(tile.key, img.clone());
        self.order.push_back(tile.key);
        while self.entries.len() > self.max_entries {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
        Some(img)
    }
}

/// Convert a `SpectrogramTile`'s RGBA8 pixels into a GPUI `RenderImage`.
/// GPUI's `RenderImage` expects BGRA byte order inside `image::RgbaImage`.
pub fn build_render_image_from_tile(tile: &SpectrogramTile) -> Option<Arc<RenderImage>> {
    let w = tile.key.width;
    let h = tile.key.height;
    let expected = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
    if tile.rgba.len() != expected {
        return None;
    }
    let mut bgra = tile.rgba.to_vec();
    for px_bytes in bgra.as_chunks_mut::<4>().0 {
        px_bytes.swap(0, 2); // RGBA -> BGRA for GPUI RenderImage
    }
    let rgba_img = RgbaImage::from_raw(w, h, bgra)?;
    let frame = Frame::new(rgba_img);
    Some(Arc::new(RenderImage::new(smallvec![frame])))
}

/// Convert a horizontal coordinate inside `[origin_x, origin_x + width]` into seconds within `view_range`.
pub fn x_to_time_secs(x: f32, origin_x: f32, width: f32, view_range: [f64; 2]) -> f64 {
    if width <= 1.0 {
        return view_range[0];
    }
    let frac = f64::from(((x - origin_x) / width).clamp(0.0, 1.0));
    view_range[0] + frac * (view_range[1] - view_range[0]).max(0.0)
}

/// Convert a timestamp in seconds into a normalized `[0.0, 1.0]` fraction inside `view_range`.
pub fn time_secs_to_frac(secs: f64, view_range: [f64; 2]) -> Option<f32> {
    let span = view_range[1] - view_range[0];
    if span <= 1e-9 || secs < view_range[0] || secs > view_range[1] {
        return None;
    }
    Some(((secs - view_range[0]) / span).clamp(0.0, 1.0) as f32)
}

/// Paint the custom GPUI audio canvas (spectrogram tile + waveform envelope + selection + playhead).
pub fn audio_canvas_element(
    data: AudioCanvasData,
    on_bounds: impl Fn(Bounds<Pixels>) + 'static,
) -> impl IntoElement {
    canvas(
        move |bounds, _window, _cx| {
            on_bounds(bounds);
            bounds
        },
        move |bounds, _prepaint, window: &mut Window, _cx| {
            let bg = rgb(0x121216);
            let border = rgb(0x3a3a44);
            window.paint_quad(quad(
                bounds,
                px(4.0),
                bg,
                px(1.0),
                border,
                Default::default(),
            ));

            let pad = 6.0_f32;
            let inner_w = (f32::from(bounds.size.width) - pad * 2.0).max(1.0);
            let inner_h = (f32::from(bounds.size.height) - pad * 2.0).max(1.0);
            let inner_x = f32::from(bounds.origin.x) + pad;
            let inner_y = f32::from(bounds.origin.y) + pad;

            let both = data.show_waveform && data.show_spectrogram;
            let (wave_y, wave_h, spec_y, spec_h) = if both {
                let wh = (inner_h * 0.36).max(40.0);
                let gap = 8.0_f32;
                let sh = (inner_h - wh - gap).max(40.0);
                (inner_y, wh, inner_y + wh + gap, sh)
            } else if data.show_waveform {
                (inner_y, inner_h, inner_y, 0.0)
            } else {
                (inner_y, 0.0, inner_y, inner_h)
            };

            // 1. Waveform peak envelope bars (pure cached Peaks slice; zero DSP/FFT).
            if data.show_waveform && wave_h > 4.0 {
                let wf_bounds = Bounds {
                    origin: point(px(inner_x), px(wave_y)),
                    size: size(px(inner_w), px(wave_h)),
                };
                window.paint_quad(quad(
                    wf_bounds,
                    px(3.0),
                    rgb(0x17181f),
                    px(1.0),
                    rgb(0x2a2b34),
                    Default::default(),
                ));

                // Center 0-amplitude hairline
                let mid_y = wave_y + wave_h * 0.5;
                window.paint_quad(quad(
                    Bounds {
                        origin: point(px(inner_x), px(mid_y)),
                        size: size(px(inner_w), px(1.0)),
                    },
                    px(0.0),
                    rgb(0x2f313d),
                    px(0.0),
                    gpui::transparent_black(),
                    Default::default(),
                ));

                if let Some(peaks) = &data.peaks
                    && !peaks.is_empty()
                    && data.duration_secs > 1e-6
                {
                    let total = peaks.len();
                    let v0 = (data.view_range[0] / data.duration_secs).clamp(0.0, 1.0);
                    let v1 = (data.view_range[1] / data.duration_secs).clamp(v0, 1.0);
                    let i0 = ((v0 * total as f64).floor() as usize).min(total.saturating_sub(1));
                    let i1 = ((v1 * total as f64).ceil() as usize).clamp(i0 + 1, total);
                    let visible = &peaks[i0..i1];
                    let bars = (inner_w as usize / 2).clamp(1, visible.len().min(600));
                    let bar_w = (inner_w / bars as f32).max(1.0);
                    let half_h = wave_h * 0.44;
                    let wave_color = rgb(data.accent_hex);

                    for b in 0..bars {
                        let a = b * visible.len() / bars;
                        let e = ((b + 1) * visible.len() / bars)
                            .max(a + 1)
                            .min(visible.len());
                        let (lo, hi) = visible[a..e]
                            .iter()
                            .fold((0.0_f32, 0.0_f32), |(acc_lo, acc_hi), &(l, h)| {
                                (acc_lo.min(l), acc_hi.max(h))
                            });
                        let y_top = mid_y - hi.clamp(-1.0, 1.0) * half_h;
                        let y_bot = mid_y - lo.clamp(-1.0, 1.0) * half_h;
                        let h_px = (y_bot - y_top).max(1.5);
                        let x_px = inner_x + b as f32 * bar_w;
                        window.paint_quad(quad(
                            Bounds {
                                origin: point(px(x_px), px(y_top)),
                                size: size(px((bar_w - 0.5).max(1.0)), px(h_px)),
                            },
                            px(0.0),
                            wave_color,
                            px(0.0),
                            gpui::transparent_black(),
                            Default::default(),
                        ));
                    }
                }
            }

            // 2. Cached Spectrogram Tile texture (single GPU texture blit; zero FFTs on redraw).
            if data.show_spectrogram && spec_h > 4.0 {
                let sp_bounds = Bounds {
                    origin: point(px(inner_x), px(spec_y)),
                    size: size(px(inner_w), px(spec_h)),
                };
                window.paint_quad(quad(
                    sp_bounds,
                    px(3.0),
                    rgb(0x0f1420),
                    px(1.0),
                    rgb(0x2a2b34),
                    Default::default(),
                ));
                if let Some(render_img) = data.tile_image.clone() {
                    let _ = window.paint_image(
                        sp_bounds,
                        sp_bounds,
                        Corners::default(),
                        render_img,
                        0,
                        false,
                    );
                }
            }

            // 3. Stage 2 Selection Overlay (bounded region highlight).
            if let Some([s0, s1]) = data.selection {
                let span = (data.view_range[1] - data.view_range[0]).max(1e-6);
                let f0 = ((s0 - data.view_range[0]) / span).clamp(0.0, 1.0) as f32;
                let f1 = ((s1 - data.view_range[0]) / span).clamp(0.0, 1.0) as f32;
                if f1 > f0 {
                    let sel_x = inner_x + f0 * inner_w;
                    let sel_w = ((f1 - f0) * inner_w).max(2.0);
                    window.paint_quad(quad(
                        Bounds {
                            origin: point(px(sel_x), px(inner_y)),
                            size: size(px(sel_w), px(inner_h)),
                        },
                        px(2.0),
                        rgba(0x378ef038),
                        px(1.0),
                        rgb(0x378ef0),
                        Default::default(),
                    ));
                }
            }

            // 4. Lightweight Playhead Line (authoritative clock overlay).
            if let Some(frac) = time_secs_to_frac(data.playhead_secs, data.view_range) {
                let px_x = inner_x + frac * inner_w;
                window.paint_quad(quad(
                    Bounds {
                        origin: point(px(px_x), px(inner_y)),
                        size: size(px(2.0), px(inner_h)),
                    },
                    px(0.0),
                    rgb(0xf59e0b),
                    px(0.0),
                    gpui::transparent_black(),
                    Default::default(),
                ));
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinate_mapping_roundtrips_cleanly() {
        let range = [1.0, 5.0];
        let t = x_to_time_secs(150.0, 50.0, 200.0, range);
        assert!((t - 3.0).abs() < 1e-6);
        let frac = time_secs_to_frac(3.0, range).unwrap();
        assert!((frac - 0.5).abs() < 1e-6);
        assert!(time_secs_to_frac(0.5, range).is_none());
        assert!(time_secs_to_frac(5.5, range).is_none());
    }
}
