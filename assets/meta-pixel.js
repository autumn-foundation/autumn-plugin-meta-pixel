/*! autumn-plugin-meta-pixel loader. Apache-2.0.
 *
 * Reads the config block that MetaPixel::head() renders, makes the fbq stub,
 * and loads fbevents.js. Sends each event with trackSingle to each pixel in
 * the config, so no event goes to a pixel that the app did not configure.
 * Event sources:
 *   - data blocks: <script type="application/json" data-autumn-meta-pixel-event>.
 *     Each block fires once, at load or after an htmx swap.
 *   - the "autumn:meta-pixel" event (an HX-Trigger header). Fires each time.
 *   - a click on an element with data-meta-pixel. Fires each time.
 * The "autumn:meta-pixel-revoke" event (consent withdrawn) stops all of it.
 * No inline script. No eval.
 */
(function (window, document) {
  'use strict';

  var CONFIG_ID = 'autumn-meta-pixel-config';
  var EVENT_SELECTOR = 'script[type="application/json"][data-autumn-meta-pixel-event]';
  var CLICK_SELECTOR = '[data-meta-pixel]';
  var NOSCRIPT_SELECTOR = 'noscript[data-autumn-meta-pixel]';
  var DONE_ATTR = 'data-autumn-meta-pixel-done';
  var HX_EVENT = 'autumn:meta-pixel';
  var REVOKE_EVENT = 'autumn:meta-pixel-revoke';

  // An element with id "autumnMetaPixel" or "fbq" is not a loader or a pixel.
  if (window.autumnMetaPixel && typeof window.autumnMetaPixel.track === 'function') return;

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
  if (!isObject(config) || !Array.isArray(config.pixelIds)) return;
  var pixels = config.pixelIds.filter(function (id) {
    return typeof id === 'string';
  });
  if (pixels.length === 0) return;
  if (config.honorGpc && window.navigator && window.navigator.globalPrivacyControl === true) return;

  // The noscript image is for browsers with no JavaScript. Remove it, so an
  // htmx history snapshot or boosted swap cannot load it as a real image.
  document.querySelectorAll(NOSCRIPT_SELECTOR).forEach(function (el) {
    el.remove();
  });

  // The standard Meta stub: queue calls until fbevents.js loads.
  if (typeof window.fbq !== 'function') {
    var n = (window.fbq = function () {
      if (n.callMethod) n.callMethod.apply(n, arguments);
      else n.queue.push(arguments);
    });
    if (!window._fbq) window._fbq = n;
    n.push = n;
    n.loaded = true;
    n.version = '2.0';
    n.queue = [];
    var s = document.createElement('script');
    s.async = true;
    s.src = config.scriptUrl;
    document.head.appendChild(s);
  }
  var fbq = window.fbq;
  if (config.historyPageViews === false) fbq.disablePushState = true;

  // An error in fbq must not stop the other events.
  function call(args) {
    try {
      fbq.apply(null, args);
    } catch (e) {
      if (window.console) window.console.warn('meta_pixel: fbq failed', e);
    }
  }

  pixels.forEach(function (id) {
    if (config.autoConfig === false) call(['set', 'autoConfig', false, id]);
    call(['init', id]);
  });
  if (config.pageView !== false) {
    pixels.forEach(function (id) {
      call(['trackSingle', id, 'PageView']);
    });
  }

  var revoked = false;

  function track(ev) {
    if (revoked) return;
    if (!isObject(ev) || typeof ev.name !== 'string' || ev.name === '') return;
    var params = isObject(ev.params) ? ev.params : {};
    var method = ev.custom ? 'trackSingleCustom' : 'trackSingle';
    var targets = pixels;
    if (typeof ev.pixelId === 'string') {
      if (pixels.indexOf(ev.pixelId) < 0) return;
      targets = [ev.pixelId];
    }
    targets.forEach(function (id) {
      var args = [method, id, ev.name, params];
      if (typeof ev.eventId === 'string') args.push({ eventID: ev.eventId });
      call(args);
    });
  }

  // Fired blocks, with the text they fired. A morph swap can remove the
  // done attribute but keep the element; the map stops a second fire.
  var fired = new WeakMap();

  function fireBlock(el) {
    var text = el.textContent;
    var seen = fired.get(el);
    fired.set(el, text);
    if (seen === text) return;
    // A done block that is new to this page came from an htmx history
    // snapshot. It fired on an earlier visit.
    if (seen === undefined && el.hasAttribute(DONE_ATTR)) return;
    el.setAttribute(DONE_ATTR, '');
    track(parse(text));
  }

  function scan(root) {
    if (!root || typeof root.querySelectorAll !== 'function') return;
    if (typeof root.matches === 'function' && root.matches(EVENT_SELECTOR)) fireBlock(root);
    root.querySelectorAll(EVENT_SELECTOR).forEach(fireBlock);
  }

  window.autumnMetaPixel = { track: track, scan: scan };

  scan(document);

  document.addEventListener('htmx:load', function (e) {
    scan(e && e.detail && e.detail.elt);
  });

  // Consent withdrawn: the page and fbevents.js can stay in memory (hx-boost).
  document.addEventListener(REVOKE_EVENT, function () {
    if (revoked) return;
    revoked = true;
    fbq.disablePushState = true;
    call(['consent', 'revoke']);
  });

  document.addEventListener(HX_EVENT, function (e) {
    var events = e && e.detail && e.detail.events;
    if (Array.isArray(events)) events.forEach(track);
  });

  // Capture phase: a handler that stops propagation does not hide the click.
  document.addEventListener(
    'click',
    function (e) {
      var target = e && e.target;
      if (!target || typeof target.closest !== 'function') return;
      var el = target.closest(CLICK_SELECTOR);
      if (el) track(parse(el.getAttribute('data-meta-pixel')));
    },
    true
  );
})(window, document);
