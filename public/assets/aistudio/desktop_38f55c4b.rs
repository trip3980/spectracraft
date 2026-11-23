use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use rodio::{buffer::SamplesBuffer, OutputStream, Sink};
use rustfft::{num_complex::Complex32, FftPlanner};
use spectral_canvas_mono::capture::{self, Material, Mic, Recording, Room, Setup};
use spectral_canvas_mono::{analyze_with_texture, synthesize, write_wav};
use spectral_canvas_mono::{edge, workspace};
use std::{
    path::Path,
    sync::{mpsc, Arc},
    time::Duration,
};

#[derive(Clone, Copy, PartialEq)]
enum Take {
    Original,
    Regenerated,
}
impl Take {
    fn name(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Regenerated => "Regenerated",
        }
    }
}
type Peaks = Vec<(f32, f32)>;
fn measure_peaks(wave: &[f32]) -> Peaks {
    let n = wave.len().min(2048);
    (0..n)
        .map(|i| {
            wave[i * wave.len() / n..(i + 1) * wave.len() / n]
                .iter()
                .fold((0_f32, 0_f32), |(lo, hi), &v| (lo.min(v), hi.max(v)))
        })
        .collect()
}
const SPECTRUM_H: usize = 144;
const SPECTRUM_BOTTOM: f32 = -90.0;
const SPECTRUM_TOP: f32 = 0.0;

fn to_dbfs(amplitude: f32) -> f32 {
    20. * amplitude.max(1e-10).log10()
}

struct Spectrum {
    width: usize,
    max_hz: f32,
    dbfs: Vec<f32>,
}
impl Spectrum {
    fn measure(wave: &[f32], rate: u32) -> Self {
        let width = wave
            .len()
            .div_ceil((rate as usize / 100).max(1))
            .clamp(1, 700);
        let n = (rate as usize / 24).next_power_of_two();
        let max_hz = 12_000_f32.min(rate as f32 * 0.5);
        let window: Vec<_> = (0..n)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
            .collect();
        let norm = 2. / window.iter().sum::<f32>();
        let hz_to_bin = n as f32 / rate as f32;
        let bands: Vec<_> = (0..SPECTRUM_H)
            .map(|y| {
                let lo = ((SPECTRUM_H - 1 - y) as f32 / SPECTRUM_H as f32 * max_hz * hz_to_bin)
                    .ceil() as usize;
                let hi = ((SPECTRUM_H - y) as f32 / SPECTRUM_H as f32 * max_hz * hz_to_bin).ceil()
                    as usize;
                let start = lo.min(n / 2);
                start..hi.max(start + 1).min(n / 2 + 1)
            })
            .collect();
        let mut dbfs = vec![SPECTRUM_BOTTOM; width * SPECTRUM_H];
        let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
        let mut block = vec![Complex32::default(); n];
        let mut scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
        for col in 0..width {
            let center = ((2 * col + 1) * wave.len()) / (2 * width);
            for (k, v) in block.iter_mut().enumerate() {
                let x = (center + k)
                    .checked_sub(n / 2)
                    .and_then(|i| wave.get(i))
                    .copied()
                    .unwrap_or(0.0);
                *v = Complex32::new(window[k] * x, 0.0);
            }
            fft.process_with_scratch(&mut block, &mut scratch);
            for (y, band) in bands.iter().enumerate() {
                let peak = band
                    .clone()
                    .map(|i| {
                        let endpoint_scale = if i == 0 || i == n / 2 { 0.5 } else { 1. };
                        block[i].norm() * endpoint_scale
                    })
                    .fold(0_f32, f32::max);
                dbfs[y * width + col] = to_dbfs(peak * norm);
            }
        }
        Self {
            width,
            max_hz,
            dbfs,
        }
    }
    fn color(dbfs: f32) -> Color32 {
        let colors = [
            [15., 20., 32.],
            [28., 66., 115.],
            [21., 136., 154.],
            [202., 172., 76.],
            [255., 248., 213.],
        ];
        let a = ((dbfs.clamp(SPECTRUM_BOTTOM, SPECTRUM_TOP) - SPECTRUM_BOTTOM)
            / (SPECTRUM_TOP - SPECTRUM_BOTTOM))
            * 4.;
        let lo = (a.floor() as usize).min(3);
        let f = a - lo as f32;
        let c = |i| ((1. - f) * colors[lo][i] + f * colors[lo + 1][i]) as u8;
        Color32::from_rgb(c(0), c(1), c(2))
    }
    fn image(&self) -> egui::ColorImage {
        egui::ColorImage {
            size: [self.width, SPECTRUM_H],
            pixels: self.dbfs.iter().copied().map(Self::color).collect(),
        }
    }
}

struct Visual {
    peaks: Peaks,
    spectrum: Spectrum,
}
impl Visual {
    fn measure(wave: &[f32], rate: u32) -> Self {
        Self {
            peaks: measure_peaks(wave),
            spectrum: Spectrum::measure(wave, rate),
        }
    }
    fn peak_dbfs(&self) -> String {
        let peak = self
            .peaks
            .iter()
            .fold(0_f32, |v, (lo, hi)| v.max(lo.abs()).max(hi.abs()));
        if peak == 0. {
            "silence".to_owned()
        } else {
            format!("{:+.1} dBFS peak", to_dbfs(peak))
        }
    }
}

