import { useEffect, useRef } from "react";
import { useCharacterStore } from "@/stores/characterStore";
import type { CharacterMood } from "@/lib/constants";

// Use global PIXI and Live2DModel loaded via script tags (same as verified test page)
const PIXI = (window as any).PIXI;
const Live2DModel = PIXI?.live2d?.Live2DModel;

const MODEL_PATH = "/live2d/hiyori/hiyori_free_t08.model3.json";

const MOOD_MOTIONS: Record<CharacterMood, string> = {
  idle: "Idle",
  happy: "Tap",
  alert: "Flick",
  sleepy: "Idle",
  thinking: "FlickDown",
  talking: "Tap@Body",
};

export default function CharacterCanvas() {
  const canvasRef = useRef<HTMLDivElement>(null);
  const appRef = useRef<any>(null);
  const modelRef = useRef<any>(null);
  const mood = useCharacterStore((s) => s.mood);

  useEffect(() => {
    if (!canvasRef.current || !PIXI || !Live2DModel) {
      console.error("PIXI or Live2DModel not available on window");
      return;
    }

    const app = new PIXI.Application({
      width: 320,
      height: 400,
      backgroundAlpha: 0,
      antialias: true,
      resolution: window.devicePixelRatio || 2,
      autoDensity: true,
    });

    canvasRef.current.appendChild(app.view as HTMLCanvasElement);
    appRef.current = app;

    let destroyed = false;

    // Register ticker for Live2D auto-update (motions, physics, etc.)
    Live2DModel.registerTicker(PIXI.Ticker);

    (async () => {
      try {
        const model = await Live2DModel.from(MODEL_PATH);
        if (destroyed) {
          model.destroy();
          return;
        }

        modelRef.current = model;

        const scale = Math.min(
          (app.screen.width * 0.85) / model.width,
          (app.screen.height * 0.85) / model.height,
        );
        model.scale.set(scale);
        model.anchor.set(0.5, 0.5);
        model.x = app.screen.width / 2;
        model.y = app.screen.height / 2;

        // Enable mouse/cursor tracking for eye follow
        model.trackedPointers = [0];

        app.stage.addChild(model);
        model.motion("Idle", 0);
      } catch (e) {
        console.error("Live2D model load failed:", e);
      }
    })();

    return () => {
      destroyed = true;
      if (appRef.current) {
        appRef.current.destroy(true);
        appRef.current = null;
      }
      modelRef.current = null;
    };
  }, []);

  useEffect(() => {
    const model = modelRef.current;
    if (!model) return;
    model.motion(MOOD_MOTIONS[mood] || "Idle", undefined);
  }, [mood]);

  return <div ref={canvasRef} className="w-full h-full" />;
}
