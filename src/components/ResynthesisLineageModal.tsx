import { BookOpen, X } from 'lucide-react';

interface ResynthesisLineageModalProps {
  open: boolean;
  onClose: () => void;
}

const PREDECESSORS = [
  {
    name: 'IRCAM AudioSculpt',
    focus: 'Waveform + spectrum + sonogram, partial tracking, spectral modeling, partial & cross-synthesis',
    takeaway: 'Validates high-resolution partial analysis followed by controllable signal resynthesis.',
  },
  {
    name: 'ARSS',
    focus: 'Spectrogram analysis and spectrogram-to-signal synthesis',
    takeaway: 'Shows time-frequency representations can drive direct waveform reconstruction.',
  },
  {
    name: 'Logic Alchemy / Sample Alchemy',
    focus: 'Additive and spectral resynthesis inside creative sample instruments',
    takeaway: 'Demonstrates practical harmonic/spectral resynthesis for playable sound design.',
  },
  {
    name: 'Panharmonium & Spectraphon',
    focus: 'Hardware real-time spectral resynthesizers',
    takeaway: 'Confirms live spectral tracking and oscillator-bank resynthesis as a physical instrument paradigm.',
  },
  {
    name: 'Academic SMS (Sinusoids + Residual)',
    focus: 'Deterministic harmonic partials paired with stochastic broadband residual modeling',
    takeaway: 'Establishes clean separation between tonal trajectory and breath/bow/pick noise texture.',
  },
];

const PREREQUISITES = [
  'Single-source performance cues (timing, pitch trajectories, bends, dynamics, sustain, overlaps, and frame evidence) must be extracted honestly before any resynthesis can be trusted.',
  'Structured Capture Lab parameters (Source voicing, Microphone model/polar/aim, Surface materials & Biot/absorption data, and real Room geometry) define what “re-recorded under new conditions” means.',
  'Geometric capture physics (direct path, image-source reflections, 343.0 m/s speed of sound, Sabine RT60, axial modes) and factual reference catalogs keep outputs physically grounded.',
  'Display-only viewport actions (units, zoom, orbit, Fit View, overlays, dock layout) are strictly isolated from acoustic parameters so visual inspection never alters audio.',
  'Zero original waveform blending is enforced structurally: `studio-prompt-render` synthesizes dry + captured WAV strictly from cues JSON + rig config without opening the original WAV.',
];

const DIFFERENCES = [
  {
    label: 'Fixed Shape Reference',
    detail:
      'Treats the uploaded single-source track as read-only signal evidence (SHAPE SHA) rather than an audio buffer to patch, stretch, or crossfade.',
  },
  {
    label: 'Capture Lab Structured Prompt',
    detail:
      'Uses structured acoustic parameters (Source + Mic + Materials + Room), not opaque text prompts or black-box style transfers.',
  },
  {
    label: 'Atomic Fresh Signal Synthesis',
    detail:
      'Synthesizes a brand-new dry waveform and virtual room capture from extracted cues + capture conditions (`original_audio_opened: false`).',
  },
  {
    label: 'Honest Uncertainty Preservation',
    detail:
      'Preserves uncertain frames, bleed risk, and out-of-range notes as performed instead of flattening ambiguity or silently clamping pitch.',
  },
  {
    label: '100% Native Local Rust DSP',
    detail:
      'Runs deterministic local Rust analysis and synthesis with zero runtime network dependencies and no large-scale external music training catalogs.',
  },
];

const NON_GOALS = [
  'Not a general creative sound mangler or wild spectral morphing effect.',
  'Not a pure audio restoration, denoise, or waveform healing/inpainting tool.',
  'Never blends, clones, or mixes original reference samples into dry or captured outputs.',
  'Never invents unsupported musical notes or forces performed pitch into published instrument ranges.',
  'Does not retrain on external music datasets or rely on cloud LLM inference to generate audio.',
];

