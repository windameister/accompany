import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface UseAlwaysListeningOptions {
  /** Called with recognized text when speech is detected and classified as relevant */
  onSpeech: (text: string, intent: "direct" | "self_talk" | "wake_word") => void;
  /** Whether listening is enabled */
  enabled: boolean;
  /** Don't process voice while this is true (e.g. during TTS playback or chat loading) */
  paused: boolean;
}

// VAD parameters
const VOLUME_THRESHOLD = 0.015;    // Min volume to consider as speech
const SPEECH_START_MS = 400;       // Must be loud for this long to start recording
const SILENCE_END_MS = 1500;       // Silence this long = end of speech
const MIN_SPEECH_MS = 800;         // Ignore very short sounds
const MAX_SPEECH_MS = 30000;       // Max recording length

// Wake words/phrases that bypass intent classification and directly trigger response
const WAKE_WORDS = [
  "猫娘", "喵", "陪伴", "accompany",
  "你好", "哈喽", "hello", "hi",
  "帮我", "请问", "告诉我",
];

export function useAlwaysListening({ onSpeech, enabled, paused }: UseAlwaysListeningOptions) {
  const [isActive, setIsActive] = useState(false);  // Is currently recording a speech segment
  const [status, setStatus] = useState<"idle" | "listening" | "recording" | "processing">("idle");

  const streamRef = useRef<MediaStream | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const rafRef = useRef<number>(0);

  // VAD state
  const loudStartRef = useRef<number>(0);
  const silenceStartRef = useRef<number>(0);
  const recordingStartRef = useRef<number>(0);
  const isRecordingRef = useRef(false);

  // Start/stop the microphone stream
  useEffect(() => {
    if (!enabled) {
      cleanup();
      setStatus("idle");
      return;
    }

    let cancelled = false;

    (async () => {
      try {
        const stream = await navigator.mediaDevices.getUserMedia({
          audio: { echoCancellation: true, noiseSuppression: true },
        });
        if (cancelled) { stream.getTracks().forEach(t => t.stop()); return; }

        streamRef.current = stream;

        const ctx = new AudioContext();
        const source = ctx.createMediaStreamSource(stream);
        const analyser = ctx.createAnalyser();
        analyser.fftSize = 512;
        analyser.smoothingTimeConstant = 0.8;
        source.connect(analyser);
        analyserRef.current = analyser;

        setStatus("listening");
        startVADLoop();
      } catch (e) {
        console.error("Mic access failed:", e);
        setStatus("idle");
      }
    })();

    return () => {
      cancelled = true;
      cleanup();
    };
  }, [enabled]);

  function cleanup() {
    cancelAnimationFrame(rafRef.current);
    if (mediaRecorderRef.current?.state === "recording") {
      mediaRecorderRef.current.stop();
    }
    streamRef.current?.getTracks().forEach(t => t.stop());
    streamRef.current = null;
    analyserRef.current = null;
    isRecordingRef.current = false;
  }

  function getVolume(): number {
    const analyser = analyserRef.current;
    if (!analyser) return 0;
    const data = new Float32Array(analyser.fftSize);
    analyser.getFloatTimeDomainData(data);
    let sum = 0;
    for (let i = 0; i < data.length; i++) sum += data[i] * data[i];
    return Math.sqrt(sum / data.length); // RMS
  }

  function startVADLoop() {
    const tick = () => {
      rafRef.current = requestAnimationFrame(tick);

      if (paused) return; // Skip processing while paused

      const vol = getVolume();
      const now = Date.now();

      if (!isRecordingRef.current) {
        // Not recording: check if speech starts
        if (vol > VOLUME_THRESHOLD) {
          if (loudStartRef.current === 0) loudStartRef.current = now;
          if (now - loudStartRef.current > SPEECH_START_MS) {
            // Speech detected! Start recording
            startRecording();
          }
        } else {
          loudStartRef.current = 0;
        }
      } else {
        // Currently recording: check for silence or max length
        if (vol > VOLUME_THRESHOLD) {
          silenceStartRef.current = 0;
        } else {
          if (silenceStartRef.current === 0) silenceStartRef.current = now;
          if (now - silenceStartRef.current > SILENCE_END_MS) {
            // Silence detected, stop recording
            stopRecording();
          }
        }
        // Max length safety
        if (now - recordingStartRef.current > MAX_SPEECH_MS) {
          stopRecording();
        }
      }
    };
    rafRef.current = requestAnimationFrame(tick);
  }

  function startRecording() {
    const stream = streamRef.current;
    if (!stream || isRecordingRef.current) return;

    const mimeType = MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
      ? "audio/webm;codecs=opus"
      : "audio/mp4";

    const recorder = new MediaRecorder(stream, { mimeType });
    chunksRef.current = [];

    recorder.ondataavailable = (e) => {
      if (e.data.size > 0) chunksRef.current.push(e.data);
    };

    recorder.onstop = () => {
      const duration = Date.now() - recordingStartRef.current;
      if (duration < MIN_SPEECH_MS) {
        // Too short, ignore
        setIsActive(false);
        setStatus("listening");
        return;
      }
      processRecording();
    };

    mediaRecorderRef.current = recorder;
    recorder.start();
    isRecordingRef.current = true;
    recordingStartRef.current = Date.now();
    silenceStartRef.current = 0;
    loudStartRef.current = 0;
    setIsActive(true);
    setStatus("recording");
  }

  function stopRecording() {
    if (mediaRecorderRef.current?.state === "recording") {
      mediaRecorderRef.current.stop();
    }
    isRecordingRef.current = false;
  }

  async function processRecording() {
    setStatus("processing");

    try {
      const blob = new Blob(chunksRef.current, {
        type: mediaRecorderRef.current?.mimeType || "audio/mp4",
      });

      // Convert to base64
      const base64 = await blobToBase64(blob);

      // STT
      const text = await invoke<string>("stt_recognize", { audioBase64: base64 });
      if (!text) {
        setStatus("listening");
        setIsActive(false);
        return;
      }

      console.log("Always-on STT:", text);

      // Check wake words first
      const lower = text.toLowerCase();
      const hasWakeWord = WAKE_WORDS.some(w => lower.includes(w));

      if (hasWakeWord) {
        onSpeech(text, "wake_word");
      } else {
        // Classify intent via backend
        const intent = await invoke<string>("classify_speech_intent", { text });
        if (intent === "direct" || intent === "self_talk") {
          onSpeech(text, intent as "direct" | "self_talk");
        }
        // "ignore" → do nothing
      }
    } catch (e) {
      console.warn("Always-on processing error:", e);
    } finally {
      setStatus("listening");
      setIsActive(false);
    }
  }

  return { status, isActive };
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onloadend = () => {
      const result = reader.result as string;
      const b64 = result.split(",")[1];
      b64 ? resolve(b64) : reject(new Error("encode failed"));
    };
    reader.onerror = () => reject(new Error("read failed"));
    reader.readAsDataURL(blob);
  });
}