struct LayerSession {
    packet: edge::Packet,
    rendered: workspace::Rendered,
    controls: workspace::Controls,
    headroom: f32,
    view: Option<usize>,
}
struct Track {
    original_view: Visual,
    regenerated_view: Option<Visual>,
    name: String,
    rate: u32,
    original: Arc<[f32]>,
    rebuilt: Option<Vec<f32>>,
    dry: Option<Arc<[f32]>>,
    recording: Recording,
    layers: Option<LayerSession>,
}
impl Track {
    fn duration(&self) -> f64 {
        self.original.len() as f64 / self.rate as f64
    }
    fn audition(&self, which: Take) -> Option<Vec<f32>> {
        if let Some(layers) = &self.layers {
            let samples = if which == Take::Original {
                &self.original[..]
            } else {
                self.rebuilt.as_deref()?
            };
            return Some(samples.iter().map(|v| v * layers.headroom).collect());
        }
        let Some(other) = self.rebuilt.as_deref() else {
            return (which == Take::Original).then(|| {
                let peak = self.original.iter().fold(0_f32, |p, x| p.max(x.abs()));
                self.original
                    .iter()
                    .map(|x| x * (0.89 / peak.max(0.89)))
                    .collect()
            });
        };
        let rms = |s: &[f32]| {
            (s.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>() / s.len() as f64).sqrt()
        };
        let gain = (rms(&self.original) / rms(other).max(1e-10)).clamp(0.125, 8.) as f32;
        let peak = |s: &[f32]| s.iter().fold(0_f32, |p, x| p.max(x.abs()));
        let master = 0.89 / 0.89_f32.max(peak(&self.original)).max(peak(other) * gain);
        let (x, g) = if which == Take::Original {
            (&self.original[..], master)
        } else {
            (other, master * gain)
        };
        Some(x.iter().map(|v| v * g).collect())
    }
}

struct Playback {
    _stream: OutputStream,
    sink: Sink,
    started_at: f64,
}
enum Finished {
    Layers(Result<LayerSession, String>),
    Load(Result<Track, String>),
    Render(Result<(Vec<f32>, Visual), String>),
    Capture(Setup, Vec<f32>, Visual),
}
struct Studio {
    track: Option<Track>,
    job: Option<mpsc::Receiver<Finished>>,
    play: Option<Playback>,
    take: Take,
    cursor: f64,
    status: String,
    smoke: Option<std::path::PathBuf>,
    spectrum_mode: bool,
    include_texture: bool,
    setup: Setup,
    applied_setup: Option<Setup>,
    original_texture: Option<egui::TextureHandle>,
    regenerated_texture: Option<egui::TextureHandle>,
}

