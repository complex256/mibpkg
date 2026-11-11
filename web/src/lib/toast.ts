import { useEffect, useRef, useState } from 'react';

/** A short message shown above the tab bar. */
export interface Toast {
  id: number;
  message: string;
  error: boolean;
}

type Listener = (toasts: Toast[]) => void;

let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<Listener>();

function publish() {
  for (const listener of listeners) {
    listener([...toasts]);
  }
}

/** Show a short message at the bottom of the screen for a few seconds. */
export function toast(message: string, { error = false }: { error?: boolean } = {}) {
  const entry = { id: nextId++, message, error };
  toasts = [...toasts, entry];
  publish();
  setTimeout(() => {
    toasts = toasts.filter((t) => t.id !== entry.id);
    publish();
  }, error ? 6000 : 3000);
}

/** The live toast list (for the Toaster component). */
export function useToasts(): Toast[] {
  const [current, setCurrent] = useState<Toast[]>(toasts);
  useEffect(() => {
    const listener: Listener = (next) => setCurrent(next);
    listeners.add(listener);
    setCurrent([...toasts]);
    return () => {
      listeners.delete(listener);
    };
  }, []);
  return current;
}

/** Clear everything between tests. */
export function resetToasts() {
  toasts = [];
  publish();
}

/** Run something after a delay, cancellable on unmount (React strict mode runs effects twice). */
export function useTimeout(callback: () => void, ms: number | null) {
  const saved = useRef(callback);
  saved.current = callback;
  useEffect(() => {
    if (ms === null) {
      return;
    }
    const timer = setTimeout(() => saved.current(), ms);
    return () => clearTimeout(timer);
  }, [ms]);
}
