import { useCharacterStore } from "@/stores/characterStore";

export default function SpeechBubble() {
  const speechBubble = useCharacterStore((s) => s.speechBubble);

  if (!speechBubble) return null;

  return (
    <div className="absolute top-3 left-3 right-3 z-10 pointer-events-none">
      <div
        className="
          bg-white/95 backdrop-blur-sm
          rounded-2xl px-4 py-2.5
          shadow-lg border border-pink-100
          animate-fade-in
        "
      >
        <p className="text-sm text-gray-700 leading-relaxed break-words">
          {speechBubble}
        </p>
      </div>
    </div>
  );
}
