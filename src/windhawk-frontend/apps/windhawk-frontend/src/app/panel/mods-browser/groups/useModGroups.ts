import { useCallback, useState } from 'react';
import {
  assignToGroup,
  deleteGroup,
  type ModGroup,
  type ModGroupDestination,
  renameGroup,
  setGroupCollapsed,
  swapGroups,
} from './modGroups';
import { readModGroups, writeModGroups } from './modGroupsStorage';

export type ModGroups = {
  groups: ModGroup[];
  assign: (modIds: string[], destination: ModGroupDestination) => void;
  rename: (groupId: string, name: string) => void;
  remove: (groupId: string) => void;
  swap: (groupId: string, otherGroupId: string) => void;
  setCollapsed: (groupId: string, collapsed: boolean) => void;
};

/**
 * The groups a screen holds, and every edit bound to writing them back.
 *
 * Storage is read once, in a lazy initializer, and written on each edit.
 */
export function useModGroups(): ModGroups {
  const [groups, setGroups] = useState<ModGroup[]>(readModGroups);

  // The write is deliberately not inside the setGroups updater. A state updater
  // has to be pure - React invokes it twice under StrictMode - and while a
  // duplicate localStorage write is harmless, persistence that only works
  // because the side effect happened to be idempotent is one edit away from not.
  // Nor is it an effect on `groups`, which would also write on mount, putting
  // the parsed-and-normalized list back over whatever was there on every visit
  // by a user who never touches a group. The edits are all user-driven and never
  // batched with each other, so reading `groups` off the render is safe.
  const update = useCallback(
    (edit: (current: ModGroup[]) => ModGroup[]) => {
      const next = edit(groups);
      writeModGroups(next);
      setGroups(next);
    },
    [groups]
  );

  const assign = useCallback(
    (modIds: string[], destination: ModGroupDestination) =>
      update((current) => {
        const next = assignToGroup(current, modIds, destination);
        // The destination is opened by the move: mods vanishing into a fold
        // reads as losing them. A new group is created open, and there is no
        // destination to open when the mods are being taken out of one.
        return destination.type === 'existing'
          ? setGroupCollapsed(next, destination.groupId, false)
          : next;
      }),
    [update]
  );

  const rename = useCallback(
    (groupId: string, name: string) =>
      update((current) => renameGroup(current, groupId, name)),
    [update]
  );

  const remove = useCallback(
    (groupId: string) => update((current) => deleteGroup(current, groupId)),
    [update]
  );

  const swap = useCallback(
    (groupId: string, otherGroupId: string) =>
      update((current) => swapGroups(current, groupId, otherGroupId)),
    [update]
  );

  const setCollapsed = useCallback(
    (groupId: string, collapsed: boolean) =>
      update((current) => setGroupCollapsed(current, groupId, collapsed)),
    [update]
  );

  return { groups, assign, rename, remove, swap, setCollapsed };
}

export default useModGroups;
