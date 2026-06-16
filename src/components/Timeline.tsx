import { useMemo } from "react";
import type { ImageEntry, RatingsMap } from "../types";

interface Props {
  images: ImageEntry[]; // full chronological list
  ratings: RatingsMap;
  binMinutes: number;
  threshold: number; // count images with stars >= threshold per bin
  currentName: string | null;
  onJump: (name: string) => void;
  onBinMinutes: (m: number) => void;
  onThreshold: (t: number) => void;
}

interface Bin {
  total: number; // images captured in this bin
  covered: number; // images meeting the threshold
  handled: number; // images that have been rated or rejected
  firstName: string | null; // first image in the bin (for jump)
}

// Coverage histogram: did the photographer cover the whole event? Bars show the
// count of images at/above the threshold; bins with photos but no keepers (or no
// photos at all) are highlighted as gaps.
export default function Timeline({
  images,
  ratings,
  binMinutes,
  threshold,
  currentName,
  onJump,
  onBinMinutes,
  onThreshold,
}: Props) {
  const { bins, start, binSec, currentBin } = useMemo(() => {
    if (images.length === 0) {
      return { bins: [] as Bin[], start: 0, binSec: 1, currentBin: -1 };
    }
    const start = images[0].captureTime;
    const end = images[images.length - 1].captureTime;
    const binSec = binMinutes * 60;
    const count = Math.max(1, Math.floor((end - start) / binSec) + 1);
    const bins: Bin[] = Array.from({ length: count }, () => ({
      total: 0,
      covered: 0,
      handled: 0,
      firstName: null,
    }));
    for (const img of images) {
      const b = Math.min(count - 1, Math.floor((img.captureTime - start) / binSec));
      const bin = bins[b];
      const r = ratings[img.name];
      bin.total++;
      if (bin.firstName === null) bin.firstName = img.name;
      if ((r?.stars ?? 0) >= threshold) bin.covered++;
      if ((r?.stars ?? 0) > 0 || r?.reject) bin.handled++;
    }
    let currentBin = -1;
    if (currentName) {
      const cur = images.find((i) => i.name === currentName);
      if (cur) currentBin = Math.min(count - 1, Math.floor((cur.captureTime - start) / binSec));
    }
    return { bins, start, binSec, currentBin };
  }, [images, ratings, binMinutes, threshold, currentName]);

  // Track height encodes how many photos were shot in the bin; the fill within
  // it encodes how many are keepers. So height = shooting density, fill = coverage.
  const maxTotal = Math.max(1, ...bins.map((b) => b.total));

  const fmt = (binIndex: number) => {
    const d = new Date((start + binIndex * binSec) * 1000);
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  };

  return (
    <div className="timeline">
      <div className="timeline-controls">
        <label title="Count images at or above this rating as 'covered'">
          ≥
          <select value={threshold} onChange={(e) => onThreshold(Number(e.target.value))}>
            {[1, 2, 3, 4, 5].map((n) => (
              <option key={n} value={n}>
                {n}★
              </option>
            ))}
          </select>
        </label>
        <label title="Time bucket size">
          <select value={binMinutes} onChange={(e) => onBinMinutes(Number(e.target.value))}>
            {[5, 10, 15].map((m) => (
              <option key={m} value={m}>
                {m} min
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className="timeline-plot">
        {/* Ready strip: green dot = all rated, gray dot = none rated, else the
            count of images in the bin still left to rate/reject. */}
        <div className="timeline-ready">
          {bins.map((b, i) => {
            if (b.total === 0) return <div key={i} className="tl-rcell" />;
            const left = b.total - b.handled;
            const title = `${fmt(i)} — ${b.handled}/${b.total} reviewed (${left} left)`;
            if (b.handled === 0) {
              return (
                <div key={i} className="tl-rcell" title={title}>
                  <span className="tl-ready none" />
                </div>
              );
            }
            if (left === 0) {
              return (
                <div key={i} className="tl-rcell" title={title}>
                  <span className="tl-ready done" />
                </div>
              );
            }
            return (
              <div key={i} className="tl-rcell" title={title}>
                <span className="tl-pct">{left}</span>
              </div>
            );
          })}
        </div>
        <div className="timeline-bars">
          {bins.map((b, i) => {
            // Gap = you shot here but have no keepers yet (worth flagging).
            const gap = b.total > 0 && b.covered === 0;
            const trackH = (b.total / maxTotal) * 100;
            const fillH = b.total > 0 ? (b.covered / b.total) * 100 : 0;
            return (
              <div
                key={i}
                className={`tl-bin ${i === currentBin ? "current" : ""}`}
                title={`${fmt(i)} — ${b.covered}/${b.total} keepers · ${b.handled}/${b.total} reviewed`}
                onClick={() => b.firstName && onJump(b.firstName)}
              >
                <div className={`tl-track ${gap ? "gap" : ""}`} style={{ height: `${trackH}%` }}>
                  <div className="tl-fill" style={{ height: `${fillH}%` }} />
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
