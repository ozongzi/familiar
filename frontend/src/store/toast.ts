import { useState, useEffect } from "react";

type Toast = { id: number; message: string; type: "error" | "info" };

let nextId = 0;
const listeners = new Set<(toasts: Toast[]) => void>();
let toasts: Toast[] = [];

function notify() {
  listeners.forEach((fn) => fn([...toasts]));
}

export const toast = {
  error(message: string) {
    const id = ++nextId;
    toasts = [...toasts, { id, message, type: "error" }];
    notify();
    setTimeout(() => {
      toasts = toasts.filter((t) => t.id !== id);
      notify();
    }, 5000);
  },
  info(message: string) {
    const id = ++nextId;
    toasts = [...toasts, { id, message, type: "info" }];
    notify();
    setTimeout(() => {
      toasts = toasts.filter((t) => t.id !== id);
      notify();
    }, 3000);
  },
};

export function useToasts() {
  const [state, setState] = useState<Toast[]>([...toasts]);
  useEffect(() => {
    listeners.add(setState);
    return () => { listeners.delete(setState); };
  }, []);
  return state;
}
