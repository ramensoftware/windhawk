import { useEffect, useRef, useState } from 'react';
import { flushSync } from 'react-dom';

// More rows than this left to draw are drawn progressively.
const PROGRESSIVE_FROM = 50;
const FIRST_ROWS = 20;

// What drawing one chunk may take, and so about how long input waits on one.
export const CHUNK_BUDGET_MS = 50;

// The first chunk's rows, before any has been timed.
const FIRST_CHUNK_ROWS = 5;

/**
 * The rows the next chunk draws, after one of `rows` took `elapsedMs`: as many
 * as the budget fits at that rate, and at most twice as many as before, so one
 * chunk that came out quick does not size the next one long.
 */
export function nextChunkRows(rows: number, elapsedMs: number): number {
  const fitting = elapsedMs > 0 ? Math.floor((rows * CHUNK_BUDGET_MS) / elapsedMs) : Infinity;
  return Math.max(1, Math.min(rows * 2, fitting));
}

// How many of `total` rows to draw in a render, `drawn` of them being drawn
// already: all of them, or, with too many left, those drawn already and at
// least FIRST_ROWS.
function firstCount(total: number, drawn: number): number {
  return total - drawn > PROGRESSIVE_FROM ? Math.max(drawn, FIRST_ROWS) : Infinity;
}

/**
 * How many of an array's `total` rows to draw. A long array is drawn a chunk at
 * a time: its first rows with the form, then a chunk a task, each drawn and
 * timed on its own and the next sized to the time budget, so the page paints
 * and answers input in between however costly a row is. A short array is drawn
 * whole, and so are a few rows added to an array drawn in full; many added rows
 * are drawn a chunk at a time after those drawn already.
 *
 * `resetKey` names what the rows are drawn from, and another one starts the
 * drawing over.
 */
export function useProgressiveCount(total: number, resetKey: unknown): number {
  const [drawn, setDrawn] = useState(() => firstCount(total, 0));
  const [drawnFor, setDrawnFor] = useState({ total, resetKey });
  if (drawnFor.resetKey !== resetKey) {
    setDrawnFor({ total, resetKey });
    setDrawn(firstCount(total, 0));
  } else if (drawnFor.total !== total) {
    setDrawnFor({ total, resetKey });
    if (drawn >= drawnFor.total) {
      setDrawn(firstCount(total, drawnFor.total));
    }
  }

  const chunkRows = useRef(FIRST_CHUNK_ROWS);

  useEffect(() => {
    if (drawn >= total) {
      return;
    }
    const timer = setTimeout(() => {
      const next = drawn + chunkRows.current;
      const start = performance.now();
      flushSync(() => setDrawn(next >= total ? Infinity : next));
      chunkRows.current = nextChunkRows(chunkRows.current, performance.now() - start);
    }, 0);
    return () => clearTimeout(timer);
  }, [drawn, total]);

  return Math.min(drawn, total);
}
