/*! autumn-plugin-meta-pixel loader. Apache-2.0.
 *
 * Reads the config block that MetaPixel::head() renders, sets up fbq, and
 * loads fbevents.js. Fires each event once, from:
 *   - data blocks: <script type="application/json" data-autumn-meta-pixel-event>
 *     (at load and after an htmx swap),
 *   - the "autumn:meta-pixel" event (an HX-Trigger header),
 *   - a click on an element with data-meta-pixel.
 * No inline script. No eval.
 */
(function (window, document) {
  'use strict';

  var CONFIG_ID = 'autumn-meta-pixel-config';
  var EVENT_SELECTOR = 'script[type="application/json"][data-autumn-meta-pixel-event]';
  var CLICK_SELECTOR = '[data-meta-pixel]';
  var DONE_ATTR = 'data-autumn-meta-pixel-done';
  var HX_EVENT = 'autumn:meta-pixel';

  if (window.autumnMetaPixel) return;

  function parse(text) {
    try {
      return JSON.parse(text);
    } catch (e) {
      return null;
    }
  }

  function isObject(v) {
    return v !== null && typeof v === 'object' && !Array.isArray(v);
  }

  var configEl = document.getElementById(CONFIG_ID);
  var config = configEl && parse(configEl.textContent);
  if (!isObject(config) || !Array.isArray(config.pixelIds) || config.pixelIds.length === 0) return;
  if (config.honorGpc && window.navigator && window.navigator.globalPrivacyControl === true) return;

  // The standard Meta stub: queue calls until fbevents.js loads.
  if (!window.fbq) {
    var n = (window.fbq = function () {
      if (n.callMethod) n.callMethod.apply(n, arguments);
      else n.queue.push(arguments);
    });
    if (!window._fbq) window._fbq = n;
    n.push = n;
    n.loaded = true;
    n.version = '2.0';
    n.queue = [];
    if (config.historyPageViews === false) n.disablePushState = true;
    var s = document.createElement('script');
    s.async = true;
    s.src = config.scriptUrl;
    document.head.appendChild(s);
  }
  var fbq = window.fbq;

  config.pixelIds.forEach(function (id) {
    if (typeof id !== 'string') return;
    if (config.autoConfig === false) fbq('set', 'autoConfig', false, id);
    fbq('init', id);
  });
  if (config.pageView !== false) fbq('track', 'PageView');

  function track(ev) {
    if (!isObject(ev) || typeof ev.name !== 'string' || ev.name === '') return;
    var params = isObject(ev.params) ? ev.params : {};
    var args;
    if (typeof ev.pixelId === 'string') {
      args = [ev.custom ? 'trackSingleCustom' : 'trackSingle', ev.pixelId, ev.name, params];
    } else {
      args = [ev.custom ? 'trackCustom' : 'track', ev.name, params];
    }
    if (typeof ev.eventId === 'string') args.push({ eventID: ev.eventId });
    fbq.apply(null, args);
  }

  function fireBlock(el) {
    if (el.hasAttribute(DONE_ATTR)) return;
    el.setAttribute(DONE_ATTR, '');
    track(parse(el.textContent));
  }

  function scan(root) {
    if (!root || typeof root.querySelectorAll !== 'function') return;
    if (typeof root.matches === 'function' && root.matches(EVENT_SELECTOR)) fireBlock(root);
    root.querySelectorAll(EVENT_SELECTOR).forEach(fireBlock);
  }

  window.autumnMetaPixel = { track: track };

  scan(document);

  document.addEventListener('htmx:load', function (e) {
    scan(e && e.detail && e.detail.elt);
  });

  document.addEventListener(HX_EVENT, function (e) {
    var events = e && e.detail && e.detail.events;
    if (Array.isArray(events)) events.forEach(track);
  });

  document.addEventListener('click', function (e) {
    var target = e && e.target;
    if (!target || typeof target.closest !== 'function') return;
    var el = target.closest(CLICK_SELECTOR);
    if (el) track(parse(el.getAttribute('data-meta-pixel')));
  });
})(window, document);
