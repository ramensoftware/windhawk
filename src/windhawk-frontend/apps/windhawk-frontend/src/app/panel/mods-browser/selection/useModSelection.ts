import { useCallback, useRef, useState } from 'react';

// What a selection of installed mods holds, and the two rules that keep it
// honest: it may only hold mods that are listed, and a shift-click fills in the
// run from wherever the last plain click landed.
export type ModSelection = {
  selectedIds: Set<string>;
  isSelected: (modId: string) => boolean;
  toggle: (modId: string, checked: boolean, shiftKey: boolean) => void;
  setSelection: (modIds: string[]) => void;
  selectAll: () => void;
  clear: () => void;
};

/**
 * The selection behind the mods browser's checkboxes and its selection bar.
 *
 * `visibleModIds` is the listed order after the search box and the filter menu
 * have had their say, and it does three jobs: it bounds what the selection may
 * hold, it gives `selectAll` its set, and it gives a shift-click the run of mods
 * to fill in.
 *
 * It is nullable because the caller cannot always vouch for it. A screen that
 * has not received its mods yet, or has not settled its filter snapshot, renders
 * an empty list for a frame; pruning against that would silently empty the
 * selection on every filter change. `null` says "no list I can trust", and
 * nothing is pruned until one arrives.
 */
export function useModSelection(visibleModIds: string[] | null): ModSelection {
  const [selectedIds, setSelectedIds] = useState<Set<string>>(
    () => new Set<string>()
  );

  // Where a shift-click ranges from: the mod of the last plain toggle. Held in a
  // ref rather than in state - moving it changes what the next click does, never
  // what is on screen now.
  const anchorRef = useRef<string | null>(null);

  // A selected mod that leaves the list leaves the selection. One rule covers a
  // filter narrowing, a mod removed from its own row, and a refetch that finds a
  // mod gone from disk. Done during render, the way the screen's own filter
  // snapshot reconciles itself, so the count never renders one frame stale.
  let visibleSelectedIds = selectedIds;
  if (visibleModIds && selectedIds.size > 0) {
    const visible = new Set(visibleModIds);
    let anyHidden = false;
    for (const modId of selectedIds) {
      if (!visible.has(modId)) {
        anyHidden = true;
        break;
      }
    }
    if (anyHidden) {
      visibleSelectedIds = new Set<string>();
      for (const modId of selectedIds) {
        if (visible.has(modId)) {
          visibleSelectedIds.add(modId);
        }
      }
      setSelectedIds(visibleSelectedIds);
    }
  }

  const toggle = useCallback(
    (modId: string, checked: boolean, shiftKey: boolean) => {
      setSelectedIds((prev) => {
        const next = new Set(prev);
        const listed = visibleModIds ?? [];
        const anchorIndex = anchorRef.current
          ? listed.indexOf(anchorRef.current)
          : -1;
        const clickedIndex = listed.indexOf(modId);

        // A shift-click with an anchor still on the list takes the run between
        // the two, inclusive, and puts every mod in it in the clicked mod's new
        // state. With no anchor to range from it is a plain toggle.
        const range =
          shiftKey && anchorIndex !== -1 && clickedIndex !== -1
            ? listed.slice(
                Math.min(anchorIndex, clickedIndex),
                Math.max(anchorIndex, clickedIndex) + 1
              )
            : [modId];

        for (const id of range) {
          if (checked) {
            next.add(id);
          } else {
            next.delete(id);
          }
        }

        return next;
      });

      if (!shiftKey) {
        anchorRef.current = modId;
      }
    },
    [visibleModIds]
  );

  const setSelection = useCallback((modIds: string[]) => {
    setSelectedIds(new Set(modIds));
  }, []);

  const selectAll = useCallback(() => {
    setSelection(visibleModIds ?? []);
  }, [setSelection, visibleModIds]);

  const clear = useCallback(() => {
    setSelection([]);
    // Nothing is left for a range to start from, and an anchor that outlived the
    // selection would have the next shift-click fill in a run the user never
    // began.
    anchorRef.current = null;
  }, [setSelection]);

  const isSelected = useCallback(
    (modId: string) => visibleSelectedIds.has(modId),
    [visibleSelectedIds]
  );

  return {
    selectedIds: visibleSelectedIds,
    isSelected,
    toggle,
    setSelection,
    selectAll,
    clear,
  };
}

export default useModSelection;
