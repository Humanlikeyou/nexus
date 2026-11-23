import { useSyncExternalStore } from "react";
import { windowsReducer, type WindowsAction } from "./reducer";
import { initialWindowsState, type HomeWindowsState } from "./types";

let state: HomeWindowsState = initialWindowsState();
const listeners = new Set<() => void>();

export const homeWindowsStore = {
  get(): HomeWindowsState { return state; },
  dispatch(action: WindowsAction): HomeWindowsState {
    state = windowsReducer(state, action);
    listeners.forEach((l) => l());
    return state;
  },
  subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  },
  reset(): void {
    state = initialWindowsState();
    listeners.forEach((l) => l());
  },
};

export function useHomeWindowsState(): HomeWindowsState {
  return useSyncExternalStore(homeWindowsStore.subscribe, homeWindowsStore.get, homeWindowsStore.get);
}
