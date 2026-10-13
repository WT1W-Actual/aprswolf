import { useEffect, useRef, useState, type PointerEvent } from "react";

// How long a press must last before it starts selection.
const LONG_PRESS_MS = 500;
// A press that drifts further than this is a scroll or drag, not a hold.
const MOVE_TOLERANCE_PX = 8;

export type Selection = {
  /** True while checkboxes are showing. */
  active: boolean;
  count: number;
  keys: string[];
  isSelected: (key: string) => boolean;
  toggle: (key: string) => void;
  exit: () => void;
  /** Pointer handlers for a row: holding it starts selection with that row checked. */
  pressHandlers: (key: string) => {
    onPointerDown: (event: PointerEvent) => void;
    onPointerMove: (event: PointerEvent) => void;
    onPointerUp: () => void;
    onPointerLeave: () => void;
    onPointerCancel: () => void;
  };
  /**
   * Call first in a row's click handler. True means the click was the end of
   * the long press that started selection, or a toggle while selecting, and
   * the row's usual click action should not run.
   */
  handleClick: (key: string) => boolean;
};

/**
 * Long-press multi-select for a message list. Escape leaves selection unless
 * `paused` (a dialog is open and owns the key).
 */
export function useSelection(paused = false): Selection {
  const [selected, setSelected] = useState<Set<string> | null>(null);
  const timer = useRef<number | null>(null);
  const origin = useRef<{ x: number; y: number } | null>(null);
  const swallowClick = useRef(false);

  const cancelPress = () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
    origin.current = null;
  };

  useEffect(() => cancelPress, []);

  useEffect(() => {
    if (!selected || paused) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setSelected(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selected, paused]);

  const toggle = (key: string) => {
    setSelected((prev) => {
      const next = new Set(prev ?? []);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  return {
    active: selected !== null,
    count: selected?.size ?? 0,
    keys: selected ? Array.from(selected) : [],
    isSelected: (key) => Boolean(selected?.has(key)),
    toggle,
    exit: () => setSelected(null),
    pressHandlers: (key) => ({
      onPointerDown: (event) => {
        swallowClick.current = false;
        if (event.button !== 0 || selected) return;
        origin.current = { x: event.clientX, y: event.clientY };
        timer.current = window.setTimeout(() => {
          timer.current = null;
          swallowClick.current = true;
          setSelected(new Set([key]));
        }, LONG_PRESS_MS);
      },
      onPointerMove: (event) => {
        const start = origin.current;
        if (start && Math.hypot(event.clientX - start.x, event.clientY - start.y) > MOVE_TOLERANCE_PX) {
          cancelPress();
        }
      },
      onPointerUp: cancelPress,
      onPointerLeave: cancelPress,
      onPointerCancel: cancelPress,
    }),
    handleClick: (key) => {
      if (swallowClick.current) {
        swallowClick.current = false;
        return true;
      }
      if (selected) {
        toggle(key);
        return true;
      }
      return false;
    },
  };
}
