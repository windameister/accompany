import { useEffect, useRef } from "react";
import type { ChatMessage } from "@/stores/chatStore";

const TIER_LABELS: Record<string, string> = {
  light: "⚡",
  standard: "✨",
  heavy: "🧠",
};

interface Props {
  messages: ChatMessage[];
  isStreaming: boolean;
  streamingContent: string;
}

export default function MessageList({
  messages,
  isStreaming,
  streamingContent,
}: Props) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingContent]);

  return (
    <div className="flex-1 overflow-y-auto px-3 py-2 space-y-2">
      {messages.length === 0 && !isStreaming && (
        <div className="text-center text-gray-400 text-xs mt-8">
          点击发送消息开始聊天喵~
        </div>
      )}

      {messages.map((msg) => (
        <div
          key={msg.id}
          className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}
        >
          <div
            className={`
              max-w-[85%] rounded-2xl px-3 py-2 text-sm leading-relaxed
              ${
                msg.role === "user"
                  ? "bg-pink-500 text-white rounded-br-md"
                  : "bg-gray-100 text-gray-800 rounded-bl-md"
              }
            `}
          >
            <p className="whitespace-pre-wrap break-words">{msg.content}</p>
            {msg.role === "assistant" && msg.modelTier && (
              <span className="text-[10px] text-gray-400 mt-1 block text-right">
                {TIER_LABELS[msg.modelTier] || ""} {msg.modelTier}
              </span>
            )}
          </div>
        </div>
      ))}

      {/* Streaming message */}
      {isStreaming && (
        <div className="flex justify-start">
          <div className="max-w-[85%] rounded-2xl rounded-bl-md px-3 py-2 text-sm bg-gray-100 text-gray-800 leading-relaxed">
            {streamingContent ? (
              <p className="whitespace-pre-wrap break-words">
                {streamingContent}
                <span className="animate-pulse">▊</span>
              </p>
            ) : (
              <span className="text-gray-400 animate-pulse">思考中...</span>
            )}
          </div>
        </div>
      )}

      <div ref={bottomRef} />
    </div>
  );
}
