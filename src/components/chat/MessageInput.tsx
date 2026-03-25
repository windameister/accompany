import { useState, useRef, useEffect } from "react";

interface Props {
  onSend: (message: string) => void;
  disabled: boolean;
}

export default function MessageInput({ onSend, disabled }: Props) {
  const [input, setInput] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, [disabled]);

  const handleSubmit = () => {
    const trimmed = input.trim();
    if (!trimmed || disabled) return;
    onSend(trimmed);
    setInput("");
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  return (
    <div className="border-t border-pink-50 p-2">
      <div className="flex items-end gap-2">
        <textarea
          ref={inputRef}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={disabled}
          placeholder="说点什么..."
          rows={1}
          className="
            flex-1 resize-none rounded-xl border border-gray-200
            px-3 py-2 text-sm
            focus:outline-none focus:border-pink-300
            disabled:opacity-50
            max-h-20
          "
        />
        <button
          onClick={handleSubmit}
          disabled={disabled || !input.trim()}
          className="
            px-3 py-2 rounded-xl text-sm font-medium
            bg-pink-500 text-white
            hover:bg-pink-600 active:bg-pink-700
            disabled:opacity-40 disabled:cursor-not-allowed
            transition-colors
          "
        >
          发送
        </button>
      </div>
    </div>
  );
}
