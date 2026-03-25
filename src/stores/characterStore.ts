import { create } from "zustand";
import type { CharacterMood } from "@/lib/constants";

interface CharacterState {
  mood: CharacterMood;
  isBouncing: boolean;
  speechBubble: string | null;
  setMood: (mood: CharacterMood) => void;
  setBouncing: (bouncing: boolean) => void;
  showSpeechBubble: (text: string, durationMs?: number) => void;
  clearSpeechBubble: () => void;
}

export const useCharacterStore = create<CharacterState>((set) => ({
  mood: "idle",
  isBouncing: false,
  speechBubble: null,

  setMood: (mood) => set({ mood }),
  setBouncing: (isBouncing) => set({ isBouncing }),

  showSpeechBubble: (text, durationMs = 4000) => {
    set({ speechBubble: text });
    if (durationMs > 0) {
      setTimeout(() => set({ speechBubble: null }), durationMs);
    }
  },

  clearSpeechBubble: () => set({ speechBubble: null }),
}));
