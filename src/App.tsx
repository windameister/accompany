import { useEffect, useState, useCallback } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import CharacterCanvas from "@/components/character/CharacterCanvas";
import SpeechBubble from "@/components/character/SpeechBubble";
import { useCharacterStore } from "@/stores/characterStore";
import { useAudioQueue } from "@/hooks/useAudioPlayer";
import { chatSend, ttsSpeak, onCharacterMood, onChatToken } from "@/lib/tauri";
import type { CharacterMood } from "@/lib/constants";

function App() {
  const [inputVisible, setInputVisible] = useState(false);
  const [inputText, setInputText] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const { mood, setMood, showSpeechBubble, clearSpeechBubble } = useCharacterStore();

  // Audio queue — plays TTS chunks as they arrive
  const { stop: stopAudio, enqueue: enqueueAudio } = useAudioQueue((playing) => {
    if (!playing) {
      // Audio finished playing
      setTimeout(() => {
        setMood("idle");
        clearSpeechBubble();
      }, 1500);
    }
  });

  // Listen for mood changes from backend
  useEffect(() => {
    const unlisten = onCharacterMood((m) => setMood(m as CharacterMood));
    return () => { unlisten.then((fn) => fn()); };
  }, [setMood]);

  // Listen for Claude approval alerts
  useEffect(() => {
    const unlisten = listen<{
      session_id: string;
      project?: string;
      tool?: string;
      message: string;
      waiting_count: number;
    }>("claude-needs-approval", async (event) => {
      const { message, waiting_count } = event.payload;

      // Show alert bubble
      showSpeechBubble(message, 0);
      setMood("alert");

      // Make window visible
      const win = getCurrentWindow();
      await win.show();
      await win.setFocus();

      // TTS is handled by backend via tts-audio event — no need to call ttsSpeak here

      // Auto-dismiss after 15s if not interacted
      setTimeout(() => {
        clearSpeechBubble();
        setMood("idle");
      }, 15000);
    });

    return () => { unlisten.then((fn) => fn()); };
  }, [showSpeechBubble, clearSpeechBubble, setMood]);

  // Listen for GitHub notifications (success = bubble only, failure = TTS from backend)
  useEffect(() => {
    const unlisten = listen<string>("github-notify", (event) => {
      showSpeechBubble(event.payload, 6000);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [showSpeechBubble]);

  // Listen for streaming tokens → update speech bubble
  useEffect(() => {
    let accumulated = "";
    const unlisten = onChatToken((token) => {
      accumulated += token;
      // Show last ~80 chars
      const display = accumulated.length > 80
        ? "..." + accumulated.slice(-77)
        : accumulated;
      showSpeechBubble(display, 0);
    });

    // Reset accumulator when tts-done fires (response complete)
    const unlistenDone = listen("tts-done", () => {
      accumulated = "";
    });

    return () => {
      unlisten.then((fn) => fn());
      unlistenDone.then((fn) => fn());
    };
  }, [showSpeechBubble]);

  // Window drag
  useEffect(() => {
    const handleMouseDown = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (target.closest("[data-no-drag]")) return;
      if (e.button === 0) getCurrentWindow().startDragging();
    };
    window.addEventListener("mousedown", handleMouseDown);
    return () => window.removeEventListener("mousedown", handleMouseDown);
  }, []);

  const handleCharacterClick = useCallback(() => {
    if (isLoading) return;
    setInputVisible((v) => !v);
  }, [isLoading]);

  const handleSend = useCallback(async () => {
    const msg = inputText.trim();
    if (!msg || isLoading) return;

    setInputText("");
    setInputVisible(false);
    setIsLoading(true);
    stopAudio(); // Stop any previous audio
    showSpeechBubble("思考中...", 0);

    try {
      const res = await chatSend(msg);
      // Response text is shown via streaming tokens + speech bubble
      // Audio is played via tts-audio events + audio queue
      showSpeechBubble(
        res.content.length > 80
          ? res.content.slice(0, 77) + "..."
          : res.content,
        8000,
      );
    } catch (e) {
      showSpeechBubble(`出错了: ${String(e).slice(0, 50)}`, 4000);
      setMood("idle");
    } finally {
      setIsLoading(false);
    }
  }, [inputText, isLoading, setMood, showSpeechBubble, stopAudio]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
      if (e.key === "Escape") {
        setInputVisible(false);
      }
    },
    [handleSend],
  );

  return (
    <div className="relative w-full h-full select-none">
      <SpeechBubble />

      <div
        className="w-full h-full cursor-pointer"
        onClick={handleCharacterClick}
      >
        <CharacterCanvas />
      </div>

      {inputVisible && (
        <div
          data-no-drag
          className="absolute bottom-3 left-3 right-3 animate-fade-in"
        >
          <div className="flex gap-1.5 bg-white/95 backdrop-blur-md rounded-2xl shadow-lg border border-pink-100 p-1.5">
            <input
              type="text"
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="跟我说点什么..."
              autoFocus
              className="flex-1 bg-transparent px-3 py-2 text-sm focus:outline-none placeholder-gray-400"
            />
            <button
              onClick={handleSend}
              disabled={isLoading || !inputText.trim()}
              className="px-3 py-1.5 rounded-xl text-xs font-medium bg-pink-500 text-white hover:bg-pink-600 disabled:opacity-40 transition-colors whitespace-nowrap"
            >
              {isLoading ? "..." : "发送"}
            </button>
          </div>
        </div>
      )}

      {isLoading && (
        <div className="absolute top-2 right-2 pointer-events-none">
          <div className="w-2 h-2 rounded-full bg-pink-400 animate-pulse" />
        </div>
      )}
    </div>
  );
}

export default App;
