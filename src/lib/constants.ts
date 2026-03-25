export type CharacterMood = "idle" | "happy" | "alert" | "sleepy" | "thinking" | "talking";

export const SPRITE_PATHS: Record<CharacterMood, string> = {
  idle: "/sprites/catgirl_idle.png",
  happy: "/sprites/catgirl_happy.png",
  alert: "/sprites/catgirl_alert.png",
  sleepy: "/sprites/catgirl_sleepy.png",
  thinking: "/sprites/catgirl_thinking.png",
  talking: "/sprites/catgirl_talking.png",
};

export const MOOD_TRANSITIONS: Record<CharacterMood, { duration: number }> = {
  idle: { duration: 300 },
  happy: { duration: 200 },
  alert: { duration: 100 },
  sleepy: { duration: 500 },
  thinking: { duration: 300 },
  talking: { duration: 150 },
};
