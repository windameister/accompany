import { useRef, useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

interface TtsChunk {
  seq: number;
  audio: string; // base64
  source: string; // "chat" | "alert" | "github" | "brain"
}

/**
 * Audio player using AudioContext for AEC (Acoustic Echo Cancellation) support.
 *
 * By playing audio through AudioContext → MediaStreamDestination, the browser's
 * built-in AEC can recognize it as "local output" and cancel it from the
 * microphone input, preventing the cat girl from hearing herself.
 */
export function useAudioQueue(onPlayStateChange?: (playing: boolean) => void) {
  const queueRef = useRef<TtsChunk[]>([]);
  const isPlayingRef = useRef(false);
  const currentSourceRef = useRef<string>("");
  const audioCtxRef = useRef<AudioContext | null>(null);
  const currentSourceNodeRef = useRef<AudioBufferSourceNode | null>(null);

  // Lazy-init AudioContext (needs user gesture on some browsers)
  const getAudioContext = useCallback(() => {
    if (!audioCtxRef.current) {
      audioCtxRef.current = new AudioContext();
    }
    if (audioCtxRef.current.state === "suspended") {
      audioCtxRef.current.resume();
    }
    return audioCtxRef.current;
  }, []);

  const playNext = useCallback(async () => {
    if (queueRef.current.length === 0) {
      isPlayingRef.current = false;
      currentSourceRef.current = "";
      onPlayStateChange?.(false);
      return;
    }

    const chunk = queueRef.current.shift()!;
    currentSourceRef.current = chunk.source;

    try {
      const ctx = getAudioContext();
      const blob = base64ToBlob(
        chunk.audio,
        chunk.audio.startsWith("UklGR") ? "audio/wav" : "audio/mp3",
      );
      const arrayBuffer = await blob.arrayBuffer();
      const audioBuffer = await ctx.decodeAudioData(arrayBuffer);

      const source = ctx.createBufferSource();
      source.buffer = audioBuffer;

      // Route through AudioContext destination (enables browser AEC)
      source.connect(ctx.destination);
      currentSourceNodeRef.current = source;

      source.onended = () => {
        currentSourceNodeRef.current = null;
        playNext();
      };

      source.start();
    } catch (e) {
      console.warn("Audio playback error:", e);
      currentSourceNodeRef.current = null;
      playNext();
    }
  }, [onPlayStateChange, getAudioContext]);

  const enqueue = useCallback(
    (chunk: TtsChunk) => {
      // Alert/brain sources interrupt ongoing chat audio
      if (chunk.source !== "chat" && currentSourceRef.current === "chat") {
        queueRef.current = queueRef.current.filter((c) => c.source !== "chat");
        if (currentSourceNodeRef.current) {
          try { currentSourceNodeRef.current.stop(); } catch { /* */ }
          currentSourceNodeRef.current = null;
        }
        isPlayingRef.current = false;
      }

      queueRef.current.push(chunk);

      if (!isPlayingRef.current) {
        isPlayingRef.current = true;
        onPlayStateChange?.(true);
        playNext();
      }
    },
    [playNext, onPlayStateChange],
  );

  const stop = useCallback(() => {
    queueRef.current = [];
    if (currentSourceNodeRef.current) {
      try { currentSourceNodeRef.current.stop(); } catch { /* */ }
      currentSourceNodeRef.current = null;
    }
    isPlayingRef.current = false;
    currentSourceRef.current = "";
    onPlayStateChange?.(false);
  }, [onPlayStateChange]);

  // Listen for TTS audio chunks from backend — register once
  const enqueueRef = useRef(enqueue);
  enqueueRef.current = enqueue;

  useEffect(() => {
    const unlisten = listen<TtsChunk>("tts-audio", (event) => {
      enqueueRef.current(event.payload);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  return { stop, isPlayingRef };
}

function base64ToBlob(base64: string, mimeType: string): Blob {
  const byteChars = atob(base64);
  const byteArray = new Uint8Array(byteChars.length);
  for (let i = 0; i < byteChars.length; i++) {
    byteArray[i] = byteChars.charCodeAt(i);
  }
  return new Blob([byteArray], { type: mimeType });
}