impl Studio {
    fn new() -> Self {
        Self {
            track: None,
            job: None,
            play: None,
            take: Take::Original,
            cursor: 0.,
            status: "Open a mono WAV to begin.".into(),
            smoke: None,
            spectrum_mode: true,
            include_texture: false,
            setup: Setup::default(),
            applied_setup: None,
            original_texture: None,
            regenerated_texture: None,
        }
    }
    fn stop(&mut self) {
        if let Some(p) = self.play.take() {
            self.cursor = if p.sink.empty() {
                0.
            } else {
                p.started_at + p.sink.get_pos().as_secs_f64()
            };
            p.sink.stop();
        }
    }
    fn start(&mut self) {
        let Some(t) = &self.track else {
            return;
        };
        let Some(mut wave) = t.audition(self.take) else {
            return;
        };
        let start = (self.cursor * t.rate as f64)
            .round()
            .clamp(0., wave.len() as f64) as usize;
        let start = if start == wave.len() { 0 } else { start };
        wave.drain(..start);
        match OutputStream::try_default().and_then(|(stream, handle)| {
            Sink::try_new(&handle)
                .map(|s| (stream, s))
                .map_err(|_| rodio::StreamError::NoDevice)
        }) {
            Ok((stream, sink)) => {
                sink.append(SamplesBuffer::new(1, t.rate, wave));
                self.play = Some(Playback {
                    _stream: stream,
                    sink,
                    started_at: start as f64 / t.rate as f64,
                });
            }
            Err(e) => self.status = format!("Playback unavailable: {e}"),
        }
    }
    fn switch(&mut self, take: Take) {
        if take == self.take {
            return;
        }
        let playing = self.play.is_some();
        self.stop();
        self.take = take;
        if playing {
            self.start();
        }
    }
    fn load(&mut self, path: std::path::PathBuf, ctx: &egui::Context) {
        self.status = "Opening WAV…".into();
        self.stop();
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = capture::load_wav_for_capture(&path)
                .map(|(rate, original, recording)| Track {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    rate,
                    original_view: Visual::measure(&original, rate),
                    regenerated_view: None,
                    original: original.into(),
                    rebuilt: None,
                    dry: None,
                    recording,
                    layers: None,
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Finished::Load(result));
            ctx.request_repaint();
        });
    }
    fn regenerate(&mut self, ctx: &egui::Context) {
        let Some(t) = &self.track else {
            return;
        };
        let (rate, input) = (t.rate, t.original.clone());
        let include_texture = self.include_texture;
        self.status = if include_texture {
            "Regenerating tonal layers and modeled texture…"
        } else {
            "Regenerating from measured frequencies and levels…"
        }
        .into();
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result =
                analyze_with_texture(&input, rate, include_texture).map_err(|e| e.to_string());
            drop(input);
            let result = result.and_then(|m| synthesize(&m, 12345).map_err(|e| e.to_string()));
            let _ = tx.send(Finished::Render(result.map(|w| {
                let view = Visual::measure(&w, rate);
                (w, view)
            })));
            ctx.request_repaint();
        });
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.job else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => {
                self.job = None;
                self.status = "Processing stopped unexpectedly.".into();
                return;
            }
        };
        self.job = None;
        match result {
            Finished::Load(Ok(t)) => {
                self.original_texture = Some(ctx.load_texture(
                    "Original spectrum",
                    t.original_view.spectrum.image(),
                    egui::TextureOptions::NEAREST,
                ));
                self.regenerated_texture = None;
                self.applied_setup = None;
                self.track = Some(t);
                self.cursor = 0.;
                self.take = Take::Original;
                self.status = "Loaded. Ready to regenerate.".into();
            }
            Finished::Render(Ok((w, visual))) => {
                if let Some(t) = self.track.as_mut() {
                    t.layers = None;
                }
                if let Some(t) = self.track.as_mut() {
                    self.regenerated_texture = Some(ctx.load_texture(
                        "Regenerated spectrum",
                        visual.spectrum.image(),
                        egui::TextureOptions::NEAREST,
                    ));
                    t.regenerated_view = Some(visual);
                    t.dry = Some(Arc::from(w.clone()));
                    t.rebuilt = Some(w);
                    self.applied_setup = None;
                }
                self.status =
                    "Ready to compare. Export keeps the source sample rate and length.".into();
            }
            Finished::Layers(Ok(session)) => {
                if let Some(t) = self.track.as_mut() {
                    t.layers = Some(session);
                }
                self.remix_layers(ctx);
            }
            Finished::Capture(setup, w, visual) => {
                if let Some(t) = self.track.as_mut() {
                    self.regenerated_texture = Some(ctx.load_texture(
                        "Virtual capture",
                        visual.spectrum.image(),
                        egui::TextureOptions::NEAREST,
                    ));
                    t.regenerated_view = Some(visual);
                    t.rebuilt = Some(w);
                    self.applied_setup = Some(setup);
                }
                self.status = format!(
                    "Capture ready: {}. Direct arrival aligned; output keeps source length.",
                    setup.room.label()
                );
            }
            Finished::Load(Err(e)) | Finished::Render(Err(e)) | Finished::Layers(Err(e)) => {
                self.status = e;
                if self.smoke.take().is_some() {
                    eprintln!("{}", self.status);
                    std::process::exit(1);
                }
            }
        }
    }
    fn capture(&mut self, ctx: &egui::Context) {
        let Some(t) = self.track.as_ref() else {
            return;
        };
        let Some(dry) = t.dry.clone() else {
            return;
        };
        self.stop();
        let rate = self.track.as_ref().unwrap().rate;
        let setup = self.setup.clamp();
        self.status = "Capturing the same regenerated take in the virtual room…".into();
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let w = capture::render(&dry, rate, setup);
            let view = Visual::measure(&w, rate);
            let _ = tx.send(Finished::Capture(setup, w, view));
            ctx.request_repaint();
        });
    }

    fn capture_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::CollapsingHeader::new("Capture lab · live track report").show(ui,|ui| {
            ui.strong("Measured from this file");
            match self.track.as_ref().map(|t|t.recording) {
                None=>{ ui.label("Open a recording to populate its report."); }
                Some(r) if r.channels==1=>{ui.label("Mono input. No stereo cues for left/right placement.");}
                Some(r)=>{
                    ui.label("Stereo input. Left channel drives regeneration; both channels inform this report.");
                    if let (Some(lr),Some(c))=(r.left_minus_right_db,r.lr_similarity) {
                        ui.label(format!("Left minus right: {lr:+.2} dB RMS   |   Channel similarity: {c:+.3}"));
                    } else {ui.label("No reliable stereo comparison for this signal.");}
                }
            };
            ui.label("Original mic position, distance and room dimensions: not identifiable from these cues.");
            ui.separator();
            ui.strong("Predicted virtual capture · editable, not the original room");
            ui.horizontal_wrapped(|ui| {
                let previous=self.setup.room;
                egui::ComboBox::from_id_salt("room_type").selected_text(self.setup.room.label()).show_ui(ui,|ui| {
                    for room in Room::CATALOG {
                        ui.selectable_value(&mut self.setup.room,room,room.label());
                    }
                });
                if previous!=self.setup.room {self.setup.custom_dimensions=None;}
                egui::ComboBox::from_id_salt("mic_type").selected_text(format!("Ideal {}",self.setup.mic.label())).show_ui(ui,|ui| {
                    for mic in Mic::CATALOG {ui.selectable_value(&mut self.setup.mic,mic,mic.label());}
                });
                if ui.button("Reset position").clicked(){self.setup.offset=Setup::default().offset;}
            });
            if self.setup.room != Room::Direct {
                egui::CollapsingHeader::new("Geometry and surface library").show(ui,|ui| {
                    let mut dims=self.setup.dimensions();
                    ui.horizontal_wrapped(|ui| {
                        if self.setup.room==Room::Open { ui.label("Open-air editing bounds"); } else { ui.label("Room dimensions"); }
                        for (v,label) in dims.iter_mut().zip(["Width","Depth","Height"]) {
                            ui.add(egui::DragValue::new(v).prefix(format!("{label}: ")).range(1.5..=40.).suffix(" m").speed(0.02));
                        }
                        if ui.button("Preset size").clicked(){dims=self.setup.room.dimensions();}
                    });
                    self.setup.custom_dimensions=(dims!=self.setup.room.dimensions()).then_some(dims);
                    let mut materials=self.setup.custom_materials.unwrap_or(self.setup.room.materials());
                    ui.horizontal_wrapped(|ui| {
                        for (axis, (label,surface)) in ["Walls","Floor / ground","Ceiling"].iter().zip(materials.iter_mut()).enumerate() {
                            if self.setup.room==Room::Open && axis!=1 { continue; }
                            ui.label(*label);
                            egui::ComboBox::from_id_salt(("surface",axis)).selected_text(surface.label()).show_ui(ui,|ui| {
                                for item in Material::CATALOG {ui.selectable_value(surface,item,item.label());}
                            });
                        }
                        if ui.button("Preset surfaces").clicked(){ materials=self.setup.room.materials(); }
                    });
                    self.setup.custom_materials=(materials!=self.setup.room.materials()).then_some(materials);
                    ui.label("Generic broadband reflection profiles, not measured materials.");
                });
            }
            egui::CollapsingHeader::new("Microphone references · specs only").show(ui,|ui| {
                ui.hyperlink_to("Shure SM57 · response and polar plots","https://www.shure.com/en-US/docs/guide/SM57");
                ui.hyperlink_to("Neumann KM 184 · response and polar plots","https://www.neumann.com/en-li/products/microphones/km-184-series-180/");
                ui.label("These are manufacturer references, not emulations. The selectable mics are ideal patterns with flat on-axis response.");
            });
            self.setup=self.setup.clamp();
            let dim=self.setup.dimensions();let src=self.setup.source();
            ui.horizontal_wrapped(|ui| {
                let (r,p)=ui.allocate_painter(Vec2::new(192.,110.),egui::Sense::click_and_drag());
                p.rect_filled(r.rect,4.,Color32::from_rgb(32,42,57));
                let boxr=r.rect.shrink(9.);
                for j in 0..=4 {
                    let f=j as f32/4.;
                    let x=boxr.left()+f*boxr.width();let y=boxr.top()+f*boxr.height();
                    p.line_segment([Pos2::new(x,boxr.top()),Pos2::new(x,boxr.bottom())],Stroke::new(1_f32,Color32::DARK_GRAY));
                    p.line_segment([Pos2::new(boxr.left(),y),Pos2::new(boxr.right(),y)],Stroke::new(1_f32,Color32::DARK_GRAY));
                }
                if r.dragged() || r.clicked() {
                    if let Some(pt)=r.interact_pointer_pos() {
                        self.setup.offset[0]=((pt.x-boxr.left())/boxr.width()).clamp(0.,1.)*dim[0]-src[0];
                        self.setup.offset[1]=((pt.y-boxr.top())/boxr.height()).clamp(0.,1.)*dim[1]-src[1];
                        self.setup=self.setup.clamp();
                    }
                }
                let xy=|x:f32,y:f32|Pos2::new(boxr.left()+x/dim[0]*boxr.width(),boxr.top()+y/dim[1]*boxr.height());
                let a=xy(src[0],src[1]);let b=xy(src[0]+self.setup.offset[0],src[1]+self.setup.offset[1]);
                p.line_segment([a,b],Stroke::new(1_f32,Color32::LIGHT_GRAY));
                p.circle_filled(a,5.,Color32::from_rgb(123,181,255));p.circle_filled(b,5.,Color32::from_rgb(255,192,106));
                p.text(a+Vec2::new(7.,-8.),egui::Align2::LEFT_CENTER,"Source",egui::FontId::proportional(11.),Color32::WHITE);
                p.text(b-Vec2::new(7.,-8.),egui::Align2::RIGHT_CENTER,"Mic",egui::FontId::proportional(11.),Color32::WHITE);
                ui.vertical(|ui| {
                    ui.label("Top view · drag or use the sliders");
                    for (a,label) in ["Left / right","Forward / back","Up / down"].iter().enumerate() {
                        ui.add(egui::Slider::new(&mut self.setup.offset[a],0.1-src[a]..=dim[a]-src[a]-0.1).text(*label).suffix(" m").fixed_decimals(2));
                    }
                });
            });
            self.setup=self.setup.clamp();
            let p=capture::predict(self.setup);
            if self.setup.room!=Room::Direct {
                let label=if self.setup.room==Room::Open {"Placement bounds"} else {"Room"};
                ui.label(format!("{label}: {:.1} × {:.1} × {:.1} m",dim[0],dim[1],dim[2]));
                let [wall,floor,ceiling]=self.setup.materials();
                if self.setup.room==Room::Open {ui.label(format!("Ground: {}",floor.label()));} else {
                    ui.label(format!("Walls: {}   |   Floor: {}   |   Ceiling: {}",wall.label(),floor.label(),ceiling.label()));
                }
            }
            ui.label(format!("Source to mic: {:.2} m   |   Direct path aligned for comparison",p.distance_m));
            if let (Some(ms),Some(g))=(p.first_reflection_ms,p.reflected_path_db){
                ui.label(format!("First modeled reflection: {ms:.1} ms after direct   |   Reflected path energy: {g:+.1} dB vs direct"));
                ui.label(format!("Last included reflection: {:.0} ms after direct (model limit, not reverb time)",p.last_path_ms));
            } else {ui.label("No reflections added. This does not remove the space already present in the performance.");}
            let ready=self.job.is_none()&&self.track.as_ref().is_some_and(|t|t.dry.is_some());
            if ui.add_enabled(ready,egui::Button::new("Render virtual capture")).clicked(){self.capture(ctx);}
            if let Some(old)=self.applied_setup {
                let pending=if old!=self.setup{" · changed settings not rendered yet"}else{""};
                ui.label(format!("Current output: {} ({:?}){pending}",old.room.label(),old.mic));
            } else {ui.label("Current output: regeneration with no added room. Regenerate first, then render a capture.");}
        });
    }
    fn export(&mut self, path: &Path) -> bool {
        let Some(t) = &self.track else {
            return false;
        };
        let Some(y) = t.rebuilt.as_deref() else {
            return false;
        };
        let result = write_wav(t.rate, y, path);
        let success = result.is_ok();
        self.status = match result {
            Ok(g) => format!(
                "Exported float32 mono WAV (export gain {:.1} dB).",
                20. * g.log10()
            ),
            Err(e) => format!("Export did not complete: {e}"),
        };
        success
    }
}

