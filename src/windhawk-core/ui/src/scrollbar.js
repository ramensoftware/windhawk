// Custom overlay scrollbars for the Tauri shell.
//
// This is injected as a Tauri initialization script (see `shell::scrollbar_init_script`
// wired in `lib.rs`), so it runs ONLY in the Tauri app - the shared front-end keeps its
// host's scrollbars everywhere else (in VSCode the webview supplies flat ::-webkit
// scrollbars itself). WebView2 renders Edge "Fluent" scrollbars (rounded thumb, paddle
// arrows, reserved width) and ignores ::-webkit-scrollbar geometry, so the only way to
// get the flat VSCode look is to hide the native bar and draw our own.
//
// Each scrollable element gets a thin, square thumb that floats over the content
// (position:fixed, so it reserves no width and never shifts the layout) and does not
// auto-hide. The thumb is themed from the --wh-cscroll-* variables the theme shim sets,
// and tracks the container's box and opacity so it fades/scales WITH popups
// instead of flashing over them. The page body uses `overflow:hidden`; the real scroll
// containers are inner React nodes, so we discover them by scanning and re-scan as the
// app mutates. Thumbs live as direct children of <body> (outside React's #root), so
// React never reconciles them away.
//
// Vertical scrollbars only - the layout shift the native bar caused was the vertical one.
//
// Drag and track-paging use pointer events with pointer capture, so they work with
// mouse, touch, and pen and keep receiving moves once the pointer leaves the element. In
// an RTL container the bar sits on the left edge, matching the native vertical scrollbar.
(function () {
  var WIDTH = 10; // thumb thickness in px, matching VSCode's webview bar
  var MIN_THUMB = 20; // minimum thumb length in px

  function ready(fn) {
    if (document.body) {
      fn();
    } else {
      document.addEventListener('DOMContentLoaded', fn);
    }
  }

  function stopEvent(e) {
    e.stopPropagation();
  }

  ready(function () {
    var style = document.createElement('style');
    style.textContent =
      // Hide the native scrollbar on elements we take over (both the standard
      // property and the webkit pseudo, to cover whichever the runtime honors).
      '.wh-cscroll-host{scrollbar-width:none!important;}' +
      '.wh-cscroll-host::-webkit-scrollbar{width:0!important;height:0!important;display:none!important;}' +
      // The track is transparent: it only captures clicks in the empty space for page
      // up/down. It sits under the thumb (the thumb is appended after it). touch-action
      // none so a touch/pen press pages instead of panning the page.
      '.wh-cscroll-track{position:fixed;width:' +
      WIDTH +
      'px;pointer-events:auto;touch-action:none;}' +
      // Flat (no radius), themed from the slider variables; per-thumb opacity is set
      // in layout to match the container. touch-action none so a touch/pen press drags
      // the thumb instead of panning the page.
      '.wh-cscroll-thumb{position:fixed;width:' +
      WIDTH +
      'px;pointer-events:auto;touch-action:none;cursor:default;' +
      'background-color:var(--wh-cscroll-thumb,rgba(121,121,121,.4));}' +
      '.wh-cscroll-thumb:hover{background-color:var(--wh-cscroll-thumb-hover,rgba(100,100,100,.7));}' +
      '.wh-cscroll-thumb.wh-active{background-color:var(--wh-cscroll-thumb-active,rgba(191,191,191,.4));}';
    document.head.appendChild(style);

    var tracked = []; // array of records, one per taken-over scroll container
    var resizeObs = window.ResizeObserver ? new ResizeObserver(scheduleLayout) : null;

    function isScrollable(el) {
      if (el === document.body || el === document.documentElement) {
        return false;
      }
      // Cheap geometry check first: scan walks every element, so this rejects the vast
      // majority (which do not overflow) before the costly getComputedStyle.
      if (el.scrollHeight - el.clientHeight <= 1 || el.clientHeight <= 40) {
        return false;
      }
      var s = getComputedStyle(el);
      // Respect elements the app deliberately renders without a scrollbar.
      if (s.getPropertyValue('scrollbar-width') === 'none') {
        return false;
      }
      var oy = s.overflowY;
      return oy === 'auto' || oy === 'scroll';
    }

    // The z-index a thumb should sit at: the highest z-index among the container's
    // ancestors. A thumb for background content then stays BEHIND a modal overlay
    // (Ant gives the mask a high z-index), while a thumb for a modal's own scroll area
    // sits at the modal's level - and above the modal itself, since it is appended to
    // <body> last and so wins the equal-z-index tie by paint order.
    function stackZ(el) {
      var z = 0;
      for (var p = el; p && p !== document.body; p = p.parentElement) {
        var v = parseInt(getComputedStyle(p).zIndex, 10);
        if (!isNaN(v) && v > z) {
          z = v;
        }
      }
      return z;
    }

    function track(el) {
      el.classList.add('wh-cscroll-host');
      var z = stackZ(el);
      // The track captures clicks in the empty space for page up/down; the thumb is
      // appended after it so it stays on top and handles its own drag.
      var bar = document.createElement('div');
      bar.className = 'wh-cscroll-track';
      bar.style.zIndex = z;
      bar.style.display = 'none';
      document.body.appendChild(bar);
      var thumb = document.createElement('div');
      thumb.className = 'wh-cscroll-thumb';
      thumb.style.zIndex = z;
      thumb.style.display = 'none';
      document.body.appendChild(thumb);

      var dragging = false;
      var startY = 0;
      var startScroll = 0;

      function onDown(e) {
        dragging = true;
        startY = e.clientY;
        startScroll = el.scrollTop;
        thumb.classList.add('wh-active');
        document.body.style.userSelect = 'none';
        thumb.setPointerCapture(e.pointerId);
        e.preventDefault();
        // The thumb lives in <body>, outside the popup it scrolls, so a popup that
        // closes on an outside pointer-down (e.g. an Ant dropdown) would dismiss when
        // the thumb is grabbed. Keep the event from reaching document-level handlers.
        e.stopPropagation();
      }
      function onMove(e) {
        if (!dragging) {
          return;
        }
        var ch = el.clientHeight;
        var sh = el.scrollHeight;
        var th = Math.max((ch * ch) / sh, MIN_THUMB);
        var maxTop = ch - th;
        if (maxTop > 0) {
          el.scrollTop = startScroll + ((e.clientY - startY) / maxTop) * (sh - ch);
        }
      }
      function onUp() {
        if (!dragging) {
          return;
        }
        dragging = false;
        thumb.classList.remove('wh-active');
        document.body.style.userSelect = '';
      }

      // Track click pages toward the cursor by one viewport, smooth so it animates like
      // a native Page Up/Down (the thumb follows via the scroll events the animation
      // emits). While the button is held it auto-repeats, stopping once the thumb
      // reaches the cursor or the scroll hits its end - like a native scrollbar.
      var pageDir = 0;
      var pointerY = 0;
      var pageDelay = null;
      var pageTimer = null;

      function pageReached() {
        var tr = thumb.getBoundingClientRect();
        // The cursor is over the thumb, or the thumb's leading edge passed it (the
        // latter guards against a step overshooting, which would otherwise loop away).
        if (pointerY >= tr.top && pointerY <= tr.bottom) {
          return true;
        }
        return pageDir > 0 ? tr.top >= pointerY : tr.bottom <= pointerY;
      }

      function pageAtEnd() {
        return pageDir > 0
          ? el.scrollTop + el.clientHeight >= el.scrollHeight - 1
          : el.scrollTop <= 0;
      }

      function pageStep() {
        if (pageReached() || pageAtEnd()) {
          stopPaging();
          return;
        }
        el.scrollBy({ top: pageDir * el.clientHeight, behavior: 'smooth' });
      }

      function onPageMove(e) {
        pointerY = e.clientY;
      }

      function stopPaging() {
        if (pageDelay) {
          clearTimeout(pageDelay);
          pageDelay = null;
        }
        if (pageTimer) {
          clearInterval(pageTimer);
          pageTimer = null;
        }
      }

      function onTrackDown(e) {
        var tr = thumb.getBoundingClientRect();
        pageDir = e.clientY < tr.top ? -1 : e.clientY > tr.bottom ? 1 : 0;
        e.preventDefault();
        e.stopPropagation();
        if (!pageDir) {
          return;
        }
        bar.setPointerCapture(e.pointerId);
        pointerY = e.clientY;
        stopPaging();
        pageStep();
        // Initial delay, then auto-repeat, matching a native scrollbar's hold behavior.
        pageDelay = setTimeout(function () {
          pageDelay = null;
          pageTimer = setInterval(pageStep, 250);
        }, 300);
      }

      // Pointer events (capture is taken in the handlers) so drag and paging work with
      // mouse, touch, and pen and keep tracking once the pointer leaves the element.
      thumb.addEventListener('pointerdown', onDown);
      thumb.addEventListener('pointermove', onMove);
      thumb.addEventListener('pointerup', onUp);
      thumb.addEventListener('pointercancel', onUp);
      bar.addEventListener('pointerdown', onTrackDown);
      bar.addEventListener('pointermove', onPageMove);
      bar.addEventListener('pointerup', stopPaging);
      bar.addEventListener('pointercancel', stopPaging);
      // pointerdown is already stopped in the handlers; also swallow the compatibility
      // mouse-down and click so a popup that closes on an outside mouse interaction (an
      // Ant dropdown) is not dismissed. These die with the elements, so no cleanup.
      thumb.addEventListener('mousedown', stopEvent);
      thumb.addEventListener('click', stopEvent);
      bar.addEventListener('mousedown', stopEvent);
      bar.addEventListener('click', stopEvent);
      if (resizeObs) {
        resizeObs.observe(el);
      }

      var rec = {
        el: el,
        thumb: thumb,
        bar: bar,
        // Skip the very first layout: rc-motion applies a popup's animation start state
        // (opacity 0) a frame after mount, so reading opacity now could catch the
        // default 1 and flash. By the next frame the start state is in effect.
        firstFrame: true,
        dispose: function () {
          stopPaging();
          thumb.remove();
          bar.remove();
          if (resizeObs) {
            resizeObs.unobserve(el);
          }
          delete el.__whCScroll;
          el.classList.remove('wh-cscroll-host');
        },
      };
      el.__whCScroll = rec;
      tracked.push(rec);
      // A new container is usually a popup animating in; relayout per-frame so the
      // thumb fades and scales in with it (see the opacity match in layout).
      kickAnimLoop();
    }

    // The container's cumulative opacity (multiplied up the tree) plus whether it is
    // RTL, gathered in one ancestor walk. The thumb lives in <body>, so it is not
    // affected by an ancestor's opacity the way the container is; matching it makes the
    // thumb fade in/out WITH the popup (whose appear/leave motion animates opacity on an
    // ancestor) instead of flashing over a not-yet-visible dropdown. RTL is read off the
    // container's own style in the same walk, so it costs no extra getComputedStyle.
    function containerStyle(el) {
      var opacity = 1;
      var rtl = false;
      for (var p = el; p && p !== document.body; p = p.parentElement) {
        var s = getComputedStyle(p);
        if (p === el) {
          rtl = s.direction === 'rtl';
        }
        var po = parseFloat(s.opacity);
        if (!isNaN(po)) {
          opacity *= po;
        }
      }
      return { opacity: opacity, rtl: rtl };
    }

    function hide(rec) {
      rec.thumb.style.display = 'none';
      rec.bar.style.display = 'none';
    }

    function layout(rec) {
      var el = rec.el;
      var thumb = rec.thumb;
      var bar = rec.bar;
      if (!el.isConnected) {
        return false;
      }
      // React may rewrite className on re-render and drop our host class; reassert it.
      if (!el.classList.contains('wh-cscroll-host')) {
        el.classList.add('wh-cscroll-host');
      }
      if (rec.firstFrame) {
        rec.firstFrame = false;
        hide(rec);
        return true;
      }
      var ch = el.clientHeight;
      var sh = el.scrollHeight;
      if (sh - ch <= 1) {
        hide(rec);
        return true;
      }
      // Match the container's opacity so the thumb fades in/out with a popup instead of
      // flashing over it (RTL comes from the same walk). While effectively invisible,
      // take the thumb and track out entirely so they cannot capture clicks over the
      // not-yet-shown popup.
      var cs = containerStyle(el);
      if (cs.opacity < 0.01) {
        hide(rec);
        return true;
      }
      // Size and place from the RENDERED box (getBoundingClientRect reflects the live
      // transform), so the thumb tracks popups that scale/slide while animating; the
      // visible fraction still comes from the unscaled layout metrics.
      var r = el.getBoundingClientRect();
      var track = r.height;
      var th = Math.max(track * (ch / sh), MIN_THUMB);
      var travel = track - th;
      var top = r.top + (travel > 0 ? (el.scrollTop / (sh - ch)) * travel : 0);
      // In an RTL container the native vertical scrollbar sits on the left, so place
      // ours there too; otherwise the right edge.
      var left = cs.rtl ? r.left : r.right - WIDTH;
      // The track spans the whole visible height (for page up/down); the thumb rides on
      // top of it.
      bar.style.display = 'block';
      bar.style.top = r.top + 'px';
      bar.style.height = track + 'px';
      bar.style.left = left + 'px';
      thumb.style.display = 'block';
      thumb.style.opacity = cs.opacity;
      thumb.style.height = th + 'px';
      thumb.style.top = top + 'px';
      thumb.style.left = left + 'px';
      return true;
    }

    function layoutAll() {
      for (var i = tracked.length - 1; i >= 0; i--) {
        if (!layout(tracked[i])) {
          tracked[i].dispose();
          tracked.splice(i, 1);
        }
      }
    }

    function scan() {
      var els = document.querySelectorAll('*');
      for (var i = 0; i < els.length; i++) {
        var el = els[i];
        if (el.__whCScroll) {
          continue;
        }
        if (isScrollable(el)) {
          track(el);
        }
      }
      layoutAll();
    }

    var layoutPending = false;
    function scheduleLayout() {
      if (layoutPending) {
        return;
      }
      layoutPending = true;
      requestAnimationFrame(function () {
        layoutPending = false;
        layoutAll();
      });
    }

    var scanPending = false;
    function scheduleScan() {
      if (scanPending) {
        return;
      }
      scanPending = true;
      requestAnimationFrame(function () {
        scanPending = false;
        scan();
      });
    }

    // Scroll does not bubble, but it is observable in the capture phase, so one
    // listener catches scrolling in any container.
    window.addEventListener('scroll', scheduleLayout, true);
    window.addEventListener('resize', scheduleLayout);

    // While a popup animates in or out (transform/opacity), its bounding box moves
    // every frame - which scroll/resize/mutation events do not cover - so the thumb
    // would lag. When an animation touches a tracked container (or an ancestor that
    // moves it, like a dropdown wrapper), relayout each frame until it settles. Gated
    // on the target so unrelated hover transitions elsewhere do not spin the loop.
    var animUntil = 0;
    var animLooping = false;
    function animLoop() {
      layoutAll();
      if (performance.now() < animUntil) {
        requestAnimationFrame(animLoop);
      } else {
        animLooping = false;
      }
    }
    function kickAnimLoop() {
      animUntil = performance.now() + 300;
      if (!animLooping) {
        animLooping = true;
        requestAnimationFrame(animLoop);
      }
    }
    function affectsTracked(target) {
      if (!target || !target.contains) {
        return false;
      }
      for (var i = 0; i < tracked.length; i++) {
        var el = tracked[i].el;
        if (target === el || target.contains(el) || el.contains(target)) {
          return true;
        }
      }
      return false;
    }
    function onAnim(e) {
      if (affectsTracked(e.target)) {
        kickAnimLoop();
      }
    }
    document.addEventListener('transitionrun', onAnim, true);
    document.addEventListener('animationstart', onAnim, true);
    // End events kick the loop too, so the thumb settles at the container's final
    // position and opacity once the animation finishes.
    document.addEventListener('transitionend', onAnim, true);
    document.addEventListener('animationend', onAnim, true);

    // Re-scan as the React app adds/removes content or toggles overflow.
    new MutationObserver(scheduleScan).observe(document.body, {
      childList: true,
      subtree: true,
      attributeFilter: ['class', 'style'],
    });

    scan();
  });
})();
