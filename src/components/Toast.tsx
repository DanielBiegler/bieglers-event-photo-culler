import { useEffect, useState } from "react";

export interface ToastMsg {
  id: number;
  text: string;
}

interface Props {
  toast: ToastMsg | null;
}

/**
 * Minimal transient notification: white text on a dark pill in the bottom-left,
 * above the timeline. Fades in, holds, then fades out. The global no-transition
 * rule is intentionally overridden for `.toast` (see styles.css) — this gentle
 * fade is the one place we want it.
 */
export default function Toast({ toast }: Props) {
  const [shown, setShown] = useState<ToastMsg | null>(null);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (!toast) return;
    setShown(toast);
    setVisible(false); // mount at opacity 0 so the fade-in actually transitions
    const showId = setTimeout(() => setVisible(true), 20);
    const hideId = setTimeout(() => setVisible(false), 1900);
    const clearId = setTimeout(() => setShown(null), 2600); // after fade-out
    return () => {
      clearTimeout(showId);
      clearTimeout(hideId);
      clearTimeout(clearId);
    };
  }, [toast]);

  if (!shown) return null;
  return (
    <div className={`toast ${visible ? "show" : ""}`} role="status">
      {shown.text}
    </div>
  );
}
