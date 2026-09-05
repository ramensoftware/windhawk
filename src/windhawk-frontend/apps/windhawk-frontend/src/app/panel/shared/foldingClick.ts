/**
 * Whether a press on text that stands in for what it folds is the press that
 * folds it. A press ending a selection is the selection's: reading a name by
 * dragging across it is not asking for what it names to close over it.
 */
export function isFoldingClick(): boolean {
  return document.getSelection()?.isCollapsed !== false;
}

/** An onClick that folds, unless the press it lands on ended a text selection. */
export function foldingClickHandler(onToggle: () => void) {
  return () => {
    if (isFoldingClick()) {
      onToggle();
    }
  };
}
