import { useEffect, useState } from "react";
import { onEvent, type AppEvent } from "../api";

/** A counter that bumps whenever one of the backend events fires. Use it as an
 *  effect dependency to refetch. */
export function useReloadOn(events: AppEvent[]): number {
  const [tick, setTick] = useState(0);
  const key = events.join(",");
  useEffect(() => {
    const offs = events.map((e) => onEvent(e, () => setTick((n) => n + 1)));
    return () => {
      offs.forEach((p) => p.then((off) => off()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return tick;
}

export function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setV(value), ms);
    return () => clearTimeout(id);
  }, [value, ms]);
  return v;
}
