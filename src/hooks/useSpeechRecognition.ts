import { useState, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface UseSpeechRecognitionOptions {
  onResult: (text: string) => void;
}

export function useSpeechRecognition({ onResult }: UseSpeechRecognitionOptions) {
  const [isListening, setIsListening] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);

  const isSupported = typeof MediaRecorder !== "undefined" && !!navigator.mediaDevices;

  const startListening = useCallback(async () => {
    setError(null);

    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const mimeType = MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
        ? "audio/webm;codecs=opus"
        : "audio/mp4";
      const mediaRecorder = new MediaRecorder(stream, { mimeType });

      chunksRef.current = [];

      mediaRecorder.ondataavailable = (e) => {
        if (e.data.size > 0) chunksRef.current.push(e.data);
      };

      mediaRecorder.onstop = async () => {
        stream.getTracks().forEach((t) => t.stop());

        if (chunksRef.current.length === 0) {
          setError("没有录到音频");
          return;
        }

        const blob = new Blob(chunksRef.current, { type: mediaRecorder.mimeType });
        setIsProcessing(true);

        try {
          const base64 = await blobToBase64(blob);
          const text = await invoke<string>("stt_recognize", { audioBase64: base64 });
          if (text) {
            onResult(text);
          } else {
            setError("未识别到语音");
          }
        } catch (e) {
          setError(String(e));
        } finally {
          setIsProcessing(false);
        }
      };

      mediaRecorderRef.current = mediaRecorder;
      mediaRecorder.start();
      setIsListening(true);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.includes("Permission") || msg.includes("NotAllowed")) {
        setError("需要麦克风权限");
      } else {
        setError(`录音失败: ${msg}`);
      }
    }
  }, [onResult]);

  const stopListening = useCallback(() => {
    if (mediaRecorderRef.current && mediaRecorderRef.current.state === "recording") {
      mediaRecorderRef.current.stop();
    }
    setIsListening(false);
  }, []);

  return { isListening, isProcessing, isSupported, startListening, stopListening, error };
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onloadend = () => {
      const result = reader.result as string;
      const base64 = result.split(",")[1];
      base64 ? resolve(base64) : reject(new Error("编码失败"));
    };
    reader.onerror = () => reject(new Error("读取失败"));
    reader.readAsDataURL(blob);
  });
}
