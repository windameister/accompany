import { useEffect, useState, useCallback } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import CharacterCanvas from "@/components/character/CharacterCanvas";
import SpeechBubble from "@/components/character/SpeechBubble";
import { useCharacterStore } from "@/stores/characterStore";
import { useAudioQueue } from "@/hooks/useAudioPlayer";
import { useSpeechRecognition } from "@/hooks/useSpeechRecognition";
import { useAlwaysListening } from "@/hooks/useAlwaysListening";
import { chatSend, onCharacterMood, onChatToken } from "@/lib/tauri";
import { invoke } from "@tauri-apps/api/core";
import type { CharacterMood } from "@/lib/constants";

function App() {
  const [inputVisible, setInputVisible] = useState(false);
  const [inputText, setInputText] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [alwaysListenEnabled] = useState(true);
  const { setMood, showSpeechBubble, clearSpeechBubble } = useCharacterStore();
  const { stop: stopAudio } = useAudioQueue((playing) => {
    if (!playing) {
      setTimeout(() => {
        setMood("idle");
        clearSpeechBubble();
      }, 1500);
    }
  });

  // Send a message (from text input or voice)
  const sendMessage = useCallback(async (msg: string) => {
    const trimmed = msg.trim();
    if (!trimmed || isLoading) return;

    setInputText("");
    setInputVisible(false);
    setIsLoading(true);
    stopAudio();
    showSpeechBubble("思考中...", 0);

    try {
      const res = await chatSend(trimmed);
      showSpeechBubble(
        res.content.length > 80 ? res.content.slice(0, 77) + "..." : res.content,
        8000,
      );
    } catch (e) {
      showSpeechBubble(`出错了: ${String(e).slice(0, 50)}`, 4000);
      setMood("idle");
    } finally {
      setIsLoading(false);
    }
  }, [isLoading, setMood, showSpeechBubble, stopAudio]);

  // Speech recognition — sends recognized text directly via sendMessage
  const {
    isListening,
    isProcessing: sttProcessing,
    isSupported: sttSupported,
    startListening,
    stopListening,
    error: sttError,
  } = useSpeechRecognition({ onResult: sendMessage });

  // Always-on voice listening with intent classification
  const handleAlwaysOnSpeech = useCallback((text: string, intent: "direct" | "self_talk" | "wake_word") => {
    if (intent === "wake_word" || intent === "direct") {
      // User is talking to the cat girl
      sendMessage(text);
    } else if (intent === "self_talk") {
      // User is talking to themselves — cat girl proactively responds with care
      sendMessage(`[用户在自言自语: "${text}"] 请作为猫娘助手，温柔地回应或关心一下`);
    }
  }, [sendMessage]);

  const { status: listenStatus } = useAlwaysListening({
    onSpeech: handleAlwaysOnSpeech,
    enabled: alwaysListenEnabled,
    paused: isLoading || isListening || sttProcessing,
  });

  // First launch: cat girl initiates conversation naturally
  useEffect(() => {
    let cancelled = false;
    const timer = setTimeout(async () => {
      if (cancelled) return;
      try {
        const enrolled = await invoke<boolean>("voice_is_enrolled");
        if (enrolled) return; // Already enrolled, skip greeting

        setIsLoading(true);
        setMood("happy");
        showSpeechBubble("...", 0);

        const res = await chatSend(
          "[系统指令：这是首次启动，用户还没注册过。请主动做自我介绍，告诉主人你是猫娘桌面助手，然后亲切地问主人叫什么名字。简短自然，2-3句话即可。]"
        );

        showSpeechBubble(
          res.content.length > 80 ? res.content.slice(0, 77) + "..." : res.content,
          15000,
        );
        setIsLoading(false);
      } catch {
        setIsLoading(false);
      }
    }, 2000);

    return () => { cancelled = true; clearTimeout(timer); };
  }, []);

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
      const { message } = event.payload;
      showSpeechBubble(message, 0);
      setMood("alert");
      const win = getCurrentWindow();
      await win.show();
      await win.setFocus();
      setTimeout(() => {
        clearSpeechBubble();
        setMood("idle");
      }, 15000);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [showSpeechBubble, clearSpeechBubble, setMood]);

  // Listen for GitHub notifications
  useEffect(() => {
    const unlisten = listen<string>("github-notify", (event) => {
      showSpeechBubble(event.payload, 6000);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [showSpeechBubble]);

  // Listen for streaming tokens
  useEffect(() => {
    let accumulated = "";
    const unlisten = onChatToken((token) => {
      accumulated += token;
      const display = accumulated.length > 80
        ? "..." + accumulated.slice(-77)
        : accumulated;
      showSpeechBubble(display, 0);
    });
    const unlistenDone = listen("tts-done", () => { accumulated = ""; });
    return () => {
      unlisten.then((fn) => fn());
      unlistenDone.then((fn) => fn());
    };
  }, [showSpeechBubble]);

  // Click-through
  useEffect(() => {
    const win = getCurrentWindow();
    let ignoring = false;

    const isOverCharacter = (x: number, y: number) => {
      const cx = 160, cy = 200;
      const rx = 80, ry = 160;
      const dx = (x - cx) / rx;
      const dy = (y - cy) / ry;
      return dx * dx + dy * dy <= 1.0;
    };

    const handleMouseMove = async (e: MouseEvent) => {
      const isOverUI = (e.target as HTMLElement).closest("[data-no-drag]");
      const overChar = isOverCharacter(e.clientX, e.clientY);
      if (isOverUI || overChar) {
        if (ignoring) { await win.setIgnoreCursorEvents(false); ignoring = false; }
      } else {
        if (!ignoring) { await win.setIgnoreCursorEvents(true); ignoring = true; }
      }
    };

    const handleMouseDown = (e: MouseEvent) => {
      if ((e.target as HTMLElement).closest("[data-no-drag]")) return;
      if (e.button === 0 && !ignoring) win.startDragging();
    };

    const handleMouseLeave = async () => {
      if (ignoring) { await win.setIgnoreCursorEvents(false); ignoring = false; }
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mousedown", handleMouseDown);
    document.addEventListener("mouseleave", handleMouseLeave);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mousedown", handleMouseDown);
      document.removeEventListener("mouseleave", handleMouseLeave);
      if (ignoring) win.setIgnoreCursorEvents(false);
    };
  }, []);

  const handleCharacterClick = useCallback(() => {
    if (isLoading) return;
    setInputVisible((v) => !v);
  }, [isLoading]);

  const handleSend = useCallback(() => {
    sendMessage(inputText);
  }, [inputText, sendMessage]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); }
      if (e.key === "Escape") setInputVisible(false);
    },
    [handleSend],
  );

  // Voice button handlers
  const handleMicDown = useCallback(() => {
    if (isLoading || isListening || sttProcessing) return;
    showSpeechBubble("🎤 在听...", 0);
    startListening();
  }, [isLoading, isListening, sttProcessing, startListening, showSpeechBubble]);

  const handleMicUp = useCallback(() => {
    if (isListening) {
      stopListening();
    }
  }, [isListening, stopListening]);

  return (
    <div className="relative w-full h-full select-none">
      <SpeechBubble />

      <div
        className="w-full h-full cursor-pointer"
        onClick={handleCharacterClick}
      >
        <CharacterCanvas />
      </div>

      {/* Input bar: text + mic button */}
      {inputVisible && (
        <div
          data-no-drag
          className="absolute bottom-3 left-3 right-3 animate-fade-in"
        >
          <div className="flex gap-1.5 bg-white/95 backdrop-blur-md rounded-2xl shadow-lg border border-pink-100 p-1.5">
            {/* Mic button */}
            {sttSupported && (
              <button
                onMouseDown={handleMicDown}
                onMouseUp={handleMicUp}
                onMouseLeave={handleMicUp}
                onTouchStart={handleMicDown}
                onTouchEnd={handleMicUp}
                disabled={isLoading || sttProcessing}
                className={`
                  px-2.5 py-1.5 rounded-xl text-sm transition-all
                  ${isListening
                    ? "bg-red-500 text-white scale-110 animate-pulse"
                    : sttProcessing
                    ? "bg-yellow-400 text-white animate-pulse"
                    : "bg-gray-100 text-gray-500 hover:bg-gray-200"
                  }
                  disabled:opacity-40
                `}
                title="按住说话"
              >
                🎤
              </button>
            )}

            <input
              type="text"
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={sttSupported ? "打字或按住🎤说话" : "跟我说点什么..."}
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
          {sttError && (
            <p className="text-[10px] text-red-500 mt-1 px-2">{sttError}</p>
          )}
        </div>
      )}

      {/* STT processing indicator */}
      {sttProcessing && (
        <div className="absolute top-2 left-2 pointer-events-none">
          <span className="text-[10px] bg-yellow-400/80 text-white px-2 py-0.5 rounded-full animate-pulse">识别中...</span>
        </div>
      )}

      {/* Status indicators (top-right) */}
      <div className="absolute top-2 right-2 flex gap-1 items-center pointer-events-none">
        {/* Always-on listening status */}
        {alwaysListenEnabled && !isListening && !isLoading && (
          <div
            className={`w-2 h-2 rounded-full transition-colors ${
              listenStatus === "recording" ? "bg-red-500 animate-pulse" :
              listenStatus === "processing" ? "bg-yellow-400 animate-pulse" :
              listenStatus === "listening" ? "bg-green-400" :
              "bg-gray-300"
            }`}
            title={`常驻监听: ${listenStatus}`}
          />
        )}

        {/* Push-to-talk recording */}
        {isListening && (
          <div className="w-3 h-3 rounded-full bg-red-500 animate-pulse" />
        )}

        {/* Loading */}
        {isLoading && !isListening && (
          <div className="w-2 h-2 rounded-full bg-pink-400 animate-pulse" />
        )}
      </div>
    </div>
  );
}

export default App;
