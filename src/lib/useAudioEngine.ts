import { useCallback, useEffect, useRef, useState } from 'react';
import { buildBitDepthBuffer } from './audioCanvas.ts';
import { dbToSafeLinearGain } from './contract.ts';

export type TakeMode = 'original' | 'regenerated' | 'captured' | 'difference';

interface UseAudioEngineOptions {
  origBuffer: AudioBuffer | null;
  regenBuffer: AudioBuffer | null;
  capturedBuffer: AudioBuffer | null;
  diffBuffer: AudioBuffer | null;
  previewBitDepth: 32 | 24 | 16;
  levelMatchAb: boolean;
  abDryMatchGainDb: number;
  durationS: number;
  hasSession: boolean;
}

/**
 * Single-responsibility Web Audio playback & seamless crossfade engine (Refactoring §3d).
 *
 * Invariants:
 * - Playback engine owns playhead; UI redraws or layout changes never trigger a seek.
 * - Switching takes preserves the exact playhead timestamp.
 * - Live Preview background re-renders smoothly crossfade (40 ms GainNode ramp) at the
 *   current playhead without stopping or restarting playback.
 */
export function useAudioEngine({
  origBuffer,
  regenBuffer,
  capturedBuffer,
  diffBuffer,
  previewBitDepth,
  levelMatchAb,
  abDryMatchGainDb,
  durationS,
  hasSession,
}: UseAudioEngineOptions) {
  const [takeMode, setTakeMode] = useState<TakeMode>('regenerated');
  const takeModeRef = useRef<TakeMode>('regenerated');
  const [isPlaying, setIsPlaying] = useState(false);
  const [playheadS, setPlayheadS] = useState(0);

  const playheadRef = useRef(0);
  const isPlayingRef = useRef(false);
  const audioCtxRef = useRef<AudioContext | null>(null);
  const activeSourceRef = useRef<AudioBufferSourceNode | null>(null);
  const activeGainRef = useRef<GainNode | null>(null);
  const fadingNodesRef = useRef<Set<{ src: AudioBufferSourceNode; gain: GainNode }>>(new Set());
  const bitDepthCacheRef = useRef<WeakMap<AudioBuffer, Partial<Record<16 | 24, AudioBuffer>>>>(
    new WeakMap(),
  );
  const playStartCtxTimeRef = useRef(0);
  const playStartOffsetRef = useRef(0);
  const rafRef = useRef<number | null>(null);

  useEffect(() => {
    takeModeRef.current = takeMode;
  }, [takeMode]);

  const ensureAudioContext = useCallback(() => {
    if (!audioCtxRef.current) {
      const Ctx =
        window.AudioContext ||
        (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
      audioCtxRef.current = new Ctx();
    }
    return audioCtxRef.current;
  }, []);

  const stopPlaybackNode = useCallback(() => {
    for (const entry of fadingNodesRef.current) {
      try {
        entry.src.onended = null;
        entry.src.stop();
        entry.src.disconnect();
      } catch {
        // already stopped
      }
      try {
        entry.gain.disconnect();
      } catch {
        // already disconnected
      }
    }
    fadingNodesRef.current.clear();
    if (activeSourceRef.current) {
      try {
        activeSourceRef.current.onended = null;
        activeSourceRef.current.stop();
        activeSourceRef.current.disconnect();
      } catch {
        // already stopped
      }
      activeSourceRef.current = null;
    }
    if (activeGainRef.current) {
      try {
        activeGainRef.current.disconnect();
      } catch {
        // already disconnected
      }
      activeGainRef.current = null;
    }
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
  }, []);

  const decodeWavUrl = useCallback(
    async (url: string): Promise<AudioBuffer> => {
      const res = await fetch(url);
      if (!res.ok) throw new Error(`Failed to fetch WAV (${res.status})`);
      const arr = await res.arrayBuffer();
      const ctx = ensureAudioContext();
      return ctx.decodeAudioData(arr.slice(0));
    },
    [ensureAudioContext],
  );

  const getPlaybackBufferForTake = useCallback(
    (mode: TakeMode): AudioBuffer | null => {
      const base =
        mode === 'original'
          ? origBuffer
          : mode === 'difference'
            ? diffBuffer
            : mode === 'captured'
              ? capturedBuffer ?? regenBuffer
              : regenBuffer;
      if (!base) return null;
      if (previewBitDepth === 32) return base;
      let entry = bitDepthCacheRef.current.get(base);
      if (!entry) {
        entry = {};
        bitDepthCacheRef.current.set(base, entry);
      }
      const cached = entry[previewBitDepth];
      if (cached) return cached;
      const ctx = ensureAudioContext();
      const built = buildBitDepthBuffer(ctx, base, previewBitDepth);
      entry[previewBitDepth] = built;
      return built;
    },
    [origBuffer, regenBuffer, capturedBuffer, diffBuffer, previewBitDepth, ensureAudioContext],
  );

  const crossfadeToBuffer = useCallback(
    (nextBuf: AudioBuffer) => {
      if (!isPlayingRef.current || !audioCtxRef.current) return;
      const ctx = audioCtxRef.current;
      const now = ctx.currentTime;
      const fadeS = 0.04;
      const pos = Math.max(0, Math.min(nextBuf.duration - 0.02, playheadRef.current));
      const matchDb = takeModeRef.current === 'regenerated' && levelMatchAb ? abDryMatchGainDb : 0;
      const targetGain = dbToSafeLinearGain(matchDb);

      const oldSrc = activeSourceRef.current;
      const oldGain = activeGainRef.current;

      const newSrc = ctx.createBufferSource();
      const newGain = ctx.createGain();
      newSrc.buffer = nextBuf;
      newGain.gain.setValueAtTime(0.0001, now);
      newGain.gain.linearRampToValueAtTime(targetGain, now + fadeS);
      newSrc.connect(newGain);
      newGain.connect(ctx.destination);
      newSrc.start(now, pos);

      if (oldGain && oldSrc) {
        const fadingEntry = { src: oldSrc, gain: oldGain };
        fadingNodesRef.current.add(fadingEntry);
        try {
          oldSrc.onended = () => {
            fadingNodesRef.current.delete(fadingEntry);
            try {
              oldSrc.disconnect();
            } catch {
              // ignore
            }
            try {
              oldGain.disconnect();
            } catch {
              // ignore
            }
          };
          oldGain.gain.cancelScheduledValues(now);
          oldGain.gain.setValueAtTime(Math.max(0.0001, oldGain.gain.value), now);
          oldGain.gain.linearRampToValueAtTime(0.0001, now + fadeS);
          oldSrc.stop(now + fadeS + 0.01);
        } catch {
          fadingNodesRef.current.delete(fadingEntry);
        }
      }

      activeSourceRef.current = newSrc;
      activeGainRef.current = newGain;
      playStartCtxTimeRef.current = now;
      playStartOffsetRef.current = pos;

      newSrc.onended = () => {
        if (activeSourceRef.current === newSrc) {
          activeSourceRef.current = null;
          activeGainRef.current = null;
          isPlayingRef.current = false;
          setIsPlaying(false);
        }
      };
    },
    [levelMatchAb, abDryMatchGainDb],
  );

  // Live-update gain when A/B level-match setting toggles during active playback
  useEffect(() => {
    if (!isPlayingRef.current || !audioCtxRef.current || !activeGainRef.current) return;
    const ctx = audioCtxRef.current;
    const now = ctx.currentTime;
    const matchDb = takeModeRef.current === 'regenerated' && levelMatchAb ? abDryMatchGainDb : 0;
    const targetGain = dbToSafeLinearGain(matchDb);
    try {
      activeGainRef.current.gain.cancelScheduledValues(now);
      activeGainRef.current.gain.setValueAtTime(activeGainRef.current.gain.value, now);
      activeGainRef.current.gain.linearRampToValueAtTime(targetGain, now + 0.02);
    } catch {
      // ignore if audio context is closing
    }
  }, [levelMatchAb, abDryMatchGainDb]);

  // Live-crossfade when preview bit depth changes during active playback
  const prevBitDepthRef = useRef<32 | 24 | 16>(previewBitDepth);
  useEffect(() => {
    if (prevBitDepthRef.current === previewBitDepth) return;
    prevBitDepthRef.current = previewBitDepth;
    if (!isPlayingRef.current) return;
    const nextBuf = getPlaybackBufferForTake(takeModeRef.current);
    if (nextBuf) {
      crossfadeToBuffer(nextBuf);
    }
  }, [previewBitDepth, getPlaybackBufferForTake, crossfadeToBuffer]);

  const startPlaybackAt = useCallback(
    (offsetS: number, mode: TakeMode) => {
      const buf = getPlaybackBufferForTake(mode);
      if (!buf) return;
      stopPlaybackNode();
      const ctx = ensureAudioContext();
      if (ctx.state === 'suspended') {
        void ctx.resume();
      }
      const clamped = Math.max(0, Math.min(buf.duration - 0.01, offsetS));
      const src = ctx.createBufferSource();
      const gainNode = ctx.createGain();
      const matchDb = mode === 'regenerated' && levelMatchAb ? abDryMatchGainDb : 0;
      gainNode.gain.value = dbToSafeLinearGain(matchDb);

      src.buffer = buf;
      src.connect(gainNode);
      gainNode.connect(ctx.destination);
      src.start(0, clamped);
      activeSourceRef.current = src;
      activeGainRef.current = gainNode;
      playStartCtxTimeRef.current = ctx.currentTime;
      playStartOffsetRef.current = clamped;
      playheadRef.current = clamped;
      isPlayingRef.current = true;
      setIsPlaying(true);

      src.onended = () => {
        if (activeSourceRef.current === src) {
          activeSourceRef.current = null;
          activeGainRef.current = null;
          isPlayingRef.current = false;
          setIsPlaying(false);
        }
      };

      const tick = () => {
        if (!isPlayingRef.current || !audioCtxRef.current) return;
        const elapsed = audioCtxRef.current.currentTime - playStartCtxTimeRef.current;
        const pos = Math.min(buf.duration, playStartOffsetRef.current + elapsed);
        playheadRef.current = pos;
        setPlayheadS(pos);
        if (pos < buf.duration && isPlayingRef.current) {
          rafRef.current = requestAnimationFrame(tick);
        }
      };
      rafRef.current = requestAnimationFrame(tick);
    },
    [getPlaybackBufferForTake, stopPlaybackNode, ensureAudioContext, levelMatchAb, abDryMatchGainDb],
  );

  const togglePlay = useCallback(() => {
    if (!hasSession) return;
    if (isPlayingRef.current) {
      stopPlaybackNode();
      isPlayingRef.current = false;
      setIsPlaying(false);
    } else {
      const startAt = playheadRef.current >= durationS - 0.05 ? 0 : playheadRef.current;
      startPlaybackAt(startAt, takeMode);
    }
  }, [hasSession, durationS, takeMode, startPlaybackAt, stopPlaybackNode]);

  const seek = useCallback(
    (nextS: number) => {
      const clamped = Math.max(0, Math.min(durationS, nextS));
      playheadRef.current = clamped;
      setPlayheadS(clamped);
      if (isPlayingRef.current) {
        startPlaybackAt(clamped, takeMode);
      }
    },
    [durationS, takeMode, startPlaybackAt],
  );

  const switchTake = useCallback(
    (nextTake: TakeMode) => {
      setTakeMode(nextTake);
      if (isPlayingRef.current) {
        startPlaybackAt(playheadRef.current, nextTake);
      }
    },
    [startPlaybackAt],
  );

  const resetTransport = useCallback(
    (nextTake: TakeMode = 'regenerated') => {
      stopPlaybackNode();
      isPlayingRef.current = false;
      setIsPlaying(false);
      playheadRef.current = 0;
      setPlayheadS(0);
      setTakeMode(nextTake);
    },
    [stopPlaybackNode],
  );

  useEffect(
    () => () => {
      stopPlaybackNode();
      if (audioCtxRef.current && audioCtxRef.current.state !== 'closed') {
        void audioCtxRef.current.close().catch(() => {
          // ignore close errors during teardown
        });
        audioCtxRef.current = null;
      }
    },
    [stopPlaybackNode],
  );

  return {
    takeMode,
    setTakeMode,
    takeModeRef,
    isPlaying,
    isPlayingRef,
    playheadS,
    playheadRef,
    ensureAudioContext,
    decodeWavUrl,
    crossfadeToBuffer,
    togglePlay,
    seek,
    switchTake,
    resetTransport,
  };
}