fn wave_panel(
    ui: &mut egui::Ui,
    label: &str,
    wave: Option<&[(f32, f32)]>,
    scale: f32,
    fraction: f64,
    color: Color32,
) -> Option<f64> {
    ui.label(egui::RichText::new(label).strong());
    let (resp, paint) = ui.allocate_painter(
        Vec2::new(ui.available_width().max(1.), 100.),
        egui::Sense::click(),
    );
    let r = resp.rect;
    paint.rect_filled(r, 7., Color32::from_rgb(24, 31, 42));
    if let Some(x) = wave {
        let n = (r.width() as usize).clamp(1, x.len());
        for i in 0..n {
            let p = x.len() * i / n;
            let q = x.len() * (i + 1) / n;
            let (lo, hi) = x[p..q]
                .iter()
                .fold((0_f32, 0_f32), |(a, b), &(lo, hi)| (a.min(lo), b.max(hi)));
            let xx = r.left() + r.width() * (i as f32 + 0.5) / n as f32;
            let y = |v| r.center().y - v / scale * r.height() * 0.44;
            paint.line_segment(
                [Pos2::new(xx, y(lo)), Pos2::new(xx, y(hi))],
                Stroke::new(1_f32, color),
            );
        }
        let x = r.left() + r.width() * fraction.clamp(0., 1.) as f32;
        paint.line_segment(
            [Pos2::new(x, r.top()), Pos2::new(x, r.bottom())],
            Stroke::new(1_f32, Color32::WHITE),
        );
    } else {
        paint.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for audio",
            egui::FontId::proportional(13.),
            Color32::LIGHT_GRAY,
        );
    }
    if resp.clicked() && wave.is_some() {
        resp.interact_pointer_pos()
            .map(|p| ((p.x - r.left()) / r.width()).clamp(0., 1.) as f64)
    } else {
        None
    }
}

