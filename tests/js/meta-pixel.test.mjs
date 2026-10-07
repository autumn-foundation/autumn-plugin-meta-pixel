// Tests for assets/meta-pixel.js. Run: node --test tests/js/
//
// A small fake DOM gives only what the loader uses. Each test runs the
// loader in a fresh VM context.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const LOADER = readFileSync(new URL('../../assets/meta-pixel.js', import.meta.url), 'utf8');
const EVENT_SEL = 'script[type="application/json"][data-autumn-meta-pixel-event]';
const CLICK_SEL = '[data-meta-pixel]';

class El {
  constructor(tag, attrs = {}, text = '') {
    this.tagName = tag.toUpperCase();
    this.attrs = { ...attrs };
    this.textContent = text;
    this.children = [];
    this.parent = null;
  }
  getAttribute(n) { return n in this.attrs ? this.attrs[n] : null; }
  setAttribute(n, v) { this.attrs[n] = String(v); }
  hasAttribute(n) { return n in this.attrs; }
  appendChild(c) { c.parent = this; this.children.push(c); return c; }
  matches(sel) {
    if (sel === EVENT_SEL) {
      return this.tagName === 'SCRIPT'
        && this.attrs.type === 'application/json'
        && this.hasAttribute('data-autumn-meta-pixel-event');
    }
    if (sel === CLICK_SEL) return this.hasAttribute('data-meta-pixel');
    throw new Error(`fake DOM: selector not supported: ${sel}`);
  }
  querySelectorAll(sel) {
    const out = [];
    const walk = (el) => {
      for (const c of el.children) {
        if (c.matches(sel)) out.push(c);
        walk(c);
      }
    };
    walk(this);
    return out;
  }
  closest(sel) {
    for (let el = this; el; el = el.parent) if (el.matches(sel)) return el;
    return null;
  }
}

function makeEnv({ config, gpc, body = [], fbq } = {}) {
  const listeners = {};
  const head = new El('head');
  const bodyEl = new El('body');
  const root = new El('html');
  root.appendChild(head);
  root.appendChild(bodyEl);
  if (config !== undefined) {
    head.appendChild(new El('script', { type: 'application/json', id: 'autumn-meta-pixel-config' },
      typeof config === 'string' ? config : JSON.stringify(config)));
  }
  for (const el of body) bodyEl.appendChild(el);
  const document = {
    head,
    body: bodyEl,
    getElementById: (id) => root.querySelectorAllById(id),
    querySelectorAll: (sel) => root.querySelectorAll(sel),
    createElement: (tag) => new El(tag),
    addEventListener: (type, fn) => { (listeners[type] ||= []).push(fn); },
  };
  root.querySelectorAllById = (id) => {
    let hit = null;
    const walk = (el) => {
      for (const c of el.children) {
        if (!hit && c.attrs.id === id) hit = c;
        walk(c);
      }
    };
    walk(root);
    return hit;
  };
  const window = { document, navigator: { globalPrivacyControl: gpc }, console: { warn() {} } };
  if (fbq) window.fbq = fbq;
  window.window = window;
  const ctx = vm.createContext(window);
  const run = () => vm.runInContext(LOADER, ctx);
  run();
  const dispatch = (type, event) => (listeners[type] || []).forEach((fn) => fn(event));
  // JSON round trip: values from the VM realm have other prototypes.
  const plain = (v) => JSON.parse(JSON.stringify(v));
  const calls = () => (window.fbq ? plain(window.fbq.queue.map((args) => Array.from(args))) : null);
  const injected = () => head.children.filter((c) => c.tagName === 'SCRIPT' && c.src);
  return { window, document, dispatch, calls, injected, run, bodyEl };
}

const CONFIG = {
  pixelIds: ['111', '222'],
  pageView: true,
  historyPageViews: true,
  autoConfig: true,
  honorGpc: true,
  scriptUrl: 'https://connect.facebook.net/en_US/fbevents.js',
};

const eventBlock = (ev) => new El('script',
  { type: 'application/json', 'data-autumn-meta-pixel-event': '' }, JSON.stringify(ev));

test('no config: nothing loads', () => {
  const env = makeEnv();
  assert.equal(env.window.fbq, undefined);
  assert.equal(env.injected().length, 0);
});

test('bad config JSON: nothing loads, no throw', () => {
  const env = makeEnv({ config: '{not json' });
  assert.equal(env.window.fbq, undefined);
  assert.equal(env.injected().length, 0);
});

test('config with no pixels: nothing loads', () => {
  const env = makeEnv({ config: { ...CONFIG, pixelIds: [] } });
  assert.equal(env.window.fbq, undefined);
});

test('base code: init each pixel, PageView, async fbevents.js', () => {
  const env = makeEnv({ config: CONFIG });
  assert.deepEqual(env.calls(), [['init', '111'], ['init', '222'], ['track', 'PageView']]);
  const [s] = env.injected();
  assert.equal(s.src, CONFIG.scriptUrl);
  assert.equal(s.async, true);
  assert.equal(env.window._fbq, env.window.fbq);
  assert.equal(env.window.fbq.version, '2.0');
  assert.equal(env.window.fbq.loaded, true);
  assert.notEqual(env.window.fbq.disablePushState, true);
});

test('pageView off, autoConfig off, history page views off', () => {
  const env = makeEnv({ config: { ...CONFIG, pageView: false, autoConfig: false, historyPageViews: false } });
  assert.deepEqual(env.calls(), [
    ['set', 'autoConfig', false, '111'], ['init', '111'],
    ['set', 'autoConfig', false, '222'], ['init', '222'],
  ]);
  assert.equal(env.window.fbq.disablePushState, true);
});

