// TypeScript definitions for Tomoyo Launcher WebView plugins (`window.tomoyo` & `window.utools`).
// Place alongside your WebView entrypoint and reference via `/// <reference path="./tomoyo.d.ts" />`.

export interface TomoyoEnterPayload {
  code: string;
  type: 'text' | 'img' | 'files';
  payload: string;
}

export interface TomoyoDbDoc<T = Record<string, unknown>> {
  _id: string;
  _rev?: string;
  [key: string]: unknown;
  value?: T;
}

export interface TomoyoDbResult {
  id: string;
  rev?: string;
  ok?: boolean;
  error?: boolean;
  message?: string;
}

export interface TomoyoDb {
  put<T extends { _id: string; _rev?: string }>(doc: T): TomoyoDbResult;
  get<T = TomoyoDbDoc>(id: string): (T & { _id: string; _rev?: string }) | null;
  remove(docOrId: string | { _id: string }): TomoyoDbResult;
  allDocs<T = TomoyoDbDoc>(keyPrefixOrIds?: string | string[]): Array<T & { _id: string; _rev?: string }>;
}

export interface TomoyoDbStorage {
  setItem<T = unknown>(key: string, value: T): void;
  getItem<T = unknown>(key: string): T | null;
  removeItem(key: string): void;
}

export interface TomoyoPluginApi {
  readonly version: string;
  readonly platform: 'linux' | 'win32' | 'darwin';
  readonly pluginId: string;
  readonly initialQuery: string;
  isDarkColors(): boolean;
  copyText(text: string): boolean;
  openExternal(url: string): void;
  openPath(path: string): void;
  togglePinTopmost(): void;
  close(): void;
  onEnter(cb: (action: TomoyoEnterPayload) => void): void;
  readonly db: TomoyoDb;
  readonly dbStorage: TomoyoDbStorage;
}

export interface TomoyoUtoolsCompatApi {
  onPluginEnter(cb: (action: TomoyoEnterPayload) => void): void;
  onPluginReady(cb: () => void): void;
  onPluginOut(cb: () => void): void;
  setSubInput(
    onChange: (details: { text: string }) => void,
    placeholder?: string,
    isFocus?: boolean,
  ): boolean;
  removeSubInput(): boolean;
  setSubInputValue(val: string): boolean;
  copyText(text: string): boolean;
  hideMainWindow(): void;
  hideMainWindowPasteText(text: string): boolean;
  outPlugin(): void;
  shellOpenExternal(url: string): void;
  shellOpenPath(path: string): void;
  showNotification(body: string): void;
  isDarkColors(): boolean;
  isWindows(): boolean;
  isMacOs(): boolean;
  isLinux(): boolean;
  readonly db: TomoyoDb;
  readonly dbStorage: TomoyoDbStorage;
}

declare global {
  interface Window {
    readonly tomoyo: TomoyoPluginApi;
    readonly utools?: TomoyoUtoolsCompatApi;
  }
}

export {};
