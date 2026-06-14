import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { imageSrc } from "../api";
import type { ImageEntry } from "../types";

interface Props {
  images: ImageEntry[];
  /** Index into `images` of the currently displayed photo. */
  index: number;
  zoom: boolean;
  onToggleZoom: () => void;
  onCopy: () => void;
  /** How many neighbours on each side to warm in the browser cache. */
  preload?: number;
}

// Magnification applied on top of native (100%) pixels when zoomed in.
const ZOOM = 1.5;

// Single-image view. Keeps the current image plus a sliding window of neighbours
// mounted (hidden) so arrow-key cycling hits already-decoded frames.
export default function Loupe({ images, index, zoom, onToggleZoom, onCopy, preload = 6 }: Props) {
  const current = images[index];
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const drag = useRef<{ x: number; y: number; px: number; py: number } | null>(null);
  const moved = useRef(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const imgRef = useRef<HTMLImageElement>(null);

  // Reset pan on a new image, and when zooming out. Zooming *in* keeps the pan
  // computed from the click point (see onPointerUp), so don't reset it here.
  useLayoutEffect(() => {
    setPan({ x: 0, y: 0 });
  }, [current?.path]);
  useLayoutEffect(() => {
    if (!zoom) setPan({ x: 0, y: 0 });
  }, [zoom]);

  // Preload neighbours via hidden Image() objects (warms HTTP + decode).
  useEffect(() => {
    const warm: HTMLImageElement[] = [];
    for (let d = -preload; d <= preload; d++) {
      const n = images[index + d];
      if (n) {
        const img = new Image();
        img.src = imageSrc(n.path);
        warm.push(img);
      }
    }
    return () => {
      warm.forEach((i) => (i.src = ""));
    };
  }, [images, index, preload]);

  if (!current) return <div className="loupe empty">No image</div>;

  // Close the context menu on any click, key, or focus loss.
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("keydown", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", close);
      window.removeEventListener("blur", close);
    };
  }, [menu]);

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return; // ignore right/middle click (used for the menu)
    drag.current = { x: e.clientX, y: e.clientY, px: pan.x, py: pan.y };
    moved.current = false;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (!drag.current) return;
    const dx = e.clientX - drag.current.x;
    const dy = e.clientY - drag.current.y;
    if (Math.hypot(dx, dy) > 4) moved.current = true;
    if (zoom) setPan({ x: drag.current.px + dx, y: drag.current.py + dy });
  };
  const onPointerUp = () => {
    // A click without a drag toggles zoom; a drag pans (when zoomed).
    if (drag.current && !moved.current) {
      // Zooming in: pan so the clicked point ends up under the viewport centre.
      // On zoom the element grows from fit-size to natural size, so the click
      // offset must be scaled by (natural / fit) as well as the ZOOM factor.
      if (!zoom && wrapRef.current && imgRef.current) {
        const r = wrapRef.current.getBoundingClientRect();
        const fitW = imgRef.current.getBoundingClientRect().width;
        const ratio = fitW > 0 && imgRef.current.naturalWidth > 0 ? imgRef.current.naturalWidth / fitW : 1;
        const m = ratio * ZOOM;
        const ox = drag.current.x - (r.left + r.width / 2);
        const oy = drag.current.y - (r.top + r.height / 2);
        setPan({ x: -ox * m, y: -oy * m });
      }
      onToggleZoom();
    }
    drag.current = null;
  };

  return (
    <div
      ref={wrapRef}
      className={`loupe ${zoom ? "zoomed" : "fit"}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
    >
      <img
        key={current.path}
        ref={imgRef}
        className="loupe-img"
        src={imageSrc(current.path)}
        draggable={false}
        alt={current.name}
        style={
          zoom
            ? {
                transform: `translate(${pan.x}px, ${pan.y}px) scale(${ZOOM})`,
                maxWidth: "none",
                maxHeight: "none",
              }
            : undefined
        }
      />
      {menu && (
        <div
          className="ctx-menu"
          style={{ left: menu.x, top: menu.y }}
          // Keep clicks inside the menu from reaching the loupe's zoom/pan handlers.
          onPointerDown={(e) => e.stopPropagation()}
          onPointerUp={(e) => e.stopPropagation()}
        >
          <button
            className="ctx-item"
            onClick={() => {
              onCopy();
              setMenu(null);
            }}
          >
            Copy image
          </button>
        </div>
      )}
    </div>
  );
}