test('GPC: honored stops the load', () => {
  const env = makeEnv({ config: CONFIG, gpc: true });
  assert.equal(env.window.fbq, undefined);
  assert.equal(env.injected().length, 0);
});

test('GPC: not honored when honorGpc is false', () => {
  const env = makeEnv({ config: { ...CONFIG, honorGpc: false }, gpc: true });
  assert.equal(env.injected().length, 1);
});

test('event blocks fire once, in order, with the right fbq method', () => {
  const env = makeEnv({
    config: CONFIG,
    body: [
      eventBlock({ name: 'Purchase', custom: false, params: { value: 1, currency: 'EUR' }, eventId: 'o-1' }),
      eventBlock({ name: 'Share', custom: true, params: {} }),
      eventBlock({ name: 'Lead', custom: false, params: {}, pixelId: '222' }),
      eventBlock({ name: 'Ping', custom: true, params: { a: 1 }, pixelId: '111', eventId: 'e' }),
    ],
  });
  assert.deepEqual(env.calls().slice(3), [
    ['track', 'Purchase', { value: 1, currency: 'EUR' }, { eventID: 'o-1' }],
    ['trackCustom', 'Share', {}],
    ['trackSingle', '222', 'Lead', {}],
    ['trackSingleCustom', '111', 'Ping', { a: 1 }, { eventID: 'e' }],
  ]);
  // htmx:load on the whole body scans again. Nothing fires twice.
  env.dispatch('htmx:load', { detail: { elt: env.bodyEl } });
  assert.equal(env.calls().length, 7);
});

test('htmx:load fires events in swapped content, also the root itself', () => {
  const env = makeEnv({ config: CONFIG });
  const frag = new El('div');
  frag.appendChild(eventBlock({ name: 'AddToCart', custom: false, params: {} }));
  env.bodyEl.appendChild(frag);
  env.dispatch('htmx:load', { detail: { elt: frag } });
  const lone = eventBlock({ name: 'Lead', custom: false, params: {} });
  env.bodyEl.appendChild(lone);
  env.dispatch('htmx:load', { detail: { elt: lone } });
  env.dispatch('htmx:load', {});
  assert.deepEqual(env.calls().slice(3), [['track', 'AddToCart', {}], ['track', 'Lead', {}]]);
});

test('HX-Trigger event fires each event in detail.events', () => {
  const env = makeEnv({ config: CONFIG });
  env.dispatch('autumn:meta-pixel', {
    detail: { events: [{ name: 'Lead', custom: false, params: {} }, { name: 'X', custom: true, params: {} }] },
  });
  env.dispatch('autumn:meta-pixel', { detail: {} });
  env.dispatch('autumn:meta-pixel', {});
  assert.deepEqual(env.calls().slice(3), [['track', 'Lead', {}], ['trackCustom', 'X', {}]]);
});

test('click on a data-meta-pixel element fires its event', () => {
  const env = makeEnv({ config: CONFIG });
  const btn = new El('button', { 'data-meta-pixel': JSON.stringify({ name: 'Contact', custom: false, params: {} }) });
  const span = new El('span');
  btn.appendChild(span);
  env.bodyEl.appendChild(btn);
  env.dispatch('click', { target: span });
  env.dispatch('click', { target: env.bodyEl });
  env.dispatch('click', { target: { nodeType: 3 } }); // Text node: no closest().
  env.dispatch('click', { target: null });
  assert.deepEqual(env.calls().slice(3), [['track', 'Contact', {}]]);
});

test('bad events are dropped, good ones still fire', () => {
  const env = makeEnv({
    config: CONFIG,
    body: [
      new El('script', { type: 'application/json', 'data-autumn-meta-pixel-event': '' }, '{oops'),
      eventBlock({ custom: false, params: {} }),
      eventBlock({ name: 42, custom: false }),
      eventBlock(null),
      eventBlock({ name: 'Lead', custom: false, params: 'x', eventId: 7, pixelId: 9 }),
      eventBlock({ name: 'Lead', custom: false }),
    ],
  });
  const btn = new El('button', { 'data-meta-pixel': '{nope' });
  env.bodyEl.appendChild(btn);
  env.dispatch('click', { target: btn });
  assert.deepEqual(env.calls().slice(3), [['track', 'Lead', {}], ['track', 'Lead', {}]]);
});

test('public API: autumnMetaPixel.track', () => {
  const env = makeEnv({ config: CONFIG });
  env.window.autumnMetaPixel.track({ name: 'Search', custom: false, params: { search_string: 'x' } });
  assert.deepEqual(env.calls().at(-1), ['track', 'Search', { search_string: 'x' }]);
});

test('second run of the loader does nothing', () => {
  const env = makeEnv({ config: CONFIG });
  env.run();
  assert.equal(env.calls().length, 3);
  assert.equal(env.injected().length, 1);
});

test('app fbq already present: reuse it, no second fbevents.js', () => {
  const seen = [];
  const fbq = (...args) => seen.push(JSON.parse(JSON.stringify(args)));
  fbq.queue = [];
  const env = makeEnv({ config: CONFIG, fbq });
  assert.equal(env.window.fbq, fbq);
  assert.equal(env.injected().length, 0);
  assert.deepEqual(seen, [['init', '111'], ['init', '222'], ['track', 'PageView']]);
});
