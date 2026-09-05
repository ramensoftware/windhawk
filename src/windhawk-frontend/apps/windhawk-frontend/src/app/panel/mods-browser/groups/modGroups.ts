/**
 * Groups of installed mods: the model, and every edit as a function from a group
 * list to a group list.
 *
 * Pure - no React, no storage, no knowledge of a view. Two invariants hold over
 * every list this module hands back, and the parse below restates them as the
 * reader's job because storage is the one input the edits do not control:
 * ids are unique, and a mod belongs to at most one group.
 */

export type ModGroup = {
  id: string;
  name: string;
  collapsed: boolean;
  modIds: string[];
};

/** Where a move sends the selected mods. */
export type ModGroupDestination =
  | { type: 'existing'; groupId: string }
  | { type: 'new'; name: string }
  | { type: 'none' };

/** One list on screen: the mods in no group, then one per group in order. */
export type ModGroupBlock = {
  // null for the mods that are in no group.
  group: ModGroup | null;
  modIds: string[];
};

// The shape written to storage. A later shape can be recognized rather than
// guessed at: anything that is not this reads as no groups, which is the same
// outcome as a fresh install.
const STORAGE_VERSION = 1;

// What a generated id begins with, so a stored blob says what its keys are.
const GROUP_ID_PREFIX = 'group-';

export function parseModGroups(raw: unknown): ModGroup[] {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) {
    return [];
  }

  const { version, groups } = raw as { version?: unknown; groups?: unknown };
  if (version !== STORAGE_VERSION || !Array.isArray(groups)) {
    return [];
  }

  // Degrades per entry rather than per file: one unreadable group costs that
  // group and not every other one the user made.
  const parsed: ModGroup[] = [];
  const takenIds = new Set<string>();
  const claimedModIds = new Set<string>();

  for (const entry of groups) {
    if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
      continue;
    }

    const { id, name, collapsed, modIds } = entry as {
      id?: unknown;
      name?: unknown;
      collapsed?: unknown;
      modIds?: unknown;
    };

    if (typeof id !== 'string' || id === '') {
      continue;
    }
    if (typeof name !== 'string' || name === '') {
      continue;
    }
    if (takenIds.has(id)) {
      continue;
    }
    takenIds.add(id);

    // First claim wins, so a mod named by two groups is in the first of them and
    // the renderer never has to ask which block a card belongs in.
    const members: string[] = [];
    if (Array.isArray(modIds)) {
      for (const modId of modIds) {
        if (typeof modId === 'string' && !claimedModIds.has(modId)) {
          claimedModIds.add(modId);
          members.push(modId);
        }
      }
    }

    parsed.push({ id, name, collapsed: collapsed === true, modIds: members });
  }

  return parsed;
}

export function serializeModGroups(groups: ModGroup[]): unknown {
  return { version: STORAGE_VERSION, groups };
}

/**
 * An id for a new group: no group in the list holds it, and no group the list
 * has already lost held it either.
 *
 * Drawn rather than counted, which is what buys the second half. An id is what
 * everything outside this module holds a group by, and what such a holder keeps
 * after the group is gone; one handed out twice would make it stand for the
 * group that had it before. A number one past the highest in the list is freed
 * by deleting the group that carries it.
 *
 * Nothing checks the draw against the list: at 122 bits it carries uniqueness
 * on its own, and the parse above is what a blob edited by hand runs into.
 */
export function newGroupId(): string {
  return GROUP_ID_PREFIX + crypto.randomUUID();
}

/**
 * Whether a group already goes by this name.
 *
 * Compared without surrounding space and without case: two groups a user cannot
 * tell apart on the header line are two the user did not mean to have.
 * `exceptGroupId` is the group being renamed, which may keep the name it has.
 */
export function groupNameTaken(
  groups: ModGroup[],
  name: string,
  exceptGroupId?: string
): boolean {
  const wanted = name.trim().toLowerCase();
  return groups.some(
    (group) =>
      group.id !== exceptGroupId && group.name.trim().toLowerCase() === wanted
  );
}

