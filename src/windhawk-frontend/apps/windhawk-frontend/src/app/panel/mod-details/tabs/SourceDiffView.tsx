/**
 * A read-only view of what changes between two versions of a mod's source,
 * built on CodeMirror: unified into one document, or split into the two
 * versions side by side.
 *
 * As in the source view, only the lines near the screen are in the DOM, so the
 * diff of the largest mod opens, scrolls and unfolds as quickly as a short one,
 * and the view sets no height of its own: the page scrolls it. Selection and
 * search are CodeMirror's and cover the whole of it.
 */

import { Compartment, EditorState, type Extension } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import type { TFunction } from 'i18next';
import { useEffect, useLayoutEffect, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { markWhileStuck } from '@app/panel/shared/markWhileStuck';
import { useTheme } from '@app/theme';
import { sourceCodeAppearance } from './sourceCodeTheme';
import { diffSources } from './sourceDiff';
import { unfoldLabel } from './sourceDiffGaps';
import { splitDiffView } from './sourceDiffSplit';
import { sourceDiffTheme } from './sourceDiffTheme';
import { unifiedDiff, unifiedDiffView } from './sourceDiffUnified';
import {
  contentLabel,
  readOnlySource,
  sourcePhrases,
  useFindShortcut,
} from './sourceEditor';
import { topPanelsIn } from './sourcePanels';

const DiffHost = styled.div`
  // A split view's two sides are one frame, rounded at its outer corners only.
  .cm-mergeView {
    border-radius: var(--whui-radius);
  }

  .cm-mergeView .cm-editor {
    border-radius: 0;
  }

  .cm-mergeViewEditor + .cm-mergeViewEditor {
    border-left: 1px solid var(--whui-border);
  }

  // Over the view, holding its search, and in a split view each side's over
  // that side.
  .cm-diff-panels {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;

    > div {
      flex: 1 1 0;
      min-width: 0;
    }

    &:has(.cm-panels) {
      background-color: var(--source-surface, var(--whui-card-background-color));
    }

    &[data-stuck]:has(.cm-panels) {
      box-shadow: var(--whui-stuck-shadow);
    }
  }
`;

type Role = 'unified' | 'old' | 'new';

const LABELS = {
  unified: 'modDetails.changes.title',
  old: 'modDetails.changes.oldSource',
  new: 'modDetails.changes.newSource',
} as const;

// What follows the app's theme and language, reconfigured in place as either
// changes.
const appearanceCompartment = new Compartment();

function appearanceOf(dark: boolean, t: TFunction) {
  const label = (lines: number) => t('modDetails.changes.expandLines', { count: lines });
  return (role: Role): Extension => [
    sourceCodeAppearance(dark),
    sourcePhrases(t),
    contentLabel(t(LABELS[role])),
    unfoldLabel.of(label),
  ];
}

interface Props {
  oldSource: string;
  newSource: string;
  split: boolean;
}

function SourceDiffView({ oldSource, newSource, split }: Props) {
  const { t } = useTranslation();
  const { resolvedTheme } = useTheme();

  const parentRef = useRef<HTMLDivElement>(null);
  const viewsRef = useRef<{ view: EditorView; role: Role }[]>([]);

  const diff = useMemo(() => diffSources(oldSource, newSource), [oldSource, newSource]);

  const appearance = useMemo(
    () => appearanceOf(resolvedTheme === 'dark', t),
    [resolvedTheme, t]
  );
  const appearanceRef = useRef(appearance);

  // A new diff or layout is a new view rather than an edit to the old one,
  // which Chromium's accessibility tree takes as one subtree swapped for
  // another instead of an event per node after the change.
  useLayoutEffect(() => {
    const parent = parentRef.current;
    if (!parent) {
      return;
    }

    const editor = (role: Role): Extension => [
      readOnlySource,
      sourceDiffTheme,
      appearanceCompartment.of(appearanceRef.current(role)),
    ];

    if (split) {
      const sides = splitDiffView(parent, diff, { old: editor('old'), new: editor('new') });
      viewsRef.current = [
        { view: sides.old, role: 'old' },
        { view: sides.new, role: 'new' },
      ];
      return () => {
        viewsRef.current = [];
        sides.destroy();
      };
    }

    const panelBar = parent.appendChild(document.createElement('div'));
    panelBar.className = 'cm-diff-panels';
    const unified = unifiedDiff(diff);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: unified.doc,
        extensions: [editor('unified'), unifiedDiffView(unified, diff), topPanelsIn(panelBar)],
      }),
    });
    viewsRef.current = [{ view, role: 'unified' }];
    const stopMarking = markWhileStuck(panelBar);
    return () => {
      stopMarking();
      viewsRef.current = [];
      view.destroy();
      panelBar.remove();
    };
  }, [diff, split]);

  useEffect(() => {
    if (appearanceRef.current === appearance) {
      return;
    }
    appearanceRef.current = appearance;
    for (const { view, role } of viewsRef.current) {
      view.dispatch({ effects: appearanceCompartment.reconfigure(appearance(role)) });
    }
  }, [appearance]);

  // Split, the side being read, or else the new one.
  useFindShortcut(() => {
    const views = viewsRef.current;
    return (views.find(({ view }) => view.hasFocus) ?? views[views.length - 1])?.view;
  });

  return <DiffHost ref={parentRef} />;
}

export default SourceDiffView;
