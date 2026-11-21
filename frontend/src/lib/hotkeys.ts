// Maps KeyboardEvent codes to Tomoyo hotkey spec names matching `tomoyo-core::parse_hotkey`.

const CODE_MAP: Record<string, string> = {
  Space: 'Space',
  Tab: 'Tab',
  Insert: 'Insert',
  Delete: 'Delete',
  Home: 'Home',
  End: 'End',
  PageUp: 'PageUp',
  PageDown: 'PageDown',
  PrintScreen: 'PrintScreen',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  Backquote: '`',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
};

const STANDALONE_KEYS = new Set([
  'F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12',
  'PrintScreen',
]);

export function keyEventToName(e: KeyboardEvent): string | null {
  const code = e.code || '';
  if (code in CODE_MAP) return CODE_MAP[code];
  if (code.startsWith('Key') && code.length === 4) return code.slice(3);
  if (code.startsWith('Digit') && code.length === 6) return code.slice(5);
  if (/^F([1-9]|1[0-2])$/.test(code)) return code;
  if (e.key === ' ') return 'Space';
  return null;
}

export function buildComboFromEvent(e: KeyboardEvent): { combo?: string; needMod?: boolean } {
  const mainKey = keyEventToName(e);
  if (!mainKey) return {};
  const mods: string[] = [];
  if (e.ctrlKey) mods.push('Ctrl');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  if (e.metaKey) mods.push('Win');
  if (mods.length === 0) {
    if (STANDALONE_KEYS.has(mainKey)) return { combo: mainKey };
    return { needMod: true };
  }
  // Never bind bare Shift + printable globally (would block typing uppercase letters system-wide).
  if (mods.length === 1 && mods[0] === 'Shift' && !STANDALONE_KEYS.has(mainKey)) {
    return { needMod: true };
  }
  return { combo: [...mods, mainKey].join('+') };
}
