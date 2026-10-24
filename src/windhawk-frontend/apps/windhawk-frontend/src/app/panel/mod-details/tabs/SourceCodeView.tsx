/**
 * A read-only, highlighted view of a C++ source, built on CodeMirror.
 *
 * Only the lines in and near the viewport are in the DOM, which is what keeps a
 * mod of tens of thousands of lines as quick to open, scroll and leave as a
 * short one. The view sets no height of its own, so it is as tall as its text
 * and the page scrolls it, header and all; CodeMirror follows the scrolling of
 * whichever ancestor does it to know which lines to draw.
 *
 * Selection, copying and search are CodeMirror's and cover the whole text, not
 * just the part that is drawn - which is also why the browser's own find is
 * taken over while the view is up: it would only see the lines in the DOM.
 */

import { cpp } from '@codemirror/lang-cpp';
import { Compartment, EditorState, type Extension } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import type { TFunction } from 'i18next';
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
} from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { markWhileStuck } from '@app/panel/shared/markWhileStuck';
import { useTheme } from '@app/theme';
import { sourceCodeAppearance } from './sourceCodeTheme';
import {
  contentLabel,
  readOnlySource,
  sourcePhrases,
  useFindShortcut,
} from './sourceEditor';
import { topPanelsIn } from './sourcePanels';

// The bar the view's search is drawn in: over the code, and under whatever the
// host hangs over its corner.
const SourceHost = styled.div`
  .cm-source-panels {
    position: sticky;
    top: 0;
    z-index: 1;
  }

  .cm-source-panels[data-stuck] .cm-panels {
    box-shadow: var(--whui-stuck-shadow);
  }
`;

// What follows the app's theme and language, reconfigured in place as either
// changes.
const appearanceCompartment = new Compartment();

const staticExtensions: Extension = [readOnlySource, cpp()];

function localized(t: TFunction): Extension {
  return [sourcePhrases(t), contentLabel(t('modDetails.code.title'))];
}

export interface SourceCodeViewHandle {
  // The selected text, with several ranges joined a line apart; empty when
  // nothing is selected.
  selectedText(): string;
  focus(): void;
}

interface Props {
  source: string;
}

const SourceCodeView = forwardRef<SourceCodeViewHandle, Props>(
  function SourceCodeView({ source }, ref) {
    const { t } = useTranslation();
    const { resolvedTheme } = useTheme();

    const parentRef = useRef<HTMLDivElement>(null);
    const viewRef = useRef<EditorView | null>(null);

    const appearance = useMemo(
      () => [sourceCodeAppearance(resolvedTheme === 'dark'), localized(t)],
      [resolvedTheme, t]
    );
    const appearanceRef = useRef(appearance);

    // A new source is a new view rather than an edit to the old one. Swapping
    // a subtree whole is the one DOM change Chromium's accessibility tree
    // takes cheaply, where rewriting the lines in place costs it an event per
    // node that follows the change.
    useLayoutEffect(() => {
      const parent = parentRef.current;
      if (!parent) {
        return;
      }

      const panelBar = parent.appendChild(document.createElement('div'));
      panelBar.className = 'cm-source-panels';
      const view = new EditorView({
        parent,
        state: EditorState.create({
          doc: source,
          extensions: [
            staticExtensions,
            topPanelsIn(panelBar),
            appearanceCompartment.of(appearanceRef.current),
          ],
        }),
      });
      viewRef.current = view;
      const stopMarking = markWhileStuck(panelBar);

      return () => {
        stopMarking();
        viewRef.current = null;
        view.destroy();
        panelBar.remove();
      };
    }, [source]);

    useEffect(() => {
      if (appearanceRef.current === appearance) {
        return;
      }
      appearanceRef.current = appearance;
      viewRef.current?.dispatch({
        effects: appearanceCompartment.reconfigure(appearance),
      });
    }, [appearance]);

    useFindShortcut(() => viewRef.current);

    useImperativeHandle(
      ref,
      () => ({
        selectedText() {
          const state = viewRef.current?.state;
          if (!state) {
            return '';
          }
          return state.selection.ranges
            .filter((range) => !range.empty)
            .map((range) => state.sliceDoc(range.from, range.to))
            .join(state.lineBreak);
        },
        focus() {
          viewRef.current?.focus();
        },
      }),
      []
    );

    return <SourceHost ref={parentRef} />;
  }
);

export default SourceCodeView;
