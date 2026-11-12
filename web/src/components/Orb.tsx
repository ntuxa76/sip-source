import { ThinkingOrb } from "thinking-orbs";
import { useEffect, useState } from "react";
import type { ReactNode } from "react";

/**
 * What each kind of work looks like. One design per meaning, the same everywhere, so the motion
 * itself tells what the system is doing. The orbs come from thinking-orbs (MIT).
 */
const WORK = {
  /** Fetching a page or panel. */
  load: "working",
  /** Looking through captured calls. */
  search: "searching",
  /** Running SQL. */
  query: "solving",
  /** Following live traffic. */
  stream: "listening",
  /** Reaching another system: storage, sign-in provider, saving. */
  connect: "connecting",
  /** Waiting on something else, such as storage starting up. */
  wait: "breathing",
} as const;

export type Work = keyof typeof WORK;

/**
 * An orb in its exact box, so nothing moves when it appears. `inverse` is for the accent-coloured
 * primary button, whose ground is the opposite of the page's.
 */
export function Orb({ work, size = 20, label, inverse }: { work: Work; size?: 20 | 64; label?: string; inverse?: boolean }) {
  const page = document.documentElement.dataset.theme === "dark" ? "dark" : "light";
  const theme = inverse ? (page === "dark" ? "light" : "dark") : "auto";
  return <ThinkingOrb state={WORK[work]} size={size} theme={theme} aria-label={label} className="shrink-0" style={{ width: size, height: size }} />;
}

/** True after `ms`, so work that finishes quickly never flashes an orb. */
export function useDelayed(active: boolean, ms = 200) {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    if (!active) {
      setShown(false);
      return;
    }
    const t = setTimeout(() => setShown(true), ms);
    return () => clearTimeout(t);
  }, [active, ms]);
  return active && shown;
}

/** A button label that shows an orb in place of the icon while its work runs. */
export function Busy({ active, work, icon, children, inverse }: { active: boolean; work: Work; icon?: ReactNode; children: ReactNode; inverse?: boolean }) {
  const shown = useDelayed(active);
  return (
    <>
      {shown ? <Orb work={work} inverse={inverse} /> : icon}
      {children}
    </>
  );
}
