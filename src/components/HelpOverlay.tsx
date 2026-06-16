interface Props {
  onClose: () => void;
}

const SHORTCUTS: [string, string][] = [
  ["1 – 5", "Set rating"],
  ["0", "Clear rating"],
  ["X", "Toggle reject"],
  ["← / →", "Previous / next (wraps around)"],
  ["N", "Next unrated image (resume culling)"],
  ["Ctrl + ← / →", "Jump to previous / next time-bin (wraps)"],
  ["Q", "Toggle 1.5× zoom"],
  ["G", "Grid view"],
  ["E", "Loupe view"],
  ["Ctrl+C", "Copy image to clipboard"],
  ["?", "Toggle this help"],
  ["Esc", "Close help"],
];

const MOUSE: [string, string][] = [
  ["Click image", "Toggle 1.5× zoom"],
  ["Right-click image", "Copy image to clipboard"],
  ["Drag (zoomed)", "Pan"],
  ["Click thumbnail", "Select"],
  ["Double-click thumbnail", "Open in loupe"],
  ["Click timeline bar", "Jump to that time"],
];

export default function HelpOverlay({ onClose }: Props) {
  return (
    <div className="help-backdrop" onClick={onClose}>
      <div className="help-panel" onClick={(e) => e.stopPropagation()}>
        <div className="help-title">Keyboard shortcuts</div>
        <div className="help-cols">
          <table className="help-table">
            <tbody>
              {SHORTCUTS.map(([k, d]) => (
                <tr key={k}>
                  <td>
                    <kbd>{k}</kbd>
                  </td>
                  <td>{d}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <table className="help-table">
            <tbody>
              {MOUSE.map(([k, d]) => (
                <tr key={k}>
                  <td className="help-mouse">{k}</td>
                  <td>{d}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="help-hint">Press ? or Esc to close</div>
      </div>
    </div>
  );
}