fn spectrum_panel(
    ui: &mut egui::Ui,
    label: &str,
    visual: Option<&Visual>,
    texture: Option<&egui::TextureHandle>,
    fraction: f64,
) -> Option<f64> {
    ui.horizontal(|ui| {
        ui.strong(label);
        if let Some(v) = visual {
            ui.label(v.peak_dbfs());
        }
    });
    let (resp, painter) = ui.allocate_painter(
        Vec2::new(ui.available_width().max(1.), 112.),
        egui::Sense::click(),
    );
    let r = resp.rect;
    painter.rect_filled(r, 3., Spectrum::color(SPECTRUM_BOTTOM));
    let plot = egui::Rect::from_min_max(r.min + Vec2::new(59., 0.), r.max);
    if let Some((tex, view)) = texture.zip(visual) {
        painter.image(
            tex.id(),
            plot,
            egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
        let cap = view.spectrum.max_hz / 1000.;
        for (label, y) in [
            (format!("{cap:.1} kHz"), r.top() + 8.),
            (format!("{:.1} kHz", cap / 2.), r.center().y),
            ("0 kHz".to_string(), r.bottom() - 8.),
        ] {
            painter.text(
                Pos2::new(r.left() + 3., y),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(11.),
                Color32::LIGHT_GRAY,
            );
        }
        let x = plot.left() + plot.width() * fraction.clamp(0., 1.) as f32;
        let line = [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())];
        painter.line_segment(line, Stroke::new(3_f32, Color32::BLACK));
        painter.line_segment(line, Stroke::new(1_f32, Color32::WHITE));
    } else {
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for audio",
            egui::FontId::proportional(13.),
            Color32::LIGHT_GRAY,
        );
    }
    resp.interact_pointer_pos()
        .filter(|p| resp.clicked() && visual.is_some() && plot.contains(*p))
        .map(|p| ((p.x - plot.left()) / plot.width()).clamp(0., 1.) as f64)
}

fn spectrum_legend(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label("Quiet  −90");
        let (r, p) = ui.allocate_painter(Vec2::new(170., 10.), egui::Sense::hover());
        for i in 0..90 {
            let a = r.rect.left() + r.rect.width() * i as f32 / 90.;
            let b = r.rect.left() + r.rect.width() * (i + 1) as f32 / 90.;
            p.rect_filled(
                egui::Rect::from_min_max(Pos2::new(a, r.rect.top()), Pos2::new(b, r.rect.bottom())),
                0.,
                Spectrum::color(SPECTRUM_BOTTOM + i as f32),
            );
        }
        ui.label("0 dBFS  Loud");
    });
}

