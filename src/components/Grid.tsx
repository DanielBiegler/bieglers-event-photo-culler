import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { ImageEntry, RatingsMap } from "../types";
import Thumbnail from "./Thumbnail";
import { RejectIcon } from "./Icons";

interface Props {
  images: ImageEntry[];
  ratings: RatingsMap;
  currentName: string | null;
  onSelect: (name: string) => void;
  onOpen: (name: string) => void;
}

const CELL = 190; // target cell size in px (incl. gap)
const GAP = 8;

export default function Grid({ images, ratings, currentName, onSelect, onOpen }: Props) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);

  useEffect(() => {
    const el = parentRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    setWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  const cols = Math.max(1, Math.floor((width + GAP) / (CELL + GAP)));
  const rowCount = Math.ceil(images.length / cols);

  const rowVirtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => parentRef.current,
    estimateSize: () => CELL + GAP,
    overscan: 3,
  });

  // Keep the current image visible when it changes from outside the grid.
  const currentIndex = useMemo(
    () => images.findIndex((i) => i.name === currentName),
    [images, currentName]
  );
  useEffect(() => {
    if (currentIndex >= 0 && cols > 0) {
      rowVirtualizer.scrollToIndex(Math.floor(currentIndex / cols), { align: "auto" });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentIndex, cols]);

  return (
    <div ref={parentRef} className="grid-scroll">
      <div style={{ height: rowVirtualizer.getTotalSize(), position: "relative" }}>
        {rowVirtualizer.getVirtualItems().map((vRow) => {
          const start = vRow.index * cols;
          const rowItems = images.slice(start, start + cols);
          return (
            <div
              key={vRow.key}
              className="grid-row"
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                transform: `translateY(${vRow.start}px)`,
                gap: GAP,
              }}
            >
              {rowItems.map((img) => {
                const r = ratings[img.name];
                const active = img.name === currentName;
                return (
                  <div
                    key={img.path}
                    className={`grid-cell ${active ? "active" : ""} ${r?.reject ? "rejected" : ""}`}
                    style={{ width: CELL, height: CELL }}
                    onClick={() => onSelect(img.name)}
                    onDoubleClick={() => onOpen(img.name)}
                  >
                    <Thumbnail path={img.path} />
                    <div className="grid-badge">
                      {r?.reject && <RejectIcon className="badge-reject" />}
                      {r && r.stars > 0 && <span className="badge-stars">{"★".repeat(r.stars)}</span>}
                    </div>
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
}
