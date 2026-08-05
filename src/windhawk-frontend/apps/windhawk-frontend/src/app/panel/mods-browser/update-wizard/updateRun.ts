/**
 * The rows and counts a batch update reports, derived from what the run recorded.
 *
 * The run answers one mod at a time and records an outcome as each reply arrives,
 * which is the live list as it stands. What is built here is the result list: the
 * selected mods in the order they were listed, so a run that stopped early still
 * accounts for every mod it was given.
 */

// What became of one mod. 'aborted' has no reply behind it: it is a mod the run
// never reached, or one it reached and was canceled out of.
export type ModUpdateStatus = 'updated' | 'failed' | 'aborted';

export type ModUpdateOutcome = {
  modId: string;
  status: ModUpdateStatus;
};

export type ModUpdateCounts = {
  updated: number;
  failed: number;
  aborted: number;
  total: number;
};

/**
 * The result list once the run has stopped: every selected mod, carrying its
 * outcome or tagged 'aborted' where the run never answered for it, so the counts
 * cover everything the user selected. A canceled run over eight mods that
 * finished two reads 2 updated + 6 aborted rather than a list of two.
 */
export function finalRows(
  selectedModIds: string[],
  outcomes: ModUpdateOutcome[]
): ModUpdateOutcome[] {
  const recorded = new Map(
    outcomes.map((outcome) => [outcome.modId, outcome] as const)
  );
  return selectedModIds.map(
    (modId) => recorded.get(modId) ?? { modId, status: 'aborted' as const }
  );
}

export function countOutcomes(rows: ModUpdateOutcome[]): ModUpdateCounts {
  const counts: ModUpdateCounts = {
    updated: 0,
    failed: 0,
    aborted: 0,
    total: rows.length,
  };
  for (const row of rows) {
    counts[row.status] += 1;
  }
  return counts;
}