impl Studio {
    fn draw(&mut self, ctx: &egui::Context) {
        self.poll(ctx);
        if self.job.is_none() && self.smoke.is_some() {
            if let Some(track) = self.track.as_ref() {
                if track.rebuilt.is_some() {
                    if let Some(path) = self.smoke.take() {
                        if !self.export(&path) {
                            eprintln!("{}", self.status);
                            std::process::exit(1);
                        }
                    }
                    eprintln!("{}", self.status);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    self.regenerate(ctx);
                }
            } else {
                self.smoke = None;
                eprintln!("{}", self.status);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if let Some(p) = self.play.as_ref() {
            if p.sink.empty() {
                self.stop();
            } else {
                self.cursor = p.started_at + p.sink.get_pos().as_secs_f64();
                ctx.request_repaint_after(Duration::from_millis(30));
            }
        }
        egui::CentralPanel::default().show(ctx,|ui|{ egui::ScrollArea::vertical().show(ui,|ui|{
            ui.add_space(12.);
            ui.heading("Spectral Canvas");
            ui.label("Single-take regeneration • Reference Library Preview");
            ui.add_space(10.);
            ui.horizontal_wrapped(|ui|{
                if ui.add_enabled(self.job.is_none(),egui::Button::new("Open WAV…")).clicked() {
                    if let Some(path)=rfd::FileDialog::new().add_filter("WAV",&["wav"]).pick_file() {self.load(path,ctx);}
                }
                if ui.add_enabled(self.track.is_some()&&self.job.is_none(),egui::Button::new("Regenerate")).clicked() {self.regenerate(ctx);}
                if ui.add_enabled(self.track.as_ref().is_some_and(|t|t.rebuilt.is_some())&&self.job.is_none(),egui::Button::new("Export regenerated WAV…")).clicked() {
                    if let Some(path)=rfd::FileDialog::new().set_file_name("Regenerated.wav").add_filter("WAV",&["wav"]).save_file() {self.export(&path);}
                }
                if self.job.is_some() {ui.spinner();}
            });
            ui.add_enabled(self.job.is_none(),egui::Checkbox::new(&mut self.include_texture,"Include modeled texture (experimental)"));
            ui.separator();
            if let Some(t)=self.track.as_ref() {
                ui.label(format!("{}  |  {} Hz  |  {:.2} s  |  Mono output",t.name,t.rate,t.duration()));
            }
            ui.horizontal(|ui|{
                if ui.add_enabled(self.track.is_some(),egui::Button::new(if self.play.is_some() {"Stop"} else {"Play"})).clicked() {
                    if self.play.is_some() {self.stop();} else {self.start();}
                }
                for which in [Take::Original,Take::Regenerated] {
                    let enabled=self.track.as_ref().is_some_and(|t|which==Take::Original||t.rebuilt.is_some());
                    if ui.add_enabled(enabled,egui::Button::new(which.name()).selected(self.take==which)).clicked(){self.switch(which);}
                }
                ui.label(format!("{:.2} s",self.cursor));
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.spectrum_mode,true,"Spectrograms");
                ui.selectable_value(&mut self.spectrum_mode,false,"Waveforms");
            });
            let duration=self.track.as_ref().map_or(0.,Track::duration);
            let mut seek=self.cursor.clamp(0.,duration);
            let slider=ui.add_enabled(duration>0.,egui::Slider::new(&mut seek,0.0..=duration)
                .suffix(" s").text("Shared playhead").fixed_decimals(2));
            let t=self.track.as_ref();
            let fraction=if duration>0. { self.cursor/duration } else { 0. };
            let (a,b)=if self.spectrum_mode {
                spectrum_legend(ui);
                (
                    spectrum_panel(ui,"Original",t.map(|x|&x.original_view),self.original_texture.as_ref(),fraction),
                    spectrum_panel(ui,"Regenerated",t.and_then(|x|x.regenerated_view.as_ref()),self.regenerated_texture.as_ref(),fraction)
                )
            } else {
                let original=t.map(|t|&t.original_view.peaks[..]);
                let rebuilt=t.and_then(|t|t.regenerated_view.as_ref()).map(|v|&v.peaks[..]);
                let scale=original.into_iter().chain(rebuilt).flatten().fold(1.0_f32,|p,(lo,hi)|p.max(lo.abs()).max(hi.abs()));
                (
                    wave_panel(ui,"Original waveform",original,scale,fraction,Color32::from_rgb(151,185,248)),
                    wave_panel(ui,"Regenerated waveform",rebuilt,scale,fraction,Color32::from_rgb(106,226,193))
                )
            };
            let target=if slider.changed() { Some(seek) } else { a.or(b).map(|f| f*duration) };
            if let Some(time)=target {
                let playing=self.play.is_some();self.stop();self.cursor=time;
                if playing && time < duration {self.start();}
            }
            ui.add_space(8.);
            ui.label(&self.status);
            ui.label("The plots show raw levels on shared scales. Playback-only matching uses RMS; export gain is separate.");
            ui.label("Mono/stereo WAV excerpts up to 30 seconds. Output is mono. No inference of the actual room.");
            self.layer_panel(ui,ctx);
            self.capture_panel(ui,ctx);
        }); });
    }
}

impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.draw(ctx);
    }
}

