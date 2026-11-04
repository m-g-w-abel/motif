import { describe, expect, it } from 'vitest';
import { addEngine, currentEngine, loadEngines, LOCAL_ID, removeEngine, setToken, socketUrl, switchTo, type SavedEngine } from './engines';

const desk: SavedEngine = { id: 'abc', name: 'Desk', addresses: ['192.168.1.5:7700', '[fe80::1]:7700'], token: 'desk-token' };

describe('engine list', () => {
  it('starts with this device in control', () => {
    const list = loadEngines(null, null);
    expect(list.engines.map((e) => e.id)).toEqual([LOCAL_ID]);
    expect(currentEngine(list).id).toBe(LOCAL_ID);
    expect(currentEngine(list).token).toBeNull();
  });

  it('moves the old single token to this device', () => {
    expect(currentEngine(loadEngines(null, 'old')).token).toBe('old');
  });

  it('round-trips through storage', () => {
    const list = switchTo(addEngine(loadEngines(null, null), desk), 'abc');
    const again = loadEngines(JSON.stringify(list), 'ignored');
    expect(again).toEqual(list);
    expect(currentEngine(again)).toEqual(desk);
  });

  it('recovers from broken storage', () => {
    for (const raw of ['{', '42', '{"engines":[],"current":"x"}', '{"engines":[{"id":"abc"}],"current":"abc"}']) {
      const list = loadEngines(raw, null);
      expect(currentEngine(list).id).toBe(LOCAL_ID);
    }
  });

  it('adding again replaces the entry in place', () => {
    const once = addEngine(loadEngines(null, null), desk);
    const twice = addEngine(once, { ...desk, token: 'new', addresses: ['10.0.0.2:7700'] });
    expect(twice.engines.map((e) => e.id)).toEqual([LOCAL_ID, 'abc']);
    expect(twice.engines[1].token).toBe('new');
    expect(addEngine(once, desk)).toEqual(once);
  });

  it("uses the current engine's token", () => {
    const list = addEngine(loadEngines(null, 'mine'), desk);
    expect(currentEngine(list).token).toBe('mine');
    expect(currentEngine(switchTo(list, 'abc')).token).toBe('desk-token');
    expect(currentEngine(switchTo(switchTo(list, 'abc'), LOCAL_ID)).token).toBe('mine');
  });

  it('sets the token of one engine only', () => {
    const list = setToken(addEngine(loadEngines(null, null), desk), LOCAL_ID, 'paired');
    expect(list.engines.map((e) => e.token)).toEqual(['paired', 'desk-token']);
  });

  it('ignores switching to an engine it does not know', () => {
    const list = loadEngines(null, null);
    expect(switchTo(list, 'nope')).toEqual(list);
  });

  it('removing the engine in control switches back to this device', () => {
    const list = switchTo(addEngine(loadEngines(null, null), desk), 'abc');
    const removed = removeEngine(list, 'abc');
    expect(removed.engines.map((e) => e.id)).toEqual([LOCAL_ID]);
    expect(currentEngine(removed).id).toBe(LOCAL_ID);
    expect(removeEngine(removed, 'abc')).toEqual(removed);
  });

  it('keeps this device', () => {
    const list = loadEngines(null, null);
    expect(removeEngine(list, LOCAL_ID)).toEqual(list);
  });
});

describe('socketUrl', () => {
  it("uses this device's own engine for the local entry", () => {
    expect(socketUrl(currentEngine(loadEngines(null, null)), 3, 'ws://127.0.0.1:7700/ws')).toBe('ws://127.0.0.1:7700/ws');
  });

  it('tries each address in turn', () => {
    expect([0, 1, 2].map((attempt) => socketUrl(desk, attempt, 'unused'))).toEqual([
      'ws://192.168.1.5:7700/ws',
      'ws://[fe80::1]:7700/ws',
      'ws://192.168.1.5:7700/ws',
    ]);
  });
});
