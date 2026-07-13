// The Windhawk debug-log pane: a resizable bottom split of the main window that
// tails the live [WH] output captured by the native shell and shows compiler
// diagnostics for a failed local compile. Tauri-only - in the VSCode extension the
// host uses a native output channel, and the website has no backend.
//
// It is a read-only Monaco editor, so virtualized rendering (any log volume),
// word wrap, selection/copy, and the find widget (Ctrl+F, with match-case /
// whole-word / regex) all come from Monaco rather than being hand-rolled. The
// editor holds the full log - no line cap - because Monaco only renders the visible
// slice.
//
// Visibility and the shell's reveal signal are owned by LogPaneMount, which
// lazy-loads this module (and Monaco) on the first reveal. This component owns the
// rest of the native contract: it seeds from `wh_log_backlog` once on mount, then
// appends the live `wh-log` batches; Close calls `wh_log_stop_capture` so capture is
// scoped to while the pane is open (R7, single-owner DBWIN buffer).

import {
  faAnglesDown,
  faTextWidth,
  faTrashCan,
  faXmark,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import Editor, { loader, type OnMount } from '@monaco-editor/react';
import * as monaco from 'monaco-editor/esm/vs/editor/editor.api.js';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import {
  fetchLogBacklog,
  listenLogLines,
  stopLogCapture,
  type UnlistenFn,
} from '../../tauriApi';

// Point @monaco-editor/react at the bundled monaco-editor package rather than the
// CDN, matching MonacoYamlEditor (module-scope side effect on import).
loader.config({ monaco });

// The pane opens at this fraction of the window height and never shrinks itself
// below MIN_PANE nor the app above it below MIN_APP.
const DEFAULT_FRACTION = 0.4;
const MIN_PANE = 120;
const MIN_APP = 100;
// Treat the view as "following the tail" when the scroll bottom is within this many
// pixels of the content bottom, so auto-scroll survives sub-pixel rounding.
const AT_BOTTOM_EPSILON = 8;

const Root = styled.div<{ $visible: boolean; $height: number }>`
  display: ${({ $visible }) => ($visible ? 'flex' : 'none')};
  flex: 0 0 auto;
  flex-direction: column;
  height: ${({ $height }) => $height}px;
  min-height: 0;
  direction: ltr;
  border-top: 1px solid #454545;
  background: var(--app-background-color, #1e1e1e);
  /* The main content's scroll region is position:relative, so it paints in the
     positioned phase and would otherwise draw its overlay scrollbar over this
     (static) pane. Give the pane its own stacking context above it. */
  position: relative;
  z-index: 1;
`;

const Splitter = styled.div`
  flex: 0 0 6px;
  cursor: ns-resize;
  background: transparent;
  touch-action: none;
  &:hover,
  &.wh-drag {
    background: #0078d4;
  }
`;

const Body = styled.div`
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
`;

const Header = styled.div`
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 1px 4px 1px 10px;
`;

const Title = styled.span`
  flex: 1 1 auto;
  opacity: 0.7;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  font-size: 11px;
  color: #d4d4d4;
`;

const Actions = styled.div`
  display: flex;
  gap: 2px;
`;

const IconButton = styled.button<{ $active?: boolean }>`
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  color: #d4d4d4;
  background: transparent;
  border: 0;
  border-radius: 4px;
  cursor: pointer;
  padding: 0;
  font-size: 14px;
  opacity: ${({ $active }) => ($active === false ? 0.45 : 1)};
  &:hover {
    background: rgba(127, 127, 127, 0.22);
  }
`;

const EditorWrap = styled.div`
  flex: 1 1 auto;
  min-height: 0;
`;

function clampHeight(height: number): number {
  const max = Math.max(window.innerHeight - MIN_APP, MIN_PANE);
  return Math.min(Math.max(height, MIN_PANE), max);
}

function isAtBottom(editor: monaco.editor.IStandaloneCodeEditor): boolean {
  const maxTop = editor.getScrollHeight() - editor.getLayoutInfo().height;
  return editor.getScrollTop() >= maxTop - AT_BOTTOM_EPSILON;
}

function scrollToBottom(editor: monaco.editor.IStandaloneCodeEditor): void {
  // Reveal the last line by number (Monaco computes the offset arithmetically from the
  // line count) rather than setScrollTop(getScrollHeight()), whose height can be stale
  // in the same tick as the edit that just grew the model - which would leave the tail
  // one batch short.
  const lineCount = editor.getModel()?.getLineCount() ?? 1;
  editor.revealLine(lineCount, monaco.editor.ScrollType.Immediate);
}

// Park the caret on the trailing (empty) line so the blinking caret sits at the bottom
// with the tail, rather than off-screen at the top of the document.
function caretToEnd(editor: monaco.editor.IStandaloneCodeEditor): void {
  const model = editor.getModel();
  if (model) {
    const lastLine = model.getLineCount();
    editor.setPosition({
      lineNumber: lastLine,
      column: model.getLineMaxColumn(lastLine),
    });
  }
}

export interface LogPaneProps {
  // Owned by LogPaneMount: the pane is kept mounted (so the model survives) and
  // shown/hidden via this flag as the shell reveals it or the user closes it.
  visible: boolean;
  onClose: () => void;
}

function LogPane({ visible, onClose }: LogPaneProps) {
  const { t } = useTranslation();

  // The editor mounts on the first reveal and then stays mounted, so the full model
  // survives closing and reopening the pane.
  const [height, setHeight] = useState(() =>
    clampHeight(Math.round(window.innerHeight * DEFAULT_FRACTION)),
  );
  const [wordWrap, setWordWrap] = useState(true);
  const [following, setFollowing] = useState(true);

  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const followingRef = useRef(true);
  const linesUnlistenRef = useRef<UnlistenFn | null>(null);
  // Distinguishes a reveal (hidden -> shown) from a resize while already shown, so the
  // reveal effect only jumps to the tail on an actual open.
  const wasVisibleRef = useRef(false);

  useEffect(() => {
    followingRef.current = following;
  }, [following]);

  // Append a batch to the end of the model in one edit, keeping a trailing newline so
  // the newest line is not flush against the bottom edge. applyEdits does not touch the
  // undo stack, so an unbounded stream does not grow one. Keep the tail (and the caret)
  // in view while following.
  const appendLines = useCallback((lines: string[]) => {
    const editor = editorRef.current;
    const model = editor?.getModel();
    if (!editor || !model || lines.length === 0) {
      return;
    }
    // Capture the follow state BEFORE the edit: applyEdits can synchronously fire a
    // scroll-change event, so reading followingRef afterwards risks a transient flip
    // that would skip this batch's scroll.
    const stick = followingRef.current;
    const lastLine = model.getLineCount();
    const lastColumn = model.getLineMaxColumn(lastLine);
    model.applyEdits([
      {
        range: new monaco.Range(lastLine, lastColumn, lastLine, lastColumn),
        text: lines.join('\n') + '\n',
      },
    ]);
    if (stick) {
      caretToEnd(editor);
      scrollToBottom(editor);
    }
  }, []);

  const handleEditorMount = useCallback<OnMount>(
    (editor) => {
      editorRef.current = editor;
      wasVisibleRef.current = true;

      // Manual scrolling drives following: scrolling away from the bottom stops the
      // tail, scrolling back to it resumes. Only a pure scrollTop move (the user's
      // wheel/drag) counts - a content-growth event (scrollHeightChanged, from an
      // appended line) must be ignored, or appending while at the bottom would read as
      // "scrolled up" and stop auto-scroll after the first line.
      editor.onDidScrollChange((event) => {
        if (!event.scrollTopChanged || event.scrollHeightChanged) {
          return;
        }
        const atBottom = isAtBottom(editor);
        if (atBottom !== followingRef.current) {
          followingRef.current = atBottom;
          setFollowing(atBottom);
        }
      });

      // Fetch the backlog here, now that the editor exists, then seed it and subscribe
      // to the live stream back to back. Fetching before the editor mounted would widen
      // the window between the backlog snapshot and the subscription to however long
      // Monaco takes to instantiate, dropping any live batch that lands in between;
      // doing both here keeps that window to a single microtask. Subscribing only after
      // the backlog is applied also means a line is never rendered twice.
      void fetchLogBacklog().then((backlog) => {
        const model = editor.getModel();
        if (!model) {
          return;
        }
        model.setValue(backlog.length ? backlog.join('\n') + '\n' : '');
        // Measure the (now-visible) container before scrolling; a fresh editor can report
        // a zero viewport until its first layout, which would strand the tail at the top.
        editor.layout();
        caretToEnd(editor);
        scrollToBottom(editor);
        // Re-assert on the next frame, after the automatic layout has settled, so the
        // first open reliably lands at the bottom.
        requestAnimationFrame(() => scrollToBottom(editor));
        // Focus so the caret shows and Ctrl+F / keyboard navigation work immediately,
        // without a click into the editor.
        editor.focus();

        listenLogLines(appendLines)?.then((unlisten) => {
          linesUnlistenRef.current = unlisten;
        });
      });
    },
    [appendLines],
  );

  // The editor renders on the first reveal, and its onMount seeds the backlog and
  // subscribes to the live stream. On unmount, release the live-line listener.
  useEffect(() => {
    return () => {
      linesUnlistenRef.current?.();
    };
  }, []);

  // Monaco lays out to a zero size while the pane is display:none, so re-measure on
  // every show/resize. On an actual reveal (hidden -> shown, editor already mounted from
  // a previous open) jump to the tail, resume following, and focus - so opening the pane
  // always lands on the newest output ready to search or scroll. A resize while already
  // open only re-pins the tail if we are following. The first reveal is handled in
  // handleEditorMount.
  useEffect(() => {
    if (!visible) {
      wasVisibleRef.current = false;
      return;
    }
    const editor = editorRef.current;
    if (!editor) {
      return;
    }
    editor.layout();
    const revealed = !wasVisibleRef.current;
    wasVisibleRef.current = true;
    if (revealed) {
      followingRef.current = true;
      setFollowing(true);
      caretToEnd(editor);
      scrollToBottom(editor);
      editor.focus();
    } else if (followingRef.current) {
      scrollToBottom(editor);
    }
  }, [visible, height]);

  const beginResize = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      event.preventDefault();
      const startY = event.clientY;
      const startHeight = height;
      const splitter = event.currentTarget;
      splitter.setPointerCapture(event.pointerId);
      splitter.classList.add('wh-drag');
      // Dragging up (smaller clientY) grows the bottom pane.
      const move = (moveEvent: PointerEvent) => {
        setHeight(clampHeight(startHeight + (startY - moveEvent.clientY)));
      };
      const end = () => {
        splitter.releasePointerCapture(event.pointerId);
        splitter.classList.remove('wh-drag');
        splitter.removeEventListener('pointermove', move);
        splitter.removeEventListener('pointerup', end);
      };
      splitter.addEventListener('pointermove', move);
      splitter.addEventListener('pointerup', end);
    },
    [height],
  );

  const toggleFollowing = useCallback(() => {
    setFollowing((on) => {
      const next = !on;
      followingRef.current = next;
      if (next && editorRef.current) {
        caretToEnd(editorRef.current);
        scrollToBottom(editorRef.current);
      }
      return next;
    });
  }, []);

  const clear = useCallback(() => {
    editorRef.current?.getModel()?.setValue('');
  }, []);

  const close = useCallback(() => {
    onClose();
    stopLogCapture();
  }, [onClose]);

  return (
    <Root $visible={visible} $height={height}>
      <Splitter onPointerDown={beginResize} />
      <Body>
        <Header>
          <Title>{t('logPane.title')}</Title>
          <Actions>
            <IconButton
              type="button"
              title={t('logPane.clear')}
              aria-label={t('logPane.clear')}
              onClick={clear}
            >
              <FontAwesomeIcon icon={faTrashCan} />
            </IconButton>
            <IconButton
              type="button"
              $active={wordWrap}
              title={wordWrap ? t('logPane.wrapOff') : t('logPane.wrapOn')}
              aria-pressed={wordWrap}
              onClick={() => setWordWrap((on) => !on)}
            >
              <FontAwesomeIcon icon={faTextWidth} />
            </IconButton>
            <IconButton
              type="button"
              $active={following}
              title={
                following
                  ? t('logPane.autoScrollOff')
                  : t('logPane.autoScrollOn')
              }
              aria-pressed={following}
              onClick={toggleFollowing}
            >
              <FontAwesomeIcon icon={faAnglesDown} />
            </IconButton>
            <IconButton
              type="button"
              title={t('logPane.close')}
              aria-label={t('logPane.close')}
              onClick={close}
            >
              <FontAwesomeIcon icon={faXmark} />
            </IconButton>
          </Actions>
        </Header>
        <EditorWrap>
          <Editor
            height="100%"
            theme="vs-dark"
            defaultLanguage="plaintext"
            defaultValue=""
            onMount={handleEditorMount}
            options={{
              readOnly: true,
              domReadOnly: true,
              wordWrap: wordWrap ? 'on' : 'off',
              minimap: { enabled: false },
              lineNumbers: 'off',
              folding: false,
              glyphMargin: false,
              scrollBeyondLastLine: false,
              renderLineHighlight: 'none',
              occurrencesHighlight: 'off',
              selectionHighlight: false,
              matchBrackets: 'never',
              automaticLayout: true,
              wordBasedSuggestions: 'off',
              fontSize: 12,
              scrollbar: { alwaysConsumeMouseWheel: false },
            }}
          />
        </EditorWrap>
      </Body>
    </Root>
  );
}

export default LogPane;