fn main() -> eframe::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    let automated = first.as_deref() == Some(std::ffi::OsStr::new("--smoke-test"));
    let (initial, export) = if automated {
        (args.next(), args.next())
    } else {
        (first, None)
    };
    if automated && (initial.is_none() || export.is_none()) {
        eprintln!("Usage: spectral-canvas-desktop --smoke-test INPUT.wav OUTPUT.wav");
        std::process::exit(2);
    }
    eframe::run_native(
        "Spectral Canvas",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([790., 600.])
                .with_min_inner_size([360., 565.]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut app = Studio::new();
            app.smoke = export.map(std::path::PathBuf::from);
            if let Some(path) = initial {
                app.load(path.into(), &cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audition_level_matches_without_clipping() {
        let t = Track {
            name: String::new(),
            rate: 48000,
            original: vec![1., -1.].into(),
            rebuilt: Some(vec![0.5, -0.5]),
            dry: None,
            layers: None,
            recording: Recording {
                channels: 1,
                left_minus_right_db: None,
                lr_similarity: None,
            },
            original_view: Visual::measure(&[1., -1.], 48000),
            regenerated_view: Some(Visual::measure(&[0.5, -0.5], 48000)),
        };
        assert_eq!(t.audition(Take::Original), t.audition(Take::Regenerated));
        assert!(t
            .audition(Take::Original)
            .unwrap()
            .iter()
            .all(|x| x.abs() < 0.9));
    }

    #[test]
    fn spectrogram_does_not_normalize_away_six_db_difference() {
        let x: Vec<_> = (0..9600)
            .map(|i| 0.5 * (std::f32::consts::TAU * 1500. * i as f32 / 48000.).sin())
            .collect();
        let half: Vec<_> = x.iter().map(|v| v * 0.5).collect();
        let a = Spectrum::measure(&x, 48000);
        let b = Spectrum::measure(&half, 48000);
        assert_eq!(a.width, b.width);
        assert_eq!(a.max_hz, b.max_hz);
        let (i, peak) = a
            .dbfs
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .unwrap();
        assert!((peak + 6.0206).abs() < 0.01);
        assert!((peak - b.dbfs[i] - 6.0206).abs() < 0.001);
        assert_ne!(a.image().pixels[i], b.image().pixels[i]);
        let silence = Spectrum::measure(&[0.; 100], 8000);
        assert!(silence
            .image()
            .pixels
            .iter()
            .all(|&c| c == Spectrum::color(SPECTRUM_BOTTOM)));
    }

    #[test]
    fn desktop_state_imports_regenerates_draws_and_exports() {
        let base = std::env::temp_dir().join(format!(
            "spectral-canvas-desktop-test-{}",
            std::process::id()
        ));
        std::fs::create_dir(&base).unwrap();
        let source = base.join("input.wav");
        let output = base.join("output.wav");
        let x: Vec<_> = (0..9600)
            .map(|i| (0.2 * (std::f64::consts::TAU * 440. * i as f64 / 48000.).sin()) as f32)
            .collect();
        write_wav(48000, &x, &source).unwrap();
        let original = std::fs::read(&source).unwrap();
        let ctx = egui::Context::default();
        let mut app = Studio::new();
        app.load(source.clone(), &ctx);
        let draw = |app: &mut Studio| {
            let frame = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(780., 540.))),
                    ..Default::default()
                },
                |ctx| app.draw(ctx),
            );
            assert!(!frame.shapes.is_empty());
        };
        let settle = |app: &mut Studio| {
            for _ in 0..300 {
                draw(app);
                if app.job.is_none() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(4));
            }
            panic!("desktop job timed out");
        };
        settle(&mut app);
        assert!(app.track.is_some(), "{}", app.status);
        app.regenerate(&ctx);
        settle(&mut app);
        assert!(
            app.track.as_ref().unwrap().rebuilt.is_some(),
            "{}",
            app.status
        );
        let dry = app.track.as_ref().unwrap().dry.clone().unwrap();
        app.capture(&ctx);
        settle(&mut app);
        assert!(Arc::ptr_eq(
            &dry,
            app.track.as_ref().unwrap().dry.as_ref().unwrap()
        ));
        assert_eq!(app.applied_setup, Some(Setup::default()));
        assert!(app.original_texture.is_some() && app.regenerated_texture.is_some());
        app.spectrum_mode = false;
        draw(&mut app);
        let elapsed = 0.1;
        app.cursor = elapsed;
        app.switch(Take::Regenerated);
        assert_eq!(app.cursor, elapsed);
        assert!(app.export(&output), "{}", app.status);
        assert_eq!(std::fs::read(source).unwrap(), original);
        let w = hound::WavReader::open(output).unwrap();
        assert_eq!(
            (w.spec().channels, w.spec().sample_rate, w.duration()),
            (1, 48000, 9600)
        );
        std::fs::remove_dir_all(base).unwrap();
    }
}

