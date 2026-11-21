import { describe, expect, it } from 'vitest';
import bridgeRs from '../../crates/tomoyo-webview-host/src/bridge.rs?raw';
import { hydrateSettingsInit, isPluginEnabled } from '../src/lib/defaults';
import { buildComboFromEvent } from '../src/lib/hotkeys';

const frontendSources = import.meta.glob<string>('../src/**/*.{ts,svelte}', {
  query: '?raw',
  import: 'default',
  eager: true,
});

function parseRustHostMessageCommands(src: string): Set<string> {
  const enumBlock = src.match(/pub enum HostMessage\s*\{([\s\S]*?)\r?\n\}/);
  if (!enumBlock) throw new Error('Failed to locate `pub enum HostMessage` in bridge.rs');
  const cmds = new Set<string>();
  for (const line of enumBlock[1].split(/\r?\n/)) {
    const m = line.match(/^\s+([A-Z][A-Za-z0-9]*)\b/);
    if (m) {
      const snake = m[1].replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase();
      cmds.add(snake);
    }
  }
  return cmds;
}

describe('Rust <-> TypeScript IPC contract & utility resilience', () => {
  it('every frontend postHost({ cmd }) matches a Rust HostMessage variant', () => {
    const rustCmds = parseRustHostMessageCommands(bridgeRs);
    expect(rustCmds.size).toBeGreaterThanOrEqual(40);

    const usedCmds = new Set<string>();
    const unknown: string[] = [];

    for (const [file, code] of Object.entries(frontendSources)) {
      const re = /postHost\s*\(\s*\{\s*cmd\s*:\s*['"]([a-z0-9_]+)['"]/g;
      let match: RegExpExecArray | null;
      while ((match = re.exec(code)) !== null) {
        const cmd = match[1];
        usedCmds.add(cmd);
        if (!rustCmds.has(cmd)) {
          unknown.push(`${file}: ${cmd}`);
        }
      }
    }

    expect(usedCmds.size).toBeGreaterThanOrEqual(30);
    expect(unknown).toEqual([]);
  });

  it('hydrateSettingsInit safely falls back on empty or partial host payloads', () => {
    window.__TOMOYO_SETTINGS_INIT__ = undefined;
    const empty = hydrateSettingsInit();
    expect(empty.config.hotkey).toBe('Alt+Space');
    expect(empty.config.capture.tool_hotkeys.rect).toBe('R');
    expect(empty.config.suspend_hotkeys_in_fullscreen).toBe(true);
    expect(Array.isArray(empty.flows)).toBe(true);
    expect(isPluginEnabled(empty.config, 'dev.tomoyo.ocr')).toBe(true);

    window.__TOMOYO_SETTINGS_INIT__ = {
      config: {
        disabled_plugins: ['DEV.TOMOYO.OCR'],
        suspend_hotkeys_in_fullscreen: false,
        window: { width_dip: 820 },
      },
    };
    const partial = hydrateSettingsInit();
    expect(partial.config.window.width_dip).toBe(820);
    expect(partial.config.window.max_visible_rows).toBe(8);
    expect(partial.config.suspend_hotkeys_in_fullscreen).toBe(false);
    expect(isPluginEnabled(partial.config, 'dev.tomoyo.ocr')).toBe(false);
  });

  it('buildComboFromEvent handles modifiers, function keys, and bare keys consistently', () => {
    expect(buildComboFromEvent(new KeyboardEvent('keydown', { code: 'Space', altKey: true })).combo).toBe('Alt+Space');
    expect(buildComboFromEvent(new KeyboardEvent('keydown', { code: 'KeyV', ctrlKey: true, shiftKey: true })).combo).toBe('Ctrl+Shift+V');
    expect(buildComboFromEvent(new KeyboardEvent('keydown', { code: 'F12' })).combo).toBe('F12');
    expect(buildComboFromEvent(new KeyboardEvent('keydown', { code: 'ControlLeft', ctrlKey: true }))).toEqual({});
    expect(buildComboFromEvent(new KeyboardEvent('keydown', { code: 'Digit1' })).needMod).toBe(true);
  });
});
