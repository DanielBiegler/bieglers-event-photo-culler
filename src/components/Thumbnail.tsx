import { useEffect, useRef, useState } from "react";
import { getThumbnail } from "../api";

interface Props {
  path: string;
}

// Loads the embedded EXIF thumbnail lazily (only when mounted, i.e. visible in
// the virtualized grid). Results are cached at the api layer.
export default function Thumbnail({ path }: Props) {
  const [src, setSrc] = useState<string | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    setSrc(null);
    getThumbnail(path).then((s) => {
      if (alive.current) setSrc(s);
    });
    return () => {
      alive.current = false;
    };
  }, [path]);

  if (!src) return <div className="thumb-placeholder" />;
  return <img className="thumb-img" src={src} draggable={false} alt="" />;
}