impl Studio {
    fn build_layers(&mut self, ctx: &egui::Context) {
        let Some(t) = &self.track else {
            return;
        };
        let input = t.original.clone();
        let rate = t.rate;
        self.stop();
        self.status = "Analyzing editable pitch, ringing, texture and breath layers…".into();
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> spectral_canvas_mono::Result<LayerSession> {
                let packet = edge::analyze(&input, rate, edge::Options::default())?;
                let rendered = workspace::render(&packet, 734)?;
                let headroom = rendered.headroom(&input);
                Ok(LayerSession {
                    packet,
                    rendered,
                    controls: workspace::Controls::default(),
                    headroom,
                    view: None,
                })
            })()
            .map_err(|e| e.to_string());
            let _ = tx.send(Finished::Layers(result));
            ctx.request_repaint();
        });
    }
    fn remix_layers(&mut self, ctx: &egui::Context) {
        let playing = self.play.is_some();
        self.stop();
        if let Some(t) = self.track.as_mut() {
            if let Some(session) = &t.layers {
                match session.rendered.mix(&session.controls) {
                    Ok(mix) => {
                        let shown = session
                            .view
                            .map(|i| &session.rendered.stems[i][..])
                            .unwrap_or(&mix);
                        let visual = Visual::measure(shown, t.rate);
                        self.regenerated_texture = Some(ctx.load_texture(
                            "Editable layer",
                            visual.spectrum.image(),
                            egui::TextureOptions::NEAREST,
                        ));
                        t.regenerated_view = Some(visual);
                        t.dry = Some(Arc::from(mix.clone()));
                        t.rebuilt = Some(mix);
                        self.applied_setup = None;
                        self.status="Editable layers ready. Gain and isolation use fixed audition headroom.".into();
                    }
                    Err(e) => self.status = e.to_string(),
                }
            }
        }
        if playing {
            self.start();
        }
    }
    fn layer_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.separator();
        ui.heading("Editable analysis layers");
        if ui
            .add_enabled(
                self.track.is_some() && self.job.is_none(),
                egui::Button::new("Build editable layers"),
            )
            .clicked()
        {
            self.build_layers(ctx);
        }
        if ui
            .add_enabled(
                self.track.is_some() && self.job.is_none(),
                egui::Button::new("Load editable analysis…"),
            )
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Analysis", &["json"])
                .pick_file()
            {
                self.load_layers(&path, ctx);
            }
        }
        let mut changed = false;
        let mut export = false;
        if let Some(session) = self.track.as_mut().and_then(|t| t.layers.as_mut()) {
            for i in 0..4 {
                ui.horizontal_wrapped(|ui| {
                    ui.label(workspace::NAMES[i]);
                    changed |= ui
                        .checkbox(&mut session.controls.muted[i], "Mute")
                        .changed();
                    if ui
                        .selectable_label(session.controls.solo == Some(i), "Solo")
                        .clicked()
                    {
                        session.controls.solo = if session.controls.solo == Some(i) {
                            None
                        } else {
                            Some(i)
                        };
                        changed = true;
                    }
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut session.controls.gain_db[i], -60. ..=6.)
                                .suffix(" dB"),
                        )
                        .changed();
                    if ui
                        .selectable_label(session.view == Some(i), "View")
                        .clicked()
                    {
                        session.view = Some(i);
                        changed = true;
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("Reset layers").clicked() {
                    session.controls = workspace::Controls::default();
                    session.view = None;
                    changed = true;
                }
                if ui.button("View full mix").clicked() {
                    session.view = None;
                    changed = true;
                }
                export = ui.button("Save editable analysis…").clicked();
            });
            if let Some(i) = session.view {
                ui.label(format!(
                    "{} · {} evidence spans",
                    workspace::NAMES[i],
                    session.rendered.spans[i].len()
                ));
                for span in &session.rendered.spans[i] {
                    ui.label(format!("{:.3}–{:.3} s", span[0], span[1]));
                }
            }
        }
        if changed {
            self.remix_layers(ctx);
        }
        if export {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("analysis.json")
                .save_file()
            {
                if let Some(session) = self.track.as_ref().and_then(|t| t.layers.as_ref()) {
                    self.status = match serde_json::to_vec_pretty(&session.packet)
                        .map_err(|e| e.to_string())
                        .and_then(|bytes| {
                            spectral_canvas_mono::qa::write_new(&path, &bytes)
                                .map_err(|e| e.to_string())
                        }) {
                        Ok(()) => "Saved editable analysis packet without source PCM.".into(),
                        Err(e) => e,
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod layer_tests {
    use super::*;
    #[test]
    fn native_layer_controls_preserve_reference_and_fixed_gain() {
        let ctx = egui::Context::default();
        let rate = 48000;
        let x: Vec<_> = (0..9600)
            .map(|i| (0.2 * (std::f64::consts::TAU * 440. * i as f64 / rate as f64).sin()) as f32)
            .collect();
        let packet = edge::analyze(&x, rate, edge::Options::default()).unwrap();
        let rendered = workspace::render(&packet, 734).unwrap();
        let headroom = rendered.headroom(&x);
        let mut app = Studio::new();
        app.track = Some(Track {
            name: "layers".into(),
            rate,
            original_view: Visual::measure(&x, rate),
            regenerated_view: None,
            original: x.clone().into(),
            rebuilt: None,
            dry: None,
            recording: Recording {
                channels: 1,
                left_minus_right_db: None,
                lr_similarity: None,
            },
            layers: Some(LayerSession {
                packet,
                rendered,
                controls: Default::default(),
                headroom,
                view: None,
            }),
        });
        app.remix_layers(&ctx);
        let reference = app
            .track
            .as_ref()
            .unwrap()
            .audition(Take::Original)
            .unwrap();
        for layer in 0..4 {
            app.track
                .as_mut()
                .unwrap()
                .layers
                .as_mut()
                .unwrap()
                .controls
                .solo = Some(layer);
            app.remix_layers(&ctx);
            let t = app.track.as_ref().unwrap();
            assert_eq!(
                t.rebuilt.as_ref().unwrap(),
                &t.layers.as_ref().unwrap().rendered.stems[layer]
            );
            assert_eq!(t.audition(Take::Original).unwrap(), reference);
        }
        let s = app.track.as_mut().unwrap().layers.as_mut().unwrap();
        s.controls = workspace::Controls {
            gain_db: [0., 0., 0., -6.],
            ..Default::default()
        };
        s.view = Some(1);
        app.remix_layers(&ctx);
        for width in [390., 1200.] {
            let frame = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(width, 900.),
                    )),
                    ..Default::default()
                },
                |ctx| app.draw(ctx),
            );
            assert!(!frame.shapes.is_empty());
        }
        let path =
            std::env::temp_dir().join(format!("sc-desktop-packet-{}.json", std::process::id()));
        let bytes =
            serde_json::to_vec(&app.track.as_ref().unwrap().layers.as_ref().unwrap().packet)
                .unwrap();
        spectral_canvas_mono::qa::write_new(&path, &bytes).unwrap();
        app.load_layers(&path, &ctx);
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while app.job.is_some() && std::time::Instant::now() < deadline {
            app.poll(&ctx);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.job.is_none(), "analysis load worker timed out");
        assert!(app.track.as_ref().unwrap().layers.is_some());
        assert_eq!(
            app.track
                .as_ref()
                .unwrap()
                .layers
                .as_ref()
                .unwrap()
                .controls
                .gain_db,
            [0.; 4]
        );
        std::fs::remove_file(path).unwrap();
        app.cursor = 0.12;
        app.switch(Take::Original);
        assert_eq!(app.cursor, 0.12);
        assert_eq!(&app.track.as_ref().unwrap().original[..], &x);
    }
}

impl Studio {
    fn load_layers(&mut self, path: &Path, ctx: &egui::Context) {
        let Some(track) = &self.track else {
            return;
        };
        let source = track.original.clone();
        let rate = track.rate;
        let bytes = match std::fs::read(path) {
            Ok(v) if v.len() <= 64 * 1024 * 1024 => v,
            Ok(_) => {
                self.status = "Analysis exceeds 64 MiB.".into();
                return;
            }
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        self.stop();
        self.status = "Loading editable analysis…".into();
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> spectral_canvas_mono::Result<LayerSession> {
                let packet: edge::Packet = serde_json::from_slice(&bytes)?;
                if packet.base.rate != rate || packet.base.samples != source.len() {
                    return Err(
                        "Analysis rate and duration must match the loaded reference.".into(),
                    );
                }
                let rendered = workspace::render(&packet, 734)?;
                let headroom = rendered.headroom(&source);
                Ok(LayerSession {
                    packet,
                    rendered,
                    controls: Default::default(),
                    headroom,
                    view: None,
                })
            })()
            .map_err(|e| e.to_string());
            let _ = tx.send(Finished::Layers(result));
            ctx.request_repaint();
        });
    }
}
