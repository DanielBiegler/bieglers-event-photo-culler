import type { RejectFilter, StarFilterMode, View } from "../types";
import { FolderIcon, GridIcon, LoupeIcon, ExportIcon, HelpIcon } from "./Icons";

interface Props {
  folderName: string | null;
  view: View;
  minStars: number;
  starFilterMode: StarFilterMode;
  rejectFilter: RejectFilter;
  autoAdvance: boolean;
  shownCount: number;
  totalCount: number;
  position: string;
  onOpen: () => void;
  onView: (v: View) => void;
  onMinStars: (n: number) => void;
  onStarFilterMode: (m: StarFilterMode) => void;
  onRejectFilter: (f: RejectFilter) => void;
  onAutoAdvance: (b: boolean) => void;
  onExport: () => void;
  onHelp: () => void;
}

export default function Toolbar(p: Props) {
  return (
    <div className="toolbar">
      <button className="icon-btn" title="Open folder" onClick={p.onOpen}>
        <FolderIcon />
      </button>
      <span className="folder-name" title={p.folderName ?? ""}>
        {p.folderName ?? "No folder"}
      </span>

      <div className="spacer" />

      <div className="seg">
        <button
          className={`icon-btn ${p.view === "loupe" ? "on" : ""}`}
          title="Loupe (E)"
          onClick={() => p.onView("loupe")}
        >
          <LoupeIcon />
        </button>
        <button
          className={`icon-btn ${p.view === "grid" ? "on" : ""}`}
          title="Grid (G)"
          onClick={() => p.onView("grid")}
        >
          <GridIcon />
        </button>
      </div>

      <label className="ctl" title="Rating filter">
        <select
          value={p.starFilterMode}
          onChange={(e) => p.onStarFilterMode(e.target.value as StarFilterMode)}
        >
          <option value="gte">≥</option>
          <option value="eq">=</option>
        </select>
        <select value={p.minStars} onChange={(e) => p.onMinStars(Number(e.target.value))}>
          <option value={0}>0★</option>
          {[1, 2, 3, 4, 5].map((n) => (
            <option key={n} value={n}>
              {n}★
            </option>
          ))}
        </select>
      </label>

      <label className="ctl" title="Reject filter">
        <select
          value={p.rejectFilter}
          onChange={(e) => p.onRejectFilter(e.target.value as RejectFilter)}
        >
          <option value="all">All</option>
          <option value="hide">Hide rejects</option>
          <option value="only">Only rejects</option>
        </select>
      </label>

      <label className="ctl" title="Auto-advance after rating">
        <input
          type="checkbox"
          checked={p.autoAdvance}
          onChange={(e) => p.onAutoAdvance(e.target.checked)}
        />
        auto
      </label>

      <div className="spacer" />

      <span className="counts">
        <span title="Current image position">{p.position}</span> ·{" "}
        <span title="Shown by current filter / total">
          {p.shownCount}/{p.totalCount}
        </span>
      </span>

      <button className="icon-btn" title="Export keeper CSV" onClick={p.onExport}>
        <ExportIcon />
      </button>
      <button className="icon-btn" title="Keyboard shortcuts (?)" onClick={p.onHelp}>
        <HelpIcon />
      </button>
    </div>
  );
}
