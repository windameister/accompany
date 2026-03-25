import { useRef, useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

interface TtsChunk {
  seq: number;
  audio: string; // base64
}

/**
 * Audio player that queues TTS chunks and plays them in order.
 * Listens for "tts-audio" events from the backend.
 */
export function useAudioQueue(onPlayStateChange?: (playing: boolean) => void) {
  const queueRef = useRef<string[]>([]); // base64 audio queue
  const isPlayingRef = useRef(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  const playNext = useCallback(() => {
    if (queueRef.current.length === 0) {
      isPlayingRef.current = false;
      onPlayStateChange?.(false);
      return;
    }

    const b64 = queueRef.current.shift()!;
    const blob = base64ToBlob(b64, "audio/mp3");
    const url = URL.createObjectURL(blob);
    const audio = new Audio(url);
    audioRef.current = audio;

    audio.onended = () => {
      URL.revokeObjectURL(url);
      audioRef.current = null;
      playNext(); // Play next chunk
    };

    audio.onerror = () => {
      URL.revokeObjectURL(url);
      audioRef.current = null;
      playNext(); // Skip failed chunk
    };

    audio.play().catch(() => {
      playNext();
    });
  }, [onPlayStateChange]);

  const enqueue = useCallback(
    (base64Audio: string) => {
      queueRef.current.push(base64Audio);

      // Start playing if not already
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
    onPlayStateChange?.(false);
  }, [onPlayStateChange]);

  // Listen for TTS audio chunks from backend
  useEffect(() => {
    const unlisten = listen<TtsChunk>("tts-audio", (event) => {
      enqueue(event.payload.audio);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [enqueue]);

  return { enqueue, stop, isPlayingRef };
}

function base64ToBlob(base64: string, mimeType: string): Blob {
  const byteChars = atob(base64);
  const byteArray = new Uint8Array(byteChars.length);
  for (let i = 0; i < byteChars.length; i++) {
    byteArray[i] = byteChars.charCodeAt(i);
  }
  return new Blob([byteArray], { type: mimeType });
}