export function ResynthesisLineageModal({ open, onClose }: ResynthesisLineageModalProps) {
  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 bg-black/65 flex items-center justify-center p-4"
      role="dialog"
      aria-modal="true"
      aria-label="Position Relative to Existing Resynthesis Tools"
    >
      <div className="w-[820px] max-w-full max-h-[86vh] bg-[#262626] border border-[#444444] text-[#dcdcdc] flex flex-col">
        {/* Flat-charcoal modal header */}
        <div className="h-9 px-3 bg-[#2e2e2e] border-b border-[#3e3e3e] flex items-center justify-between shrink-0">
          <div className="flex items-center gap-2">
            <BookOpen className="w-3.5 h-3.5 text-[#378ef0]" />
            <span className="font-medium text-[12px] text-[#e8e8e8]">
              SpectraCraft — Position Relative to Existing Resynthesis Tools
            </span>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close Resynthesis Lineage dialog"
            className="text-[#9a9a9a] hover:text-white p-0.5"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Scrollable structured content */}
        <div className="p-3.5 overflow-y-auto space-y-3 text-[11px]">
          {/* Lead positioning strip */}
          <div className="bg-[#1e1e1e] border border-[#363636] p-2.5 flex flex-wrap items-center justify-between gap-2">
            <div className="text-[#d4d4d4] leading-snug max-w-[600px]">
              Analyze a single-source signal &rarr; extract an honest performance &amp; spectral shape
              description &rarr; synthesize a fresh dry and virtually captured waveform under explicit
              Capture Lab conditions.
            </div>
            <span className="font-mono text-[10px] px-2 py-0.5 bg-[#25303d] border border-[#378ef0]/50 text-[#8bc2ff] shrink-0">
              SHAPE-GUIDED CAPTURE SYNTHESIS
            </span>
          </div>

          {/* Section 1: Known Resynthesis Predecessors */}
          <section className="bg-[#1e1e1e] border border-[#363636] p-2.5 space-y-2">
            <div className="flex items-center justify-between border-b border-[#2e2e2e] pb-1">
              <h2 className="font-medium text-[#e8e8e8] text-[11px] uppercase tracking-wider">
                1. Known Resynthesis Predecessors (What They Prove)
              </h2>
              <span className="font-mono text-[10px] text-[#8a8a8a]">
                Technical path proven over decades
              </span>
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-1.5">
              {PREDECESSORS.map((item) => (
                <div
                  key={item.name}
                  className="bg-[#242424] border border-[#333333] p-2 space-y-0.5"
                >
                  <div className="font-medium text-[#e4e4e4] text-[11px]">{item.name}</div>
                  <div className="text-[#9e9e9e] text-[10px] leading-snug">{item.focus}</div>
                  <div className="text-[#4cc6ac] text-[10px] leading-snug">{item.takeaway}</div>
                </div>
              ))}
            </div>
          </section>

          {/* Two-column middle row: Why Capture Lab Came First + What SpectraCraft Does Differently */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            {/* Section 2: Why Capture Lab Parameters Had to Come First */}
            <section className="bg-[#1e1e1e] border border-[#363636] p-2.5 space-y-2">
              <div className="border-b border-[#2e2e2e] pb-1">
                <h2 className="font-medium text-[#e8e8e8] text-[11px] uppercase tracking-wider">
                  2. Why Capture Lab Parameters Came First
                </h2>
              </div>
              <ul className="space-y-1.5 text-[#c8c8c8] leading-snug">
                {PREREQUISITES.map((line, i) => (
                  <li key={i} className="flex items-start gap-2">
                    <span className="font-mono text-[10px] text-[#378ef0] mt-0.5 shrink-0">
                      0{i + 1}
                    </span>
                    <span>{line}</span>
                  </li>
                ))}
              </ul>
            </section>

            {/* Section 3: What SpectraCraft Does Differently */}
            <section className="bg-[#1e1e1e] border border-[#363636] p-2.5 space-y-2">
              <div className="border-b border-[#2e2e2e] pb-1">
                <h2 className="font-medium text-[#e8e8e8] text-[11px] uppercase tracking-wider">
                  3. What SpectraCraft Does Differently
                </h2>
              </div>
              <div className="space-y-1.5">
                {DIFFERENCES.map((item) => (
                  <div
                    key={item.label}
                    className="bg-[#242424] border border-[#323232] px-2 py-1.5 space-y-0.5"
                  >
                    <div className="font-mono text-[10px] text-[#8bc2ff]">{item.label}</div>
                    <div className="text-[#cccccc] text-[10.5px] leading-snug">{item.detail}</div>
                  </div>
                ))}
              </div>
            </section>
          </div>

          {/* Section 4: Explicit Non-Goals & Scope Boundaries */}
          <section className="bg-[#1e1e1e] border border-[#363636] p-2.5 space-y-1.5">
            <div className="flex items-center justify-between border-b border-[#2e2e2e] pb-1">
              <h2 className="font-medium text-[#e8e8e8] text-[11px] uppercase tracking-wider">
                4. Explicit Non-Goals &amp; Scope Boundaries
              </h2>
              <span className="font-mono text-[10px] text-[#e5b96c]">
                Enforced in Rust &amp; UI
              </span>
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-x-4 gap-y-1 text-[#c6c6c6]">
              {NON_GOALS.map((rule, i) => (
                <div key={i} className="flex items-start gap-2 leading-snug">
                  <span className="font-mono text-[#e5b96c] shrink-0">&times;</span>
                  <span>{rule}</span>
                </div>
              ))}
            </div>
          </section>
        </div>

        {/* Footer */}
        <div className="h-10 px-3 bg-[#2c2c2c] border-t border-[#3e3e3e] flex items-center justify-between shrink-0 text-[11px]">
          <span className="font-mono text-[10px] text-[#8e8e8e]">
            Engine: spectracraft-mono (Rust) · Atomic prompt render: studio-prompt-render (original_audio_opened: false)
          </span>
          <button
            type="button"
            onClick={onClose}
            className="px-3 py-1 bg-[#378ef0] hover:bg-[#2c7ad4] text-white font-medium text-[11px]"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
