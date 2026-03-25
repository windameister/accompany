import { create } from "zustand";

export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  modelTier?: "light" | "standard" | "heavy";
  timestamp: number;
}

interface ChatState {
  messages: ChatMessage[];
  isStreaming: boolean;
  streamingContent: string;
  addUserMessage: (content: string) => string;
  startStreaming: () => void;
  appendStreamToken: (token: string) => void;
  finishStreaming: (modelTier: "light" | "standard" | "heavy") => void;
  clearMessages: () => void;
}

let nextId = 0;
const genId = () => `msg_${Date.now()}_${nextId++}`;

export const useChatStore = create<ChatState>((set, get) => ({
  messages: [],
  isStreaming: false,
  streamingContent: "",

  addUserMessage: (content) => {
    const id = genId();
    set((s) => ({
      messages: [
        ...s.messages,
        { id, role: "user", content, timestamp: Date.now() },
      ],
    }));
    return id;
  },

  startStreaming: () => {
    set({ isStreaming: true, streamingContent: "" });
  },

  appendStreamToken: (token) => {
    set((s) => ({
      streamingContent: s.streamingContent + token,
    }));
  },

  finishStreaming: (modelTier) => {
    const content = get().streamingContent;
    set((s) => ({
      isStreaming: false,
      streamingContent: "",
      messages: [
        ...s.messages,
        {
          id: genId(),
          role: "assistant",
          content,
          modelTier,
          timestamp: Date.now(),
        },
      ],
    }));
  },

  clearMessages: () => set({ messages: [], streamingContent: "" }),
}));
