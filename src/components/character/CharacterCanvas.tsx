import { useEffect, useRef, useState } from "react";
import { Application, Sprite, Texture, Assets } from "pixi.js";
import { useCharacterStore } from "@/stores/characterStore";
import { SPRITE_PATHS, type CharacterMood } from "@/lib/constants";

const MODEL_PATH = "/live2d/hiyori/hiyori_free_t08.model3.json";

const MOOD_MOTIONS: Record<CharacterMood, string> = {
  idle: "Idle",
  happy: "Tap",
  alert: "Flick",
  sleepy: "Idle",
  thinking: "FlickDown",
  talking: "Tap@Body",
};

async function initLive2D() {
  try {
    // Import cubism runtime first to register it
    await import("@naari3/pixi-live2d-display/cubism");
    // Then import the main module which has Live2DModel
    const { Live2DModel } = await import("@naari3/pixi-live2d-display");

    if (typeof (window as any).Live2DCubismCore === "undefined") {
      console.warn("Live2DCubismCore not on window");
      return null;
    }
    return Live2DModel;
  } catch (e) {
    console.warn("Live2D init failed:", e);
    return null;
  }
}

export default function CharacterCanvas() {
  const canvasRef = useRef<HTMLDivElement>(null);
  const appRef = useRef<Application | null>(null);
  const modelRef = useRef<any>(null);
  const spriteRef = useRef<Sprite | null>(null);
  const texturesRef = useRef<Record<string, Texture>>({});
  const [mode, setMode] = useState<"loading" | "live2d" | "sprite">("loading");
  const mood = useCharacterStore((s) => s.mood);

  useEffect(() => {
    if (!canvasRef.current) return;

    const app = new Application();
    let destroyed = false;

    (async () => {
      await app.init({
        width: 320,
        height: 400,
        backgroundAlpha: 0,
        antialias: true,
        resolution: window.devicePixelRatio || 2,
        autoDensity: true,
      });

      if (destroyed) { app.destroy(true); return; }

      canvasRef.current?.appendChild(app.canvas);
      appRef.current = app;

      // Try Live2D
      console.log("Initializing Live2D...");
      const Live2DModel = await initLive2D();
      if (Live2DModel) {
        try {
          console.log("Loading model from " + MODEL_PATH);
          const model = await Live2DModel.from(MODEL_PATH);
          modelRef.current = model;
          console.log("Model loaded, setting up...");

          const scale = Math.min(
            (app.screen.width * 0.85) / model.width,
            (app.screen.height * 0.85) / model.height,
          );
          model.scale.set(scale);
          model.anchor.set(0.5, 0.5);
          model.x = app.screen.width / 2;
          model.y = app.screen.height / 2;

          app.stage.addChild(model);
          model.motion("Idle", 0);

          setMode("live2d");
          console.log("");
          console.log("Live2D loaded!");
          return;
        } catch (e) {
          const msg = e instanceof Error ? e.message : String(e);
          console.log("Model fail: " + msg);
          console.warn("Live2D model load failed:", e);
        }
      } else {
        console.log("Live2D init returned null");
      }

      // Fallback: sprites
      console.log("Using sprite fallback");
      for (const [key, path] of Object.entries(SPRITE_PATHS)) {
        try { texturesRef.current[key] = await Assets.load(path); } catch {}
      }

      const tex = texturesRef.current["idle"];
      if (tex) {
        const sprite = new Sprite(tex);
        sprite.anchor.set(0.5, 0.5);
        sprite.x = app.screen.width / 2;
        sprite.y = app.screen.height / 2;
        const scale = Math.min(
          (app.screen.width * 0.8) / sprite.texture.width,
          (app.screen.height * 0.8) / sprite.texture.height,
        );
        sprite.scale.set(scale);
        app.stage.addChild(sprite);
        spriteRef.current = sprite;

        const baseY = app.screen.height / 2;
        const baseScale = scale;
        let tick = 0;
        app.ticker.add(() => {
          tick++;
          sprite.scale.set(baseScale + Math.sin(tick * 0.02) * 0.008, baseScale - Math.sin(tick * 0.02) * 0.004);
          sprite.y = baseY + Math.sin(tick * 0.015) * 2;
          sprite.rotation = Math.sin(tick * 0.008) * 0.03;
        });
      }
      setMode("sprite");
    })();

    return () => {
      destroyed = true;
      if (appRef.current) { appRef.current.destroy(true); appRef.current = null; }
      modelRef.current = null;
      spriteRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (mode === "live2d" && modelRef.current) {
      modelRef.current.motion(MOOD_MOTIONS[mood] || "Idle", undefined);
    } else if (mode === "sprite" && spriteRef.current) {
      const tex = texturesRef.current[mood];
      if (tex) spriteRef.current.texture = tex;
    }
  }, [mood, mode]);

  return <div ref={canvasRef} className="w-full h-full" />;
}
