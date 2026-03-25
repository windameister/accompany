import { useEffect, useRef, useCallback } from "react";
import { Application, Sprite, Texture, Assets } from "pixi.js";
import { useCharacterStore } from "@/stores/characterStore";
import { SPRITE_PATHS, type CharacterMood } from "@/lib/constants";

export default function CharacterCanvas() {
  const canvasRef = useRef<HTMLDivElement>(null);
  const appRef = useRef<Application | null>(null);
  const spriteRef = useRef<Sprite | null>(null);
  const texturesRef = useRef<Record<string, Texture>>({});
  const animFrameRef = useRef<number>(0);
  const mood = useCharacterStore((s) => s.mood);
  const isBouncing = useCharacterStore((s) => s.isBouncing);

  // Initialize PixiJS
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

      if (destroyed) {
        app.destroy(true);
        return;
      }

      canvasRef.current?.appendChild(app.canvas);
      appRef.current = app;

      // Preload all sprites
      const entries = Object.entries(SPRITE_PATHS);
      for (const [moodKey, path] of entries) {
        try {
          const texture = await Assets.load(path);
          texturesRef.current[moodKey] = texture;
        } catch (e) {
          console.warn(`Failed to load sprite: ${path}`, e);
        }
      }

      // Create character sprite
      const initialTexture = texturesRef.current["idle"];
      if (initialTexture) {
        const sprite = new Sprite(initialTexture);
        sprite.anchor.set(0.5, 0.5);
        sprite.x = app.screen.width / 2;
        sprite.y = app.screen.height / 2;

        // Scale to fit
        const scale = Math.min(
          (app.screen.width * 0.8) / sprite.texture.width,
          (app.screen.height * 0.8) / sprite.texture.height
        );
        sprite.scale.set(scale);

        app.stage.addChild(sprite);
        spriteRef.current = sprite;

        // Start idle animation loop
        startIdleAnimation(app, sprite);
      }
    })();

    return () => {
      destroyed = true;
      cancelAnimationFrame(animFrameRef.current);
      if (appRef.current) {
        appRef.current.destroy(true);
        appRef.current = null;
      }
    };
  }, []);

  // Update sprite texture when mood changes
  useEffect(() => {
    const sprite = spriteRef.current;
    const texture = texturesRef.current[mood];
    if (sprite && texture) {
      sprite.texture = texture;
    }
  }, [mood]);

  // Bounce animation when alerting
  useEffect(() => {
    const sprite = spriteRef.current;
    if (!sprite) return;

    if (isBouncing) {
      let frame = 0;
      const animate = () => {
        frame++;
        sprite.y = (appRef.current?.screen.height ?? 200) / 2 + Math.sin(frame * 0.15) * 8;
        animFrameRef.current = requestAnimationFrame(animate);
      };
      animate();

      return () => cancelAnimationFrame(animFrameRef.current);
    } else {
      sprite.y = (appRef.current?.screen.height ?? 200) / 2;
    }
  }, [isBouncing]);

  return <div ref={canvasRef} className="w-full h-full" />;
}

function startIdleAnimation(app: Application, sprite: Sprite) {
  const baseY = app.screen.height / 2;
  const baseScale = sprite.scale.x;
  let tick = 0;

  app.ticker.add(() => {
    tick++;
    // Gentle breathing: subtle scale oscillation
    const breathe = Math.sin(tick * 0.02) * 0.008;
    sprite.scale.set(baseScale + breathe, baseScale - breathe * 0.5);

    // Subtle floating
    sprite.y = baseY + Math.sin(tick * 0.015) * 2;

    // Occasional head tilt
    sprite.rotation = Math.sin(tick * 0.008) * 0.03;
  });
}
