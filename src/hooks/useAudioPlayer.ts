import { useRef, useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

interface TtsChunk {
  seq: number;
  audio: string; // base64
  source: string; // "chat" | "alert" | "github"
}

/**
 * Audio player with priority-based queue.
 * - "alert" and "github" sources interrupt current chat audio
 * - Same-source audio plays in order
 * - Listens for "tts-audio" events from backend
 */
export function useAudioQueue(onPlayStateChange?: (playing: boolean) => void) {
  const queueRef = useRef<TtsChunk[]>([]);
  const isPlayingRef = useRef(false);
  const currentSourceRef = useRef<string>("");
  const audioRef = useRef<HTMLAudioElement | null>(null);

  const playNext = useCallback(() => {
    if (queueRef.current.length === 0) {
      isPlayingRef.current = false;
      currentSourceRef.current = "";
      onPlayStateChange?.(false);
      return;
    }

    const chunk = queueRef.current.shift()!;
    currentSourceRef.current = chunk.source;

    // Auto-detect format: WAV starts with "UklGR" in base64 (RIFF header)
    const mime = chunk.audio.startsWith("UklGR") ? "audio/wav" : "audio/mp3";
    const blob = base64ToBlob(chunk.audio, mime);
    const url = URL.createObjectURL(blob);
    const audio = new Audio(url);
    audioRef.current = audio;

    audio.onended = () => {
      URL.revokeObjectURL(url);
      audioRef.current = null;
      playNext();
    };

    audio.onerror = () => {
      URL.revokeObjectURL(url);
      audioRef.current = null;
      playNext();
    };

    audio.play().catch(() => playNext());
  }, [onPlayStateChange]);

  const enqueue = useCallback(
    (chunk: TtsChunk) => {
      // Alert/github sources interrupt ongoing chat audio
      if (chunk.source !== "chat" && currentSourceRef.current === "chat") {
        queueRef.current = queueRef.current.filter((c) => c.source !== "chat");
        if (audioRef.current) {
          audioRef.current.pause();
          audioRef.current = null;
        }
        // Reset playing state so the new chunk triggers playback
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
    if (audioRef.current) {
      audioRef.current.pause();
      audioRef.current = null;
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

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []); // empty deps — only register once

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
