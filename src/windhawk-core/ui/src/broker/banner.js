// The runtime-broker banner for the Tauri shell.
//
// This is injected as a Tauri initialization script (see
// `broker::banner_init_script`, wired in `lib.rs`), so it runs ONLY in the Tauri
// app. Windhawk's window normally runs unelevated and issues every privileged
// command through an elevated helper process; when that helper cannot be started
// or is lost, reads keep working and writes fail. This says so, and offers to try
// again - without which a broker-less run looks like a Windhawk where everything
// is mysteriously broken. It also says when the helper is still on its way, for
// the same reason: a window that cannot save anything should say why, whether
// that is permanent or momentary.
//
// WHICH of those to show is the shell's decision, not this file's. Every state
// arrives named (`wh_broker_state` on load, the `wh-broker` event after), and
// this renders the two names that have something to say.
//
// The banner takes a row of its own above the page rather than floating over it:
// the body becomes a column, the banner is its first item, and the element the
// page mounted into gets the rest. A notice that hides the top of the app is a
// notice that costs the user something to read.
//
// It belongs in the shared front-end (windhawk-frontend), which ships from
// another repository on another release cycle. This is the placeholder that does
// not block on it: plain DOM, no framework. What it LOOKS like is that
// front-end's own safe-mode banner - an antd Alert with `banner` set: flat,
// warning-colored, an exclamation icon, and text starting on the same column as
// the app's content (--whui-max-width) - so the eventual component is a swap
// rather than a change of appearance. antd's per-theme values arrive as --wh-*
// tokens (with fallbacks, since the banner can appear before the app has
// rendered anything), which the shell re-pushes when the theme changes.
(function () {
  var BAR_ID = 'wh-broker-banner';
  var STYLE_ID = 'wh-broker-banner-style';
  // On the body while the banner is up: what gives the banner its own row.
  var OPEN_CLASS = 'wh-broker-banner-open';
  // On the element the page mounted into: what gives it the height that leaves.
  var HOST_ATTRIBUTE = 'data-wh-broker-banner-host';
  var SVG_NS = 'http://www.w3.org/2000/svg';
  // antd's ExclamationCircleFilled, the icon a warning Alert carries.
  var ICON_PATH =
    'M512 64c247.4 0 448 200.6 448 448S759.4 960 512 960 64 759.4 64 512 264.6 64 512 64z' +
    'm-32 232v272c0 4.4 3.6 8 8 8h48c4.4 0 8-3.6 8-8V296c0-4.4-3.6-8-8-8h-48c-4.4 0-8 3.6-8 8z' +
    'm32 440a48.01 48.01 0 000-96 48.01 48.01 0 000 96z';
  var current = null; // the last state pushed to us, applied once the DOM exists

  function api() {
    return window.__TAURI__;
  }

  // The shell's commands are only reachable once the Tauri API global is there.
  // It is injected before the page's own scripts, but this file runs before that
  // guarantee holds, so every call retries briefly rather than failing silently.
  function invoke(command, then) {
    var tauri = api();
    if (!tauri) {
      setTimeout(function () {
        invoke(command, then);
      }, 16);
      return;
    }
    try {
      var result = tauri.core.invoke(command);
      if (then && result && typeof result.then === 'function') {
        result.then(then, function () {});
      }
    } catch (e) {}
  }

  function listen(event, handler) {
    var tauri = api();
    if (!tauri) {
      setTimeout(function () {
        listen(event, handler);
      }, 16);
      return;
    }
    try {
      tauri.event.listen(event, function (message) {
        handler(message.payload);
      });
    } catch (e) {}
  }

  // The sizes, colors and easing are antd v4's own (Alert, and Button in its
  // ghost variant), which is what makes this read as the front-end's banner
  // rather than as something bolted on next to it.
  function css() {
    var bar = '#' + BAR_ID;
    var open = 'body.' + OPEN_CLASS;
    var host = open + '>[' + HOST_ATTRIBUTE + ']';
    return [
      // The column the banner and the page share. The page's own viewport-height
      // container is a flex item here, so it shrinks to what the banner leaves
      // instead of hanging its last row off the bottom of the window.
      open + '{display:flex;flex-direction:column;height:100vh;height:100dvh;}',
      open + '>' + bar + '{flex:0 0 auto;}',
      host + '{display:flex;flex-direction:column;flex:1 1 auto;min-height:0;}',
      host + '>*{flex:1 1 auto;min-height:0;}',
      // The Alert itself: a banner Alert drops the border and the radius, and
      // the front-end pads it so its text lines up with the app's content column
      // however wide the window is.
      bar +
        '{box-sizing:border-box;display:flex;align-items:center;' +
        'padding-block:8px;' +
        'padding-inline:calc(20px + max(50% - var(--whui-max-width,1200px)/2,0px));' +
        'font-size:14px;line-height:1.5715;word-wrap:break-word;' +
        'color:var(--wh-fg,#cccccc);background:var(--wh-warning-bg,#2b2111);}',
      bar +
        ' .wh-broker-banner-icon{flex:0 0 auto;display:inline-flex;' +
        'margin-inline-end:8px;color:var(--wh-warning,#d89614);}',
      bar +
        ' .wh-broker-banner-content{flex:1 1 auto;min-width:0;display:flex;' +
        'align-items:center;gap:8px;}',
      bar + ' .wh-broker-banner-text{min-width:0;}',
      bar +
        ' .wh-broker-banner-button{box-sizing:border-box;flex:0 0 auto;height:32px;' +
        'padding:4px 15px;font-family:inherit;font-size:14px;line-height:1.5715;' +
        'white-space:nowrap;cursor:pointer;color:var(--wh-fg,#cccccc);' +
        'background:transparent;border:1px solid var(--wh-border,#454545);' +
        'border-radius:2px;box-shadow:0 2px 0 rgba(0,0,0,.015);' +
        'transition:all .3s cubic-bezier(.645,.045,.355,1);}',
      bar +
        ' .wh-broker-banner-button:hover,' +
        bar +
        ' .wh-broker-banner-button:focus{color:var(--wh-accent,#0078d4);' +
        'border-color:var(--wh-accent,#0078d4);}',
    ].join('');
  }

  function style() {
    if (document.getElementById(STYLE_ID)) {
      return;
    }
    var element = document.createElement('style');
    element.id = STYLE_ID;
    element.textContent = css();
    (document.head || document.documentElement).appendChild(element);
  }

  function bar() {
    var existing = document.getElementById(BAR_ID);
    if (existing) {
      return existing;
    }
    style();
    var element = document.createElement('div');
    element.id = BAR_ID;
    element.setAttribute('role', 'status');
    document.body.insertBefore(element, document.body.firstChild);
    // Whatever the page mounted into is the first thing in the body, and it is
    // the one element that has to be resized: portals the app appends later are
    // empty boxes around positioned popups and take no room of their own.
    var host = element.nextElementSibling;
    if (host) {
      host.setAttribute(HOST_ATTRIBUTE, '');
    }
    document.body.classList.add(OPEN_CLASS);
    return element;
  }

  function icon() {
    var element = document.createElement('span');
    element.className = 'wh-broker-banner-icon';
    element.setAttribute('aria-hidden', 'true');
    var svg = document.createElementNS(SVG_NS, 'svg');
    svg.setAttribute('viewBox', '0 0 1024 1024');
    svg.setAttribute('width', '1em');
    svg.setAttribute('height', '1em');
    svg.setAttribute('fill', 'currentColor');
    var path = document.createElementNS(SVG_NS, 'path');
    path.setAttribute('d', ICON_PATH);
    svg.appendChild(path);
    element.appendChild(svg);
    return element;
  }

  function button(label, onClick) {
    var element = document.createElement('button');
    element.type = 'button';
    element.className = 'wh-broker-banner-button';
    element.textContent = label;
    element.addEventListener('click', onClick);
    return element;
  }

  function remove() {
    var existing = document.getElementById(BAR_ID);
    if (existing) {
      existing.remove();
    }
    if (!document.body) {
      return;
    }
    document.body.classList.remove(OPEN_CLASS);
    var host = document.body.querySelector('[' + HOST_ATTRIBUTE + ']');
    if (host) {
      host.removeAttribute(HOST_ATTRIBUTE);
    }
  }

  function render(state) {
    current = state;
    if (!document.body) {
      return;
    }
    // Two states are worth a banner, and the shell decides which - including
    // when a connect has gone on long enough to be worth mentioning, since the
    // one that finishes behind the splash on an ordinary launch must not flash
    // a notice at anybody. Everything else ("local": a portable or already
    // elevated window, "live": the helper is there, "starting": it is on its
    // way and nobody is waiting yet) shows nothing.
    var name = state && state.state;
    if (name !== 'degraded' && name !== 'connecting') {
      remove();
      return;
    }

    var element = bar();
    element.textContent = '';
    element.appendChild(icon());

    var content = document.createElement('div');
    content.className = 'wh-broker-banner-content';
    element.appendChild(content);

    var text = document.createElement('div');
    text.className = 'wh-broker-banner-text';
    text.textContent =
      name === 'connecting'
        ? 'Windhawk is starting its elevated helper. Changes cannot be saved until it is ready.'
        : 'Windhawk is running without administrator rights, so changes cannot be saved.' +
          (state.reason ? ' (' + state.reason + ')' : '');
    content.appendChild(text);

    // Nothing to retry while a retry is what is happening.
    if (name === 'degraded') {
      content.appendChild(
        button('Try again', function () {
          invoke('wh_broker_retry');
        }),
      );
    }
    content.appendChild(
      button('Dismiss', function () {
        remove();
      }),
    );
  }

  function start() {
    listen('wh-broker', render);
    // The state can have changed - or settled for good - before this page
    // existed, so ask rather than wait for the next change.
    invoke('wh_broker_state', render);
  }

  if (document.body) {
    start();
  } else {
    document.addEventListener('DOMContentLoaded', function () {
      start();
      if (current) {
        render(current);
      }
    });
  }
})();
