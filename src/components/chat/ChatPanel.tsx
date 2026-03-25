import { useState, useRef, useEffect } from "react";
import { useChatStore } from "@/stores/chatStore";
import { useCharacterStore } from "@/stores/characterStore";
import { chatSend, onChatToken } from "@/lib/tauri";
import MessageList from "./MessageList";
import MessageInput from "./MessageInput";

export default function ChatPanel({ onClose }: { onClose: () => void }) {
  const {
    messages,
    isStreaming,
    streamingContent,
    addUserMessage,
    startStreaming,
    appendStreamToken,
    finishStreaming,
  } = useChatStore();
  const setMood = useCharacterStore((s) => s.setMood);
  const [error, setError] = useState<string | null>(null);

  // Subscribe to streaming tokens
  useEffect(() => {
    const unlisten = onChatToken((token) => {
      appendStreamToken(token);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [appendStreamToken]);

  const handleSend = async (message: string) => {
    if (isStreaming) return;
    setError(null);

    addUserMessage(message);
    startStreaming();
    setMood("thinking");

    try {
      const response = await chatSend(message);
      finishStreaming(response.model_tier);
      setMood("happy");
      setTimeout(() => setMood("idle"), 2000);
    } catch (e) {
      setError(String(e));
      finishStreaming("light");
      setMood("idle");
    }
  };

  return (
    <div
      data-no-drag
      className="
        absolute inset-0 flex flex-col
        bg-white/95 backdrop-blur-md rounded-2xl
        shadow-2xl border border-pink-100
        overflow-hidden
      "
    >
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-pink-50">
        <h2 className="text-sm font-semibold text-pink-600">和猫娘聊天</h2>
        <button
          onClick={onClose}
          className="text-gray-400 hover:text-gray-600 text-lg leading-none"
        >
          ✕
        </button>
      </div>

      {/* Messages */}
      <MessageList
        messages={messages}
        isStreaming={isStreaming}
        streamingContent={streamingContent}
      />

      {/* Error */}
      {error && (
        <div className="px-4 py-1 text-xs text-red-500 bg-red-50">
          {error}
        </div>
      )}

      {/* Input */}
      <MessageInput onSend={handleSend} disabled={isStreaming} />
    </div>
  );
}
