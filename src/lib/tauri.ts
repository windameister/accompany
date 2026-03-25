import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface ChatResponse {
  content: string;
  model_tier: "light" | "standard" | "heavy";
}

export async function chatSend(message: string): Promise<ChatResponse> {
  return invoke<ChatResponse>("chat_send", { message });
}

export async function chatClear(): Promise<void> {
  return invoke("chat_clear");
}

export async function ttsSpeak(text: string, voice?: string): Promise<string> {
  return invoke<string>("tts_speak", { text, voice });
}

export function onChatToken(callback: (token: string) => void) {
  return listen<string>("chat-token", (event) => {
    callback(event.payload);
  });
}

export function onCharacterMood(callback: (mood: string) => void) {
  return listen<string>("character-mood", (event) => {
    callback(event.payload);
  });
}
