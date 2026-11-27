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

export interface PlaybackClockAnchor {
  startCtxTimeS: number;
  startOffsetS: number;
  activeDurationS: number;
}

/**
 * Pure playback position computation from AudioContext.currentTime and explicit anchor (Audit M12).
 */
export function computePlaybackPositionS(
  anchor: PlaybackClockAnchor,
  nowCtxTimeS: number,
): number {
  const elapsed = Math.max(0, nowCtxTimeS - anchor.startCtxTimeS);
  const maxDur = Math.max(0, anchor.activeDurationS);
  return Math.max(0, Math.min(maxDur, anchor.startOffsetS + elapsed));
}

/**
 * Pure crossfade target offset derived directly from the live AudioContext clock
 * (never stale RAF state) and clamped safely inside the destination buffer's duration (Audit M12).
 */
export function computeCrossfadeOffsetS(
  anchor: PlaybackClockAnchor,
  nowCtxTimeS: number,
  nextBufferDurationS: number,
): number {
  const livePos = computePlaybackPositionS(anchor, nowCtxTimeS);
  const safeMax = Math.max(0, nextBufferDurationS - 0.02);
  return Math.max(0, Math.min(safeMax, livePos));
}

/**
 * Single-responsibility Web Audio playback & seamless crossfade engine (Refactoring §3d).
 *
 * Invariants:
 * - Playback engine owns playhead via AudioContext.currentTime + explicit anchors (Audit M12).
 * - UI redraws or layout changes never trigger a seek.
 * - Switching takes or crossfading to a longer capture tail updates activeDurationSRef immediately
 *   so the playhead never caps at an earlier buffer's duration.
 * - Live Preview background re-renders smoothly crossfade (40 ms GainNode ramp) at the
 *   live AudioContext clock timestamp without stopping or restarting playback.
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
  const activeDurationSRef = useRef(0);
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

  const readLivePlayheadS = useCallback((): number => {
    if (!isPlayingRef.current || !audioCtxRef.current) {
      return playheadRef.current;
    }
    return computePlaybackPositionS(
      {
        startCtxTimeS: playStartCtxTimeRef.current,
        startOffsetS: playStartOffsetRef.current,
        activeDurationS: activeDurationSRef.current,
      },
      audioCtxRef.current.currentTime,
    );
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
    async (url: string, signal?: AbortSignal): Promise<AudioBuffer> => {
      const res = await fetch(url, signal ? { signal } : undefined);
      if (!res.ok) throw new Error(`Failed to fetch WAV (${res.status})`);
      const arr = await res.arrayBuffer();
      if (signal?.aborted) {
        throw new DOMException('Audio decode aborted', 'AbortError');
      }
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

  const scheduleTickLoop = useCallback(() => {
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    const tick = () => {
      if (!isPlayingRef.current || !audioCtxRef.current) return;
      const currentDur = activeDurationSRef.current;
      const pos = computePlaybackPositionS(
        {
          startCtxTimeS: playStartCtxTimeRef.current,
          startOffsetS: playStartOffsetRef.current,
          activeDurationS: currentDur,
        },
        audioCtxRef.current.currentTime,
      );
      playheadRef.current = pos;
      setPlayheadS(pos);
      if (pos < currentDur && isPlayingRef.current) {
        rafRef.current = requestAnimationFrame(tick);
      } else {
        rafRef.current = null;
      }
    };
    rafRef.current = requestAnimationFrame(tick);
  }, []);

  const crossfadeToBuffer = useCallback(
    (nextBuf: AudioBuffer) => {
      if (!isPlayingRef.current || !audioCtxRef.current) return;
      const ctx = audioCtxRef.current;
      const now = ctx.currentTime;
      const fadeS = 0.04;
      const pos = computeCrossfadeOffsetS(
        {
          startCtxTimeS: playStartCtxTimeRef.current,
          startOffsetS: playStartOffsetRef.current,
          activeDurationS: activeDurationSRef.current,
        },
        now,
        nextBuf.duration,
      );
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
      activeDurationSRef.current = nextBuf.duration;
      playheadRef.current = pos;

      newSrc.onended = () => {
        if (activeSourceRef.current === newSrc) {
          activeSourceRef.current = null;
          activeGainRef.current = null;
          isPlayingRef.current = false;
          setIsPlaying(false);
        }
      };

      scheduleTickLoop();
    },
    [levelMatchAb, abDryMatchGainDb, scheduleTickLoop],
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
      const clamped = Math.max(0, Math.min(Math.max(0, buf.duration - 0.01), offsetS));
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
      activeDurationSRef.current = buf.duration;
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

      scheduleTickLoop();
    },
    [
      getPlaybackBufferForTake,
      stopPlaybackNode,
      ensureAudioContext,
      levelMatchAb,
      abDryMatchGainDb,
      scheduleTickLoop,
    ],
  );

  const togglePlay = useCallback(() => {
    if (!hasSession) return;
    if (isPlayingRef.current) {
      const stoppedAt = readLivePlayheadS();
      stopPlaybackNode();
      playheadRef.current = stoppedAt;
      setPlayheadS(stoppedAt);
      isPlayingRef.current = false;
      setIsPlaying(false);
    } else {
      const activeBuf = getPlaybackBufferForTake(takeModeRef.current);
      const effectiveDur = activeBuf?.duration ?? durationS;
      const startAt = playheadRef.current >= effectiveDur - 0.05 ? 0 : playheadRef.current;
      startPlaybackAt(startAt, takeModeRef.current);
    }
  }, [hasSession, durationS, startPlaybackAt, stopPlaybackNode, readLivePlayheadS, getPlaybackBufferForTake]);

  const seek = useCallback(
    (nextS: number) => {
      const activeBuf = getPlaybackBufferForTake(takeModeRef.current);
      const effectiveDur = Math.max(durationS, activeBuf?.duration ?? 0);
      const clamped = Math.max(0, Math.min(effectiveDur, nextS));
      playheadRef.current = clamped;
      setPlayheadS(clamped);
      if (isPlayingRef.current) {
        startPlaybackAt(clamped, takeModeRef.current);
      }
    },
    [durationS, startPlaybackAt, getPlaybackBufferForTake],
  );

  const switchTake = useCallback(
    (nextTake: TakeMode) => {
      const livePos = readLivePlayheadS();
      takeModeRef.current = nextTake;
      setTakeMode(nextTake);
      if (isPlayingRef.current) {
        startPlaybackAt(livePos, nextTake);
      }
    },
    [startPlaybackAt, readLivePlayheadS],
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
