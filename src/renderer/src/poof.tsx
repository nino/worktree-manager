import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";

// MARK: Types

/** A rectangle a vanished element used to occupy, in viewport coordinates. */
export interface PoofAnchor {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** One in-flight cloud, centred on where its element used to be. */
interface Poof {
  id: number;
  x: number;
  y: number;
}

interface PoofContextValue {
  /** Puff a vanishing element away, centred on the rect it last occupied. */
  poof: (anchor: PoofAnchor) => void;
}

const PoofContext = createContext<PoofContextValue | null>(null);

// MARK: The cloud

/**
 * The lobes that make up one cloud: a fat centre ringed by six more. `size` is
 * the lobe diameter in px and `dx`/`dy` place it around the centre — they sit
 * close enough to overlap heavily, which is what lets them blur into a single
 * silhouette instead of a cluster of circles. `spin` swings each lobe's lit
 * spot around so they aren't identical stamps; `delay` staggers them so the
 * cloud billows out from the middle. The cluster itself does the expanding
 * (see `poof-cloud` in styles.css).
 */
const PUFFS = [
  { dx: 0, dy: 0, size: 40, spin: -22, delay: 0 },
  { dx: -15, dy: -9, size: 28, spin: 34, delay: 30 },
  { dx: 15, dy: -10, size: 29, spin: -30, delay: 40 },
  { dx: -9, dy: 11, size: 26, spin: 26, delay: 60 },
  { dx: 11, dy: 12, size: 25, spin: -18, delay: 50 },
  { dx: -21, dy: 2, size: 24, spin: 40, delay: 85 },
  { dx: 21, dy: 1, size: 24, spin: -36, delay: 75 },
];

/**
 * How long a cloud stays mounted. Must outlast the `poof-cloud` animation in
 * styles.css plus the largest lobe delay above.
 */
const POOF_MS = 700;

// MARK: Provider

/**
 * Hosts the puffs of smoke that play when a row is deleted — the Mac OS X
 * "drag an icon off the Dock" poof. Deleted rows unmount the moment the tree
 * refetches, so the cloud can't live inside the row: it is handed a rect and
 * plays out in a fixed, click-through layer over the whole window.
 */
export function PoofProvider({ children }: { children: ReactNode }) {
  const [poofs, setPoofs] = useState<Poof[]>([]);
  const nextId = useRef(1);
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());

  useEffect(() => {
    const pending = timers.current;
    return () => pending.forEach(clearTimeout);
  }, []);

  const poof = useCallback((anchor: PoofAnchor) => {
    const id = nextId.current++;
    setPoofs((ps) => [
      ...ps,
      { id, x: anchor.left + anchor.width / 2, y: anchor.top + anchor.height / 2 },
    ]);
    const timer = setTimeout(() => {
      timers.current.delete(timer);
      setPoofs((ps) => ps.filter((p) => p.id !== id));
    }, POOF_MS);
    timers.current.add(timer);
  }, []);

  const value = useMemo<PoofContextValue>(() => ({ poof }), [poof]);

  return (
    <PoofContext.Provider value={value}>
      {children}
      <div className="poof-layer" aria-hidden="true">
        {poofs.map((p) => (
          <div key={p.id} className="poof" style={{ left: p.x, top: p.y }}>
            {PUFFS.map((puff, i) => (
              <span
                key={i}
                className="poof-puff"
                style={
                  {
                    width: puff.size,
                    height: puff.size,
                    animationDelay: `${puff.delay}ms`,
                    "--x": `${puff.dx}px`,
                    "--y": `${puff.dy}px`,
                    "--spin": `${puff.spin}deg`,
                  } as CSSProperties
                }
              />
            ))}
          </div>
        ))}
      </div>
    </PoofContext.Provider>
  );
}

export function usePoof() {
  const ctx = useContext(PoofContext);
  if (!ctx) throw new Error("usePoof must be used within a PoofProvider");
  return ctx;
}