/**
 * The listed mods split into the lists they are drawn as: the ones in no group
 * first, then one per group in the group list's order.
 *
 * `listedModIds` is what the search box and the filter menu left, in the order
 * the view lists them, and every block keeps that order - so the sort a view
 * applies is applied once, before this, and inherited by every block.
 *
 * Every group gets a block, including one that lists nothing; whether an empty
 * block is drawn is the renderer's call. A mod that belongs to no group, or to
 * one that is not in the list, lands in the ungrouped block.
 */
export function partitionByGroup(
  groups: ModGroup[],
  listedModIds: string[]
): ModGroupBlock[] {
  const groupIdByModId = new Map<string, string>();
  const membersByGroupId = new Map<string, string[]>();

  for (const group of groups) {
    membersByGroupId.set(group.id, []);
    for (const modId of group.modIds) {
      if (!groupIdByModId.has(modId)) {
        groupIdByModId.set(modId, group.id);
      }
    }
  }

  const ungrouped: string[] = [];
  for (const modId of listedModIds) {
    const groupId = groupIdByModId.get(modId);
    const members = groupId === undefined ? undefined : membersByGroupId.get(groupId);
    if (members) {
      members.push(modId);
    } else {
      ungrouped.push(modId);
    }
  }

  return [
    { group: null, modIds: ungrouped },
    ...groups.map((group) => ({
      group,
      modIds: membersByGroupId.get(group.id) as string[],
    })),
  ];
}

/**
 * The mods moved to one destination, and out of wherever they were.
 *
 * The removal comes first whatever the destination is, which is the one order
 * that enforces one home per mod - and is the whole of what `{ type: 'none' }`
 * amounts to.
 */
export function assignToGroup(
  groups: ModGroup[],
  modIds: string[],
  destination: ModGroupDestination
): ModGroup[] {
  // A group with no name is not a group, and a second group under a name that is
  // taken is one the user cannot tell from the first. The dialog refuses both
  // first; refusing them here as well is what makes the rules exhaustible by a
  // test.
  if (
    destination.type === 'new' &&
    (destination.name.trim() === '' || groupNameTaken(groups, destination.name))
  ) {
    return groups;
  }

  const moving = [...new Set(modIds)];
  const movingSet = new Set(moving);

  const without = groups.map((group) => {
    const kept = group.modIds.filter((modId) => !movingSet.has(modId));
    return kept.length === group.modIds.length ? group : { ...group, modIds: kept };
  });

  switch (destination.type) {
    case 'none':
      return without;
    case 'existing':
      return without.map((group) =>
        group.id === destination.groupId
          ? { ...group, modIds: [...group.modIds, ...moving] }
          : group
      );
    case 'new':
      return [
        ...without,
        {
          id: newGroupId(),
          name: destination.name.trim(),
          collapsed: false,
          modIds: moving,
        },
      ];
  }
}

export function renameGroup(
  groups: ModGroup[],
  groupId: string,
  name: string
): ModGroup[] {
  const trimmed = name.trim();
  if (trimmed === '' || groupNameTaken(groups, trimmed, groupId)) {
    return groups;
  }

  return groups.map((group) =>
    group.id === groupId ? { ...group, name: trimmed } : group
  );
}

/**
 * The group dropped. Its members are in no group afterwards because nothing else
 * claims them, so nothing has to move.
 */
export function deleteGroup(groups: ModGroup[], groupId: string): ModGroup[] {
  return groups.filter((group) => group.id !== groupId);
}

/**
 * Two groups swapped, and the identity unless the list holds both.
 *
 * The other group is named rather than stepped to, because which group a group
 * is next to is a question about the list on screen - a filter can leave one
 * undrawn - and this module has no view to ask. Every other group keeps the
 * place it had, undrawn ones among them.
 */
export function swapGroups(
  groups: ModGroup[],
  groupId: string,
  otherGroupId: string
): ModGroup[] {
  const index = groups.findIndex((group) => group.id === groupId);
  const otherIndex = groups.findIndex((group) => group.id === otherGroupId);
  if (index === -1 || otherIndex === -1) {
    return groups;
  }

  const next = [...groups];
  next[index] = groups[otherIndex];
  next[otherIndex] = groups[index];
  return next;
}

export function setGroupCollapsed(
  groups: ModGroup[],
  groupId: string,
  collapsed: boolean
): ModGroup[] {
  return groups.map((group) =>
    group.id === groupId ? { ...group, collapsed } : group
  );
}
