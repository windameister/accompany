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

let bubbleTimer: ReturnType<typeof setTimeout> | null = null;

export const useCharacterStore = create<CharacterState>((set) => ({
  mood: "idle",
  isBouncing: false,
  speechBubble: null,

  setMood: (mood) => set({ mood }),
  setBouncing: (isBouncing) => set({ isBouncing }),

  showSpeechBubble: (text, durationMs = 4000) => {
    // Cancel any previous auto-dismiss timer
    if (bubbleTimer) {
      clearTimeout(bubbleTimer);
      bubbleTimer = null;
    }
    set({ speechBubble: text });
    if (durationMs > 0) {
      bubbleTimer = setTimeout(() => {
        set({ speechBubble: null });
        bubbleTimer = null;
      }, durationMs);
    }
  },

  clearSpeechBubble: () => {
    if (bubbleTimer) {
      clearTimeout(bubbleTimer);
      bubbleTimer = null;
    }
    set({ speechBubble: null });
  },
}));
