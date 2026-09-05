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
// (position:fixed, so it reserves no space and never shifts the layout) and does not
// auto-hide. The thumb is themed from the --wh-cscroll-* variables the theme shim sets,
// and tracks the container's box and opacity so it fades/scales WITH popups
// instead of flashing over them. The page body uses `overflow:hidden`; the real scroll
// containers are inner React nodes, so we discover them by scanning and re-scan as the
// app mutates. Thumbs live as direct children of <body> (outside React's #root), so
// React never reconciles them away.
//
// Two rules keep that discovery from having to be timed:
//
// - A container is claimed for what it CAN do, not for what it is doing. Anything whose
//   style lets it scroll, on either axis, is taken over on sight, while it is still
//   empty and long before anything overflows it, so the native bar has no window in
//   which to appear. Whether a thumb is DRAWN is a separate, per-frame decision in
//   `layout`. Claiming therefore turns on computed style alone, which cannot change
//   without a class or style mutation - and those the mutation observer sees.
// - What the thumb follows is the CONTENT, not the container. A container's own box does
//   not change when the content inside it grows, so each claimed container's children
//   are watched instead: that is one mechanism for an image reaching its intrinsic size,
//   a web font swapping in, a height animating, or a subtree being revealed, none of
//   which need a listener of their own.
//
// Both axes get a bar, written once and read along whichever axis it belongs to. They
// have to: hiding the native scrollbar is not per-axis, so a container taken over for
// its vertical bar would otherwise lose the horizontal one a wide code block or table in
// a readme is dragged by. Where both are showing, each gives up the corner square they
// would cross in, the way a native pair does.
//
// In Windows high contrast mode the whole overlay steps aside and the native scrollbar
// is left in place (see the forced-colors gate below).
//
// Drag and track-paging use pointer events with pointer capture, so they work with
// mouse, touch, and pen and keep receiving moves once the pointer leaves the element. In
// an RTL container the vertical bar sits on the left edge, matching the native one, and
// the horizontal one counts its offset from the right.
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
      '.wh-cscroll-track{position:fixed;pointer-events:auto;touch-action:none;}' +
      // Flat (no radius), themed from the slider variables; per-thumb opacity is set
      // in layout to match the container. touch-action none so a touch/pen press drags
      // the thumb instead of panning the page.
      '.wh-cscroll-thumb{position:fixed;pointer-events:auto;touch-action:none;' +
      'cursor:default;background-color:var(--wh-cscroll-thumb,rgba(121,121,121,.4));}' +
      // Only the thickness, which is the axis a bar does not scroll along; the length
      // along the one it does is laid out per frame.
      '.wh-cscroll-v{width:' +
      WIDTH +
      'px;}' +
      '.wh-cscroll-h{height:' +
      WIDTH +
      'px;}' +
      '.wh-cscroll-thumb:hover{background-color:var(--wh-cscroll-thumb-hover,rgba(100,100,100,.7));}' +
      '.wh-cscroll-thumb.wh-active{background-color:var(--wh-cscroll-thumb-active,rgba(191,191,191,.4));}';
    document.head.appendChild(style);

    var tracked = []; // array of records, one per taken-over scroll container
    var resizeObs = window.ResizeObserver ? new ResizeObserver(scheduleLayout) : null;

    // A container's scroll height is the extent of its children, and growing content
    // resizes those children while leaving the container's own box alone - which is why
    // the observer above never hears about it. Watching the children instead covers
    // every cause at once: an image reaching its intrinsic size (which happens as soon
    // as its header arrives, with no DOM mutation and long before the load event), a
    // font swapping in, a height transition, a subtree unhiding. A grandchild growing
    // either grows its parent, which is watched, or does not move the scroll height.
    var contentObs = window.ResizeObserver ? new ResizeObserver(scheduleLayout) : null;

    // Take up the children a claimed container has now, and let go of the ones that
    // left it: an observer keeps its targets alive, and each mod's screen brings its own.
    function syncContent(rec) {
      if (!contentObs) {
        return;
      }
      var kids = rec.el.children;
      for (var i = 0; i < kids.length; i++) {
        if (!kids[i].__whCScrollContent) {
          kids[i].__whCScrollContent = true;
          rec.content.push(kids[i]);
          contentObs.observe(kids[i]);
        }
      }
      for (var j = rec.content.length - 1; j >= 0; j--) {
        if (rec.content[j].parentElement !== rec.el) {
          dropContent(rec.content[j]);
          rec.content.splice(j, 1);
        }
      }
    }

    function dropContent(el) {
      contentObs.unobserve(el);
      el.__whCScrollContent = false;
    }

    // Windows high contrast mode hands the UI over to the user's system palette. The
    // native scrollbar honors it; our overlay thumb, painted from the theme tokens,
    // would not - so leave the native bar in place there. The query is live, so toggling
    // high contrast mid-session takes effect without a restart.
    var forcedColors = window.matchMedia && window.matchMedia('(forced-colors: active)');

    function highContrast() {
      return !!forcedColors && forcedColors.matches;
    }

    // Monaco draws its own scrollbars and rewrites its view on every keystroke, so
    // nothing inside an editor is ours to claim and none of its churn - the view
    // mutations, or the cursor's blink animation - is worth answering. Skipping the
    // subtree keeps typing off this script's critical path.
    var MONACO_SELECTOR = '.monaco-editor';

    function inMonaco(el) {
      return !!(el && el.closest && el.closest(MONACO_SELECTOR));
    }

    // Whether the element is one to claim: what its style allows, not what it is
    // currently doing. Overflowing is deliberately not part of it - an element claimed
    // before it overflows simply draws no thumb, whereas one claimed after has already
    // shown the native bar for however long the content took to arrive.
    function canScroll(el) {
      if (el === document.body || el === document.documentElement) {
        return false;
      }
      // Cheap geometry check first: scan walks every element, so this rejects the vast
      // majority (inline boxes and anything short) before the costly getComputedStyle.
      // A container this short is left to the native bar on either axis, having no room
      // to draw a thumb that could be grabbed.
      if (el.clientHeight <= 40) {
        return false;
      }
      var s = getComputedStyle(el);
      // Either axis is reason enough: hiding the native bar is not per-axis, so a
      // container claimed for one of them has to answer for both.
      if (!scrolls(s.overflowY) && !scrolls(s.overflowX)) {
        return false;
      }
      // Respect elements the app deliberately renders without a scrollbar.
      return s.getPropertyValue('scrollbar-width') !== 'none';
    }

    function scrolls(overflow) {
      return overflow === 'auto' || overflow === 'scroll';
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

    // One bar - a track and the thumb riding on it - for one axis of one container. `h`
    // picks the horizontal axis, and the accessors just below are the only place the two
    // differ: everything after them is written along the axis the bar scrolls and across
    // to the edge it hugs, so both axes share one set of geometry, drag and paging.
    function makeBar(el, h, z) {
      var axis = h ? ' wh-cscroll-h' : ' wh-cscroll-v';
      // The track captures clicks in the empty space for page up/down; the thumb is
      // appended after it so it stays on top and handles its own drag.
      var bar = document.createElement('div');
      bar.className = 'wh-cscroll-track' + axis;
      bar.style.zIndex = z;
      bar.style.display = 'none';
      bar.__whCScrollOwn = true;
      document.body.appendChild(bar);
      var thumb = document.createElement('div');
      thumb.className = 'wh-cscroll-thumb' + axis;
      thumb.style.zIndex = z;
      thumb.style.display = 'none';
      thumb.__whCScrollOwn = true;
      document.body.appendChild(thumb);

      // Where a length and a position along the axis are written, and where the one
      // across it is.
      var alongPos = h ? 'left' : 'top';
      var alongSize = h ? 'width' : 'height';
      var crossPos = h ? 'top' : 'left';
      var rtl = false; // the container's direction, refreshed by layout

      function client() {
        return h ? el.clientWidth : el.clientHeight;
      }
      function extent() {
        return h ? el.scrollWidth : el.scrollHeight;
      }
      function at(e) {
        return h ? e.clientX : e.clientY;
      }
      // The scroll offset, normalized to run from 0 at the start edge up to
      // `extent - client` at the end. That is what scrollTop and an LTR scrollLeft
      // already do; an RTL container scrolls from its right edge, so its scrollLeft runs
      // from -(extent - client) up to 0.
      function offset() {
        return h ? el.scrollLeft + (rtl ? extent() - client() : 0) : el.scrollTop;
      }
      function scrollTo(v) {
        if (h) {
          el.scrollLeft = v;
        } else {
          el.scrollTop = v;
        }
      }

      // Whether this axis overflows, and so has a bar to show at all.
      function needed() {
        return extent() - client() > 1;
      }

      var dragging = false;
      var startAt = 0;
      var startScroll = 0;
      var room = 0; // how far the thumb can travel, as the last layout drew it

      function onDown(e) {
        dragging = true;
        startAt = at(e);
        startScroll = h ? el.scrollLeft : el.scrollTop;
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
        if (!dragging || room <= 0) {
          return;
        }
        // Against the room the thumb was drawn with, so a track shortened by the corner
        // (or by a transform) maps the pointer where the thumb actually is. Relative to
        // where the drag started, so it needs no view of which end the container counts
        // its scroll offset from.
        scrollTo(startScroll + ((at(e) - startAt) / room) * (extent() - client()));
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
      var pointerAt = 0;
      var pageDelay = null;
      var pageTimer = null;

      function pageReached() {
        var tr = thumb.getBoundingClientRect();
        var lead = h ? tr.left : tr.top;
        var trail = h ? tr.right : tr.bottom;
        // The cursor is over the thumb, or the thumb's leading edge passed it (the
        // latter guards against a step overshooting, which would otherwise loop away).
        if (pointerAt >= lead && pointerAt <= trail) {
          return true;
        }
        return pageDir > 0 ? lead >= pointerAt : trail <= pointerAt;
      }

      function pageAtEnd() {
        return pageDir > 0 ? offset() + client() >= extent() - 1 : offset() <= 0;
      }

      function pageStep() {
        if (pageReached() || pageAtEnd()) {
          stopPaging();
          return;
        }
        var by = { behavior: 'smooth' };
        // A positive step is toward the trailing edge on both axes, RTL included: it is
        // the offset that runs backwards there, not the direction of travel.
        by[alongPos] = pageDir * client();
        el.scrollBy(by);
      }

      function onPageMove(e) {
        pointerAt = at(e);
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
        var here = at(e);
        var lead = h ? tr.left : tr.top;
        var trail = h ? tr.right : tr.bottom;
        pageDir = here < lead ? -1 : here > trail ? 1 : 0;
        e.preventDefault();
        e.stopPropagation();
        if (!pageDir) {
          return;
        }
        bar.setPointerCapture(e.pointerId);
        pointerAt = here;
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

      function hideBar() {
        bar.style.display = 'none';
        thumb.style.display = 'none';
      }

      // Clip one of the two nodes to the box the container is visible within, given
      // where the node sits along the axis and how long it is. clip-path takes hit
      // testing with it, so a bar clipped away cannot swallow a click either.
      function clipTo(node, box, along, length, cross) {
        if (!box) {
          node.style.clipPath = 'none';
          return;
        }
        var top = h ? cross : along;
        var left = h ? along : cross;
        var over = Math.max(0, box.top - top);
        var under = Math.max(0, top + (h ? WIDTH : length) - box.bottom);
        var before = Math.max(0, box.left - left);
        var after = Math.max(0, left + (h ? length : WIDTH) - box.right);
        node.style.clipPath =
          over || under || before || after
            ? 'inset(' + over + 'px ' + after + 'px ' + under + 'px ' + before + 'px)'
            : 'none';
      }

      // `r` is the container's rendered box and `cs` its opacity and direction, both
      // read once for the pair. `corner` is whether the other axis is showing its bar
      // too, and so wants the square where the two would otherwise cross.
      function layoutBar(r, cs, corner) {
        var c = client();
        var s = extent();
        if (s - c <= 1) {
          hideBar();
          return;
        }
        rtl = cs.rtl;
        // Along the axis, from the RENDERED box (getBoundingClientRect reflects the live
        // transform), so the bar tracks a popup that scales or slides while animating;
        // the visible fraction still comes from the unscaled layout metrics. The corner
        // comes off the end the other bar sits at, which is the bottom, or - for the
        // horizontal bar in an RTL container - the left.
        var span = (h ? r.width : r.height) - (corner ? WIDTH : 0);
        var start = (h ? r.left : r.top) + (corner && h && cs.rtl ? WIDTH : 0);
        var len = Math.max(span * (c / s), MIN_THUMB);
        room = span - len;
        // Clamped, so a scrollLeft that does not run the way `offset` reads it could
        // only pin the thumb to one end rather than place it outside its track.
        var frac = Math.min(1, Math.max(0, offset() / (s - c)));
        // In an RTL container the native vertical scrollbar sits on the left, so ours
        // goes there too; the horizontal bar hugs the bottom either way.
        var cross = h ? r.bottom - WIDTH : cs.rtl ? r.left : r.right - WIDTH;
        // The track spans the whole visible length (for page up/down); the thumb rides
        // on top of it.
        var thumbAt = start + room * frac;
        bar.style.display = 'block';
        bar.style[alongPos] = start + 'px';
        bar.style[alongSize] = span + 'px';
        bar.style[crossPos] = cross + 'px';
        clipTo(bar, cs.clip, start, span, cross);
        thumb.style.display = 'block';
        thumb.style.opacity = cs.opacity;
        thumb.style[alongPos] = thumbAt + 'px';
        thumb.style[alongSize] = len + 'px';
        thumb.style[crossPos] = cross + 'px';
        clipTo(thumb, cs.clip, thumbAt, len, cross);
      }

      return {
        needed: needed,
        layout: layoutBar,
        hide: hideBar,
        dispose: function () {
          stopPaging();
          thumb.remove();
          bar.remove();
        },
      };
    }

    function track(el) {
      el.classList.add('wh-cscroll-host');
      var z = stackZ(el);
      var vert = makeBar(el, false, z);
      var horiz = makeBar(el, true, z);
      if (resizeObs) {
        resizeObs.observe(el);
      }

      var rec = {
        el: el,
        vert: vert,
        horiz: horiz,
        content: [], // the children contentObs holds, kept by syncContent
        // Skip the very first layout: rc-motion applies a popup's animation start state
        // (opacity 0) a frame after mount, so reading opacity now could catch the
        // default 1 and flash. By the next frame the start state is in effect.
        firstFrame: true,
        dispose: function () {
          vert.dispose();
          horiz.dispose();
          if (resizeObs) {
            resizeObs.unobserve(el);
          }
          for (var i = 0; i < rec.content.length; i++) {
            dropContent(rec.content[i]);
          }
          rec.content.length = 0;
          delete el.__whCScroll;
          el.classList.remove('wh-cscroll-host');
        },
      };
      el.__whCScroll = rec;
      tracked.push(rec);
      // A container claimed while its popup is animating in would otherwise sit still
      // until the motion ended: the events that kick the loop are gated on the container
      // being tracked, and this one was not when the motion began. Relayout per frame for
      // a moment, so the bars fade and scale in with it (see the opacity match in layout).
      kickAnimLoop();
    }

    // The container's cumulative opacity (multiplied up the tree), whether it is RTL,
    // and the box it is actually visible within, all gathered in one ancestor walk.
    //
    // A bar lives in <body>, so an ancestor affects it through none of the three the way
    // it affects the container. Matching the opacity makes the thumb fade in/out WITH
    // the popup (whose appear/leave motion animates opacity on an ancestor) instead of
    // flashing over a not-yet-visible dropdown. Carrying the clip keeps a bar for a
    // container nested in a scrolling one - a code block inside the readme - from being
    // drawn over whatever sits beyond the edge it has been scrolled past. RTL is read
    // off the container's own style, so it costs no extra getComputedStyle.
    function containerStyle(el) {
      var opacity = 1;
      var rtl = false;
      var clip = null;
      for (var p = el; p && p !== document.body; p = p.parentElement) {
        var s = getComputedStyle(p);
        if (p === el) {
          rtl = s.direction === 'rtl';
        } else if (scrolls(s.overflowX) || scrolls(s.overflowY)) {
          clip = intersect(clip, p.getBoundingClientRect());
        }
        var po = parseFloat(s.opacity);
        if (!isNaN(po)) {
          opacity *= po;
        }
      }
      return { opacity: opacity, rtl: rtl, clip: clip };
    }

    function intersect(a, b) {
      if (!a) {
        return b;
      }
      return {
        top: Math.max(a.top, b.top),
        left: Math.max(a.left, b.left),
        bottom: Math.min(a.bottom, b.bottom),
        right: Math.min(a.right, b.right),
      };
    }

    function hide(rec) {
      rec.vert.hide();
      rec.horiz.hide();
    }

    function layout(rec) {
      var el = rec.el;
      if (!el.isConnected) {
        return false;
      }
      // React may rewrite className on re-render and drop our host class; reassert it.
      // Only when it is actually missing: writing the attribute back unchanged is still
      // a mutation, which the observer would answer with a scan, every frame.
      if (!el.classList.contains('wh-cscroll-host')) {
        el.classList.add('wh-cscroll-host');
      }
      if (rec.firstFrame) {
        rec.firstFrame = false;
        hide(rec);
        return true;
      }
      // Which bars the container wants, decided before either is laid out: each needs to
      // know whether the other is there to leave it the corner.
      var wantsVert = rec.vert.needed();
      var wantsHoriz = rec.horiz.needed();
      if (!wantsVert && !wantsHoriz) {
        hide(rec);
        return true;
      }
      // Match the container's opacity so the thumb fades in/out with a popup instead of
      // flashing over it (RTL comes from the same walk). While effectively invisible,
      // take the thumbs and tracks out entirely so they cannot capture clicks over the
      // not-yet-shown popup.
      var cs = containerStyle(el);
      if (cs.opacity < 0.01) {
        hide(rec);
        return true;
      }
      var r = el.getBoundingClientRect();
      rec.vert.layout(r, cs, wantsHoriz);
      rec.horiz.layout(r, cs, wantsVert);
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

    // Hand every container back to its native scrollbars: dispose drops both bars and
    // the host class that hid them.
    function untrackAll() {
      for (var i = tracked.length - 1; i >= 0; i--) {
        tracked[i].dispose();
      }
      tracked.length = 0;
    }

    function scan() {
      if (highContrast()) {
        untrackAll();
        return;
      }
      // A tree walker rather than a flat query: rejecting an editor skips its entire
      // subtree, which a per-element test would still have to walk.
      var walker = document.createTreeWalker(
        document.body,
        NodeFilter.SHOW_ELEMENT,
        function (node) {
          return node.matches(MONACO_SELECTOR)
            ? NodeFilter.FILTER_REJECT
            : NodeFilter.FILTER_ACCEPT;
        }
      );
      for (var el = walker.nextNode(); el; el = walker.nextNode()) {
        if (el.__whCScroll) {
          continue;
        }
        if (canScroll(el)) {
          track(el);
        }
      }
      // Here rather than in track: a child list changes only through a mutation, and a
      // mutation is one of the things that runs a scan, so this is where both a new
      // container and a new child are picked up.
      for (var j = 0; j < tracked.length; j++) {
        syncContent(tracked[j]);
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

    // Entering high contrast drops every thumb on the next scan; leaving it takes the
    // containers back over.
    if (forcedColors) {
      forcedColors.addEventListener('change', scheduleScan);
    }

    // Scroll does not bubble, but it is observable in the capture phase, so one
    // listener catches scrolling in any container.
    window.addEventListener('scroll', scheduleLayout, true);
    // A resize scans rather than just relaying out: it is the one bulk reflow that is
    // not a mutation, and it can hand an element the height (or, through a media query,
    // the overflow) that makes it claimable. A scan lays out at the end anyway.
    window.addEventListener('resize', scheduleScan);

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
      if (!inMonaco(e.target) && affectsTracked(e.target)) {
        kickAnimLoop();
      }
    }
    document.addEventListener('transitionrun', onAnim, true);
    document.addEventListener('animationstart', onAnim, true);
    // End events kick the loop too, so the thumb settles at the container's final
    // position and opacity once the animation finishes.
    document.addEventListener('transitionend', onAnim, true);
    document.addEventListener('animationend', onAnim, true);

    // Re-scan as the React app adds/removes content or toggles overflow. The bars live
    // in <body> as well, and laying one out writes its style, so records for our own
    // nodes are dropped: answering them would put a full scan in every frame of a
    // scroll, which is where a scan is least affordable.
    new MutationObserver(function (records) {
      for (var i = 0; i < records.length; i++) {
        var target = records[i].target;
        if (!target.__whCScrollOwn && !inMonaco(target)) {
          scheduleScan();
          return;
        }
      }
    }).observe(document.body, {
      childList: true,
      subtree: true,
      attributeFilter: ['class', 'style'],
    });

    scan();
  });
})();
