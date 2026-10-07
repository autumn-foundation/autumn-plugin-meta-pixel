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
const NOSCRIPT_SEL = 'noscript[data-autumn-meta-pixel]';

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
  removeAttribute(n) { delete this.attrs[n]; }
  remove() {
    if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this);
    this.parent = null;
  }
  matches(sel) {
    if (sel === EVENT_SEL) {
      return this.tagName === 'SCRIPT'
        && this.attrs.type === 'application/json'
        && this.hasAttribute('data-autumn-meta-pixel-event');
    }
    if (sel === CLICK_SEL) return this.hasAttribute('data-meta-pixel');
    if (sel === NOSCRIPT_SEL) {
      return this.tagName === 'NOSCRIPT' && this.hasAttribute('data-autumn-meta-pixel');
    }
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

function makeEnv({ config, gpc, body = [], fbq, preset = {} } = {}) {
  const listeners = {};
  const options = {};
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
    addEventListener: (type, fn, opts) => {
      (listeners[type] ||= []).push(fn);
      options[type] = opts;
    },
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
  Object.assign(window, preset);
  window.window = window;
  const ctx = vm.createContext(window);
  const run = () => vm.runInContext(LOADER, ctx);
  run();
  const dispatch = (type, event) => (listeners[type] || []).forEach((fn) => fn(event));
  // JSON round trip: values from the VM realm have other prototypes.
  const plain = (v) => JSON.parse(JSON.stringify(v));
  const calls = () => (window.fbq ? plain(window.fbq.queue.map((args) => Array.from(args))) : null);
  const injected = () => head.children.filter((c) => c.tagName === 'SCRIPT' && c.src);
  return { window, document, dispatch, calls, injected, run, bodyEl, options };
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

test('base code: init each pixel, PageView to each pixel, async fbevents.js', () => {
  const env = makeEnv({ config: CONFIG });
  assert.deepEqual(env.calls(), [
    ['init', '111'], ['init', '222'],
    ['trackSingle', '111', 'PageView'], ['trackSingle', '222', 'PageView'],
  ]);
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

const BASE = 4; // init x2, PageView x2.

test('event blocks fire once, in order, only to configured pixels', () => {
  const env = makeEnv({
    config: CONFIG,
    body: [
      eventBlock({ name: 'Purchase', custom: false, params: { value: 1, currency: 'EUR' }, eventId: 'o-1' }),
      eventBlock({ name: 'Share', custom: true, params: {} }),
      eventBlock({ name: 'Lead', custom: false, params: {}, pixelId: '222' }),
      eventBlock({ name: 'Ping', custom: true, params: { a: 1 }, pixelId: '111', eventId: 'e' }),
      eventBlock({ name: 'Leak', custom: false, params: {}, pixelId: '999' }),
    ],
  });
  assert.deepEqual(env.calls().slice(BASE), [
    ['trackSingle', '111', 'Purchase', { value: 1, currency: 'EUR' }, { eventID: 'o-1' }],
    ['trackSingle', '222', 'Purchase', { value: 1, currency: 'EUR' }, { eventID: 'o-1' }],
    ['trackSingleCustom', '111', 'Share', {}],
    ['trackSingleCustom', '222', 'Share', {}],
    ['trackSingle', '222', 'Lead', {}],
    ['trackSingleCustom', '111', 'Ping', { a: 1 }, { eventID: 'e' }],
  ]);
  // htmx:load on the whole body scans again. Nothing fires twice.
  env.dispatch('htmx:load', { detail: { elt: env.bodyEl } });
  assert.equal(env.calls().length, BASE + 6);
});

test('a morph that drops the done attribute does not fire the block again', () => {
  const block = eventBlock({ name: 'Lead', custom: false, params: {}, pixelId: '111' });
  const env = makeEnv({ config: CONFIG, body: [block] });
  assert.equal(env.calls().length, BASE + 1);
  block.removeAttribute('data-autumn-meta-pixel-done');
  env.dispatch('htmx:load', { detail: { elt: block } });
  assert.equal(env.calls().length, BASE + 1);
  // New content in the same element is a new event.
  block.removeAttribute('data-autumn-meta-pixel-done');
  block.textContent = JSON.stringify({ name: 'Contact', custom: false, params: {}, pixelId: '111' });
  env.dispatch('htmx:load', { detail: { elt: block } });
  assert.deepEqual(env.calls().at(-1), ['trackSingle', '111', 'Contact', {}]);
});

test('htmx:load fires events in swapped content, also the root itself', () => {
  const env = makeEnv({ config: { ...CONFIG, pixelIds: ['111'] } });
  const frag = new El('div');
  frag.appendChild(eventBlock({ name: 'AddToCart', custom: false, params: {} }));
  env.bodyEl.appendChild(frag);
  env.dispatch('htmx:load', { detail: { elt: frag } });
  const lone = eventBlock({ name: 'Lead', custom: false, params: {} });
  env.bodyEl.appendChild(lone);
  env.dispatch('htmx:load', { detail: { elt: lone } });
  env.dispatch('htmx:load', {});
  assert.deepEqual(env.calls().slice(2), [
    ['trackSingle', '111', 'AddToCart', {}], ['trackSingle', '111', 'Lead', {}],
  ]);
});

test('HX-Trigger event fires each event in detail.events', () => {
  const env = makeEnv({ config: { ...CONFIG, pixelIds: ['111'] } });
  env.dispatch('autumn:meta-pixel', {
    detail: { events: [{ name: 'Lead', custom: false, params: {} }, { name: 'X', custom: true, params: {} }] },
  });
  env.dispatch('autumn:meta-pixel', { detail: {} });
  env.dispatch('autumn:meta-pixel', {});
  assert.deepEqual(env.calls().slice(2), [
    ['trackSingle', '111', 'Lead', {}], ['trackSingleCustom', '111', 'X', {}],
  ]);
});

test('click on a data-meta-pixel element fires its event, in the capture phase', () => {
  const env = makeEnv({ config: { ...CONFIG, pixelIds: ['111'] } });
  const btn = new El('button', { 'data-meta-pixel': JSON.stringify({ name: 'Contact', custom: false, params: {} }) });
  const span = new El('span');
  btn.appendChild(span);
  env.bodyEl.appendChild(btn);
  env.dispatch('click', { target: span });
  env.dispatch('click', { target: env.bodyEl });
  env.dispatch('click', { target: { nodeType: 3 } }); // Text node: no closest().
  env.dispatch('click', { target: null });
  assert.deepEqual(env.calls().slice(2), [['trackSingle', '111', 'Contact', {}]]);
  // Capture: a handler that stops propagation does not hide the click.
  assert.equal(env.options.click, true);
});

test('bad events are dropped, good ones still fire', () => {
  const env = makeEnv({
    config: { ...CONFIG, pixelIds: ['111'] },
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
  assert.deepEqual(env.calls().slice(2), [
    ['trackSingle', '111', 'Lead', {}], ['trackSingle', '111', 'Lead', {}],
  ]);
});

test('an fbq error does not stop the other events', () => {
  const seen = [];
  const fbq = (...args) => {
    if (args[2] === 'Bad') throw new Error('boom');
    seen.push(JSON.parse(JSON.stringify(args)));
  };
  const env = makeEnv({
    config: { ...CONFIG, pixelIds: ['111'] },
    fbq,
    body: [
      eventBlock({ name: 'Bad', custom: false, params: {} }),
      eventBlock({ name: 'After', custom: false, params: {} }),
    ],
  });
  env.dispatch('autumn:meta-pixel', {
    detail: { events: [{ name: 'Bad', custom: false, params: {} }, { name: 'AfterHx', custom: false, params: {} }] },
  });
  assert.deepEqual(seen.slice(2).map((c) => c[2]), ['After', 'AfterHx']);
});

test('noscript images of the plugin are removed when JavaScript runs', () => {
  const ns = new El('noscript', { 'data-autumn-meta-pixel': '' });
  const other = new El('noscript');
  const env = makeEnv({ config: CONFIG, body: [ns, other] });
  assert.equal(ns.parent, null);
  assert.equal(other.parent, env.bodyEl);
});

test('public API: autumnMetaPixel.track and scan', () => {
  const env = makeEnv({ config: { ...CONFIG, pixelIds: ['111'] } });
  env.window.autumnMetaPixel.track({ name: 'Search', custom: false, params: { search_string: 'x' } });
  assert.deepEqual(env.calls().at(-1), ['trackSingle', '111', 'Search', { search_string: 'x' }]);
  const frag = new El('div');
  frag.appendChild(eventBlock({ name: 'Lead', custom: false, params: {} }));
  env.window.autumnMetaPixel.scan(frag);
  assert.deepEqual(env.calls().at(-1), ['trackSingle', '111', 'Lead', {}]);
});

test('second run of the loader does nothing', () => {
  const env = makeEnv({ config: CONFIG });
  env.run();
  assert.equal(env.calls().length, BASE);
  assert.equal(env.injected().length, 1);
});

test('app fbq already present: reuse it, no second fbevents.js', () => {
  const seen = [];
  const fbq = (...args) => seen.push(JSON.parse(JSON.stringify(args)));
  fbq.queue = [];
  const env = makeEnv({ config: { ...CONFIG, historyPageViews: false }, fbq });
  assert.equal(env.window.fbq, fbq);
  assert.equal(env.injected().length, 0);
  assert.equal(fbq.disablePushState, true);
  assert.deepEqual(seen, [
    ['init', '111'], ['init', '222'],
    ['trackSingle', '111', 'PageView'], ['trackSingle', '222', 'PageView'],
  ]);
});

test('revoke: consent revoke, no pushState page views, no more events', () => {
  const env = makeEnv({ config: { ...CONFIG, pixelIds: ['111'] } });
  const btn = new El('button', { 'data-meta-pixel': JSON.stringify({ name: 'Contact', custom: false, params: {} }) });
  env.bodyEl.appendChild(btn);
  env.dispatch('autumn:meta-pixel-revoke', {});
  assert.deepEqual(env.calls().at(-1), ['consent', 'revoke']);
  assert.equal(env.window.fbq.disablePushState, true);
  const n = env.calls().length;
  env.dispatch('click', { target: btn });
  env.dispatch('autumn:meta-pixel', { detail: { events: [{ name: 'Lead', custom: false, params: {} }] } });
  const frag = new El('div');
  frag.appendChild(eventBlock({ name: 'Lead', custom: false, params: {} }));
  env.dispatch('htmx:load', { detail: { elt: frag } });
  env.window.autumnMetaPixel.track({ name: 'Lead', custom: false, params: {} });
  assert.equal(env.calls().length, n);
});

test('a DOM element named fbq does not stop the loader', () => {
  // <a id="fbq"> makes window.fbq an element (DOM clobbering).
  const clobber = new El('a', { id: 'fbq' });
  const env = makeEnv({ config: { ...CONFIG, pixelIds: ['111'] }, fbq: clobber });
  assert.equal(typeof env.window.fbq, 'function');
  assert.equal(env.injected().length, 1);
  assert.deepEqual(env.calls(), [['init', '111'], ['trackSingle', '111', 'PageView']]);
});

test('contract: fixture events give the fixture fbq calls', () => {
  const fixture = JSON.parse(readFileSync(new URL('../fixtures/events.json', import.meta.url), 'utf8'));
  for (const c of fixture.cases) {
    const env = makeEnv({ config: { ...CONFIG, pixelIds: fixture.pixelIds }, body: [eventBlock(c.event)] });
    assert.deepEqual(env.calls().slice(2 * fixture.pixelIds.length), c.calls, c.id);
  }
});

test('custom scriptUrl is the injected src', () => {
  const env = makeEnv({ config: { ...CONFIG, scriptUrl: 'https://cdn.example.com/fb.js' } });
  assert.equal(env.injected()[0].src, 'https://cdn.example.com/fb.js');
});

test('a new block that is already done (history snapshot) does not fire', () => {
  const block = eventBlock({ name: 'Lead', custom: false, params: {} });
  block.setAttribute('data-autumn-meta-pixel-done', '');
  const env = makeEnv({ config: { ...CONFIG, pixelIds: ['111'] }, body: [block] });
  assert.equal(env.calls().length, 2);
});

test('an element named autumnMetaPixel does not stop the loader', () => {
  const seen = [];
  const fbq = (...args) => seen.push(args[0]);
  const env = makeEnv({ config: { ...CONFIG, pixelIds: ['111'] }, fbq, preset: { autumnMetaPixel: new El('a') } });
  assert.deepEqual(seen, ['init', 'trackSingle']);
  assert.equal(typeof env.window.autumnMetaPixel.track, 'function');
});

test('a second revoke does nothing', () => {
  const env = makeEnv({ config: { ...CONFIG, pixelIds: ['111'] } });
  env.dispatch('autumn:meta-pixel-revoke', {});
  env.dispatch('autumn:meta-pixel-revoke', {});
  assert.equal(env.calls().filter((c) => c[0] === 'consent').length, 1);
});

test('only string pixel IDs count', () => {
  const env = makeEnv({ config: { ...CONFIG, pixelIds: [7, '111', null] } });
  assert.deepEqual(env.calls(), [['init', '111'], ['trackSingle', '111', 'PageView']]);
  const none = makeEnv({ config: { ...CONFIG, pixelIds: [7] } });
  assert.equal(none.window.fbq, undefined);
});

test('empty names and array params', () => {
  const env = makeEnv({
    config: { ...CONFIG, pixelIds: ['111'] },
    body: [
      eventBlock({ name: '', custom: false, params: {} }),
      eventBlock({ name: 'Lead', custom: false, params: [1, 2] }),
    ],
  });
  assert.deepEqual(env.calls().slice(2), [['trackSingle', '111', 'Lead', {}]]);
});

test('morph then history restore does not fire a block again', () => {
  const block = eventBlock({ name: 'Lead', custom: false, params: {} });
  const env = makeEnv({ config: { ...CONFIG, pixelIds: ['111'] }, body: [block] });
  assert.equal(env.calls().length, 3);
  // Morph: same element, same text, done attribute gone.
  block.removeAttribute('data-autumn-meta-pixel-done');
  env.dispatch('htmx:load', { detail: { elt: block } });
  // History snapshot: a new element with the attributes of the live one.
  const restored = new El('script', { ...block.attrs }, block.textContent);
  env.bodyEl.appendChild(restored);
  env.dispatch('htmx:load', { detail: { elt: restored } });
  assert.equal(env.calls().length, 3);
});
