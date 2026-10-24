import { faChevronDown, faThumbsUp } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, ConfigProvider, Modal, Segmented } from 'antd';
import { useContext, useEffect, useId, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled, { css } from 'styled-components';
import { MODAL_FOOTERLESS_STYLES } from '@app/antdTheme';
import { ProgressSpin } from '@app/components/ProgressSpin';
import ReactMarkdownCustom from '@app/components/ReactMarkdownCustom';
import { showErrorMessage } from '@app/feedback';
import { formatShortDate } from '@app/utils';
import type { ModReviewVote } from '@app/webviewIPCMessages';
import { foldingClickHandler } from '../foldingClick';
import useModalClose from '../useModalClose';
import ModReviewForm from './ModReviewForm';
import {
  type ReviewOrder,
  type ReviewThread,
  type ModReview,
  VOTE_UNDO_WINDOW_MS,
  effectiveVotes,
  findVote,
  groupThreads,
  isVoteUndoable,
} from './modReviews';
import { useModReviews } from './useModReviews';
import { useModReviewVotes } from './useModReviewVotes';

declare const WEBPACK_IS_WEBSITE: boolean;

const Message = styled.div`
  color: var(--whui-text-muted);
  font-style: italic;
`;

// The sort, in the corner over the first review's header line rather than on
// a line of its own, so the list starts where it does without it: the row it
// sits in has no height.
const SortCorner = styled.div`
  display: flex;
  justify-content: flex-end;
  align-items: flex-start;
  height: 0;
`;

// Centered on the header line by hand, the small segmented being 28px to the
// line's 22px.
const SortControl = styled.div`
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: -3px;
  color: var(--whui-text-secondary);
  font-size: 12px;
`;

const ThreadList = styled.div`
  display: flex;
  flex-direction: column;
  gap: 16px;
`;

const Thread = styled.div`
  display: flex;
  flex-direction: column;
  gap: 8px;
`;

const CommentBlock = styled.div<{ $reply?: boolean }>`
  display: flex;
  flex-direction: column;
  gap: 4px;

  ${({ $reply }) =>
    $reply &&
    css`
      margin-inline-start: 24px;
      padding-inline-start: 12px;
      border-inline-start: 2px solid var(--whui-border);
    `}
`;

// What the header line keeps between the things sharing it.
const HEADER_GAP = 8;

const CommentHeader = styled.div`
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  column-gap: ${HEADER_GAP}px;
`;

// The glyph's box: the 12px the app's other folds draw theirs at, so a fold
// reads the same wherever the app offers one.
const CARET_WIDTH = 12;

// The caret folding a review away. It hangs in the body's padding, pulled
// before the line by the whole of its reach, so the author begins where the
// line does and the text below stays flush with the name.
//
// It comes out when the pointer is on the review it acts on, so a list of
// reviews is a list rather than a column of carets; a folded review keeps it,
// the caret being the whole of what says the review can be opened. Where there
// is no pointer to bring it out it is drawn outright.
//
// One glyph turned rather than two swapped, so the caret moves between the two
// states instead of jumping; it turns the way the text runs, so a folded review
// points the way it would open out.
const CollapseButton = styled(Button)<{ $collapsed: boolean; $rtl: boolean }>`
  position: relative;
  flex: none;
  min-width: 0;
  width: ${CARET_WIDTH}px;
  height: auto;
  padding: 0;
  margin-inline-start: -${CARET_WIDTH + HEADER_GAP}px;
  color: var(--whui-text-secondary);
  opacity: ${({ $collapsed }) => ($collapsed ? 1 : 0)};
  transition: opacity 120ms ease-out;

  // A target wider than the glyph.
  &::before {
    content: '';
    position: absolute;
    inset: -4px -6px;
  }

  ${CommentBlock}:hover > ${CommentHeader} > &,
  &:focus-visible {
    opacity: 1;
  }

  @media (hover: none) {
    opacity: 1;
  }

  svg {
    transition: transform 120ms ease-out;

    ${({ $collapsed, $rtl }) =>
      $collapsed &&
      css`
        transform: rotate(${$rtl ? '90deg' : '-90deg'});
      `}
  }
`;

// The author, the version and the date: the line a folded review is left as,
// and on a review a press on any of them folds it, as the caret does. Laid
// out as the header's own items, so the line wraps as it would with nothing
// around them; the span is only where a press lands, and what hands the
// cursor down.
const HeaderText = styled.span<{ $foldable: boolean }>`
  display: contents;

  ${({ $foldable }) =>
    $foldable &&
    css`
      cursor: pointer;
    `}
`;

const ReviewAuthor = styled.span`
  font-weight: 600;
`;

// The chip the details header draws a mod's id in.
export const VersionChip = styled.span`
  border-radius: var(--whui-radius-sm);
  background: var(--whui-chip-bg);
  padding: 1px 4px;
  font-size: 12px;
`;

const CommentDate = styled.span`
  color: var(--whui-text-muted);
  font-size: 12px;
`;

// A review ends where its text does: the last block's own spacing would
// double the gap to the footer.
const CommentMarkdown = styled(ReactMarkdownCustom)`
  > :last-child {
    margin-bottom: 0;
  }
`;

// The text as the poster wrote it, read as markdown: a line break kept as one,
// no image drawn, in the direction of its own script rather than the UI's.
export function ReviewContent({
  content,
  className,
}: {
  content: string;
  className?: string;
}) {
  return (
    <CommentMarkdown
      className={className}
      markdown={content}
      direction="auto"
      allowImages={false}
      breaks
    />
  );
}

const CommentFooter = styled.div`
  display: flex;
  align-items: center;
  gap: 4px;
`;

// A cast vote keeps its highlight once it is final and the button disabled:
// antd's disabled look would read as a vote that cannot be cast.
const VoteButton = styled(Button)`
  &&[aria-pressed='true'] {
    color: var(--whui-primary);
    border-color: var(--whui-primary);
    background: transparent;
  }

  &&[aria-pressed='true']:disabled {
    cursor: default;
  }
`;

// The count where there is no button to carry it: the website, and a host
// whose votes could not be read.
export const VoteCount = styled.span<{ $voted?: boolean }>`
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: ${({ $voted }) => ($voted ? 'var(--whui-primary)' : 'var(--whui-text-secondary)')};
`;

// The scrolled body's padding, which the write bar's margins reach across.
const BODY_PADDING = 24;

// The form of a review of its own, under the last of the reviews.
const ComposeRow = styled.div`
  margin-top: 16px;
`;

// The way to that form, pinned to the bottom of the body however far it is
// scrolled. It spans the body edge to edge, its own padding standing in for the
// body's, so a review scrolled under it is covered rather than shown in the
// gap. The inset reaches across the padding too: a sticky box is held inside
// the scroll container's content box, and would stop a padding short of the
// edge otherwise.
const WriteBar = styled.div`
  position: sticky;
  bottom: -${BODY_PADDING}px;
  margin: 16px -${BODY_PADDING}px -${BODY_PADDING}px;
  padding: 12px ${BODY_PADDING}px 16px;
  border-top: 1px solid var(--whui-divider);
  background: var(--whui-modal-background-color);
`;

const nowSeconds = () => Math.floor(Date.now() / 1000);

// When each vote still offered back was clicked, by review id.
type CastAt = ReadonlyMap<number, number>;

function without(castAt: CastAt, reviewId: number): CastAt {
  const next = new Map(castAt);
  next.delete(reviewId);
  return next;
}

export function formatReviewDate(timestamp: number, language: string): string {
  return formatShortDate(new Date(timestamp * 1000), language);
}

function CommentEntry({
  review,
  count,
  voted,
  undoable,
  voting,
  onVote,
  onReply,
  reply,
  collapsed,
  onToggleCollapsed,
}: {
  review: ModReview;
  count: number;
  voted: boolean;
  // Whether the vote can still be taken back: the button then stays live
  // under its highlight, and the click takes the vote back.
  undoable: boolean;
  // Whether a vote can be cast: absent where voting is off, pending while the
  // host's votes are still being read.
  voting: 'on' | 'pending' | 'off';
  onVote: (reviewId: number) => void;
  // The reply action, on a review where posting is on.
  onReply?: () => void;
  reply?: boolean;
  // The fold, on a review: folded, it is its header line alone. A reply has
  // none, folding with its review.
  collapsed?: boolean;
  onToggleCollapsed?: () => void;
}) {
  const { t, i18n } = useTranslation();
  const { direction } = useContext(ConfigProvider.ConfigContext);

  const voteLabel = undoable
    ? t('mod.reviews.undo')
    : voted
      ? t('mod.reviews.voted')
      : t('mod.reviews.vote');

  return (
    <CommentBlock $reply={reply} data-testid={`mod-review-${review.id}`}>
      <CommentHeader>
        {onToggleCollapsed && (
          <CollapseButton
            type="link"
            size="small"
            $collapsed={!!collapsed}
            $rtl={direction === 'rtl'}
            aria-expanded={!collapsed}
            aria-label={t(collapsed ? 'mod.reviews.expand' : 'mod.reviews.collapse', {
              name: review.authorName,
            })}
            data-testid={`mod-review-collapse-${review.id}`}
            onClick={onToggleCollapsed}
          >
            <FontAwesomeIcon icon={faChevronDown} />
          </CollapseButton>
        )}
        <HeaderText
          $foldable={!!onToggleCollapsed}
          onClick={onToggleCollapsed && foldingClickHandler(onToggleCollapsed)}
        >
          <ReviewAuthor>{review.authorName}</ReviewAuthor>
          {review.modVersion && <VersionChip>{review.modVersion}</VersionChip>}
          <CommentDate>{formatReviewDate(review.timestamp, i18n.language)}</CommentDate>
        </HeaderText>
      </CommentHeader>
      {!collapsed && (
        <>
          <ReviewContent content={review.content} />
          <CommentFooter>
            {voting === 'off' ? (
              <VoteCount $voted={voted} title={voteLabel}>
                <FontAwesomeIcon icon={faThumbsUp} />
                {count}
              </VoteCount>
            ) : (
              <VoteButton
                size="small"
                type={voted ? 'primary' : 'default'}
                ghost={voted}
                aria-pressed={voted}
                aria-label={voteLabel}
                title={voteLabel}
                disabled={(voted && !undoable) || voting === 'pending'}
                data-testid={`mod-review-vote-${review.id}`}
                onClick={() => onVote(review.id)}
              >
                <FontAwesomeIcon icon={faThumbsUp} />
                {count}
              </VoteButton>
            )}
            {onReply && (
              <Button
                type="link"
                size="small"
                data-testid={`mod-review-reply-${review.id}`}
                onClick={onReply}
              >
                {t('mod.reviews.reply')}
              </Button>
            )}
          </CommentFooter>
        </>
      )}
    </CommentBlock>
  );
}

export type PostingContext = {
  // The installed version a post is stamped with, when the mod's copy names
  // one.
  modVersion?: string;
};

// What the form is open for: a review of its own, or a reply to one. Undefined
// until the user has said, which is when an empty list may open it on its own.
type Composing = { parentId?: number } | null | undefined;

function ModReviewThreads({
  modId,
  posting,
  onDraftChange,
}: {
  modId: string;
  posting?: PostingContext;
  onDraftChange: (draft: boolean) => void;
}) {
  const { t } = useTranslation();
  const sortLabelId = useId();

  const { document, error, isLoading } = useModReviews(modId);
  const { votes, recordVote, retractVote } = useModReviewVotes(modId);
  const [now] = useState(nowSeconds);
  const [order, setOrder] = useState<ReviewOrder>('top');
  const [composing, setComposing] = useState<Composing>(undefined);

  // The reviews folded to their header line, by id, so a fold survives a
  // change of order. Held for as long as the modal is up; a reopened modal
  // draws every review open.
  const [collapsedIds, setCollapsedIds] = useState<ReadonlySet<number>>(
    () => new Set()
  );
  const toggleCollapsed = (reviewId: number) =>
    setCollapsedIds((current) => {
      const next = new Set(current);
      if (!next.delete(reviewId)) {
        next.add(reviewId);
      }
      return next;
    });

  // The votes the order is drawn from: the host's as first read, held for as
  // long as the modal is up. The counts follow every vote as it is cast; the
  // order does not, or a review would jump up the list under the pointer that
  // just voted on it.
  const [orderVotes, setOrderVotes] = useState<ModReviewVote[] | null>(null);
  if (orderVotes === null && votes !== undefined) {
    setOrderVotes(votes ?? []);
  }

  // The votes this modal cast and still offers to take back. An entry goes
  // with the undo, with its minute, or with the modal, whose owner unmounts it
  // on close; a reopened modal starts with none.
  const [castAt, setCastAt] = useState<CastAt>(() => new Map());

  // A vote whose minute runs out is drawn final with no pointer move: one
  // timer to the earliest deadline drops what has run out, and the map's
  // change arms the next.
  useEffect(() => {
    let earliest = Infinity;
    for (const castAtMs of castAt.values()) {
      earliest = Math.min(earliest, castAtMs + VOTE_UNDO_WINDOW_MS);
    }
    if (earliest === Infinity) {
      return;
    }
    const timer = setTimeout(
      () => {
        const at = Date.now();
        setCastAt(
          (current) =>
            new Map(
              [...current].filter(([, castAtMs]) => isVoteUndoable(castAtMs, at))
            )
        );
      },
      Math.max(0, earliest - Date.now())
    );
    return () => clearTimeout(timer);
  }, [castAt]);

  const threads = useMemo(
    () => (document ? groupThreads(document, orderVotes ?? [], now, order) : []),
    [document, orderVotes, now, order]
  );

  if (error) {
    return <Message>{t('mod.reviews.loadFailed')}</Message>;
  }
  if (isLoading || !document) {
    return <ProgressSpin />;
  }

  // Voting needs a profile to write to, which the website has none of, and a
  // host that answered the read; while the read is out the button waits.
  const voting: 'on' | 'pending' | 'off' =
    WEBPACK_IS_WEBSITE || votes === null
      ? 'off'
      : votes === undefined
        ? 'pending'
        : 'on';

  const handleVote = async (reviewId: number) => {
    const at = Date.now();
    setCastAt((current) => new Map(current).set(reviewId, at));
    const outcome = await recordVote(reviewId);
    if (outcome !== 'written') {
      // A vote the host did not take leaves nothing to take back; the entry
      // is dropped unless a later click has replaced it.
      setCastAt((current) =>
        current.get(reviewId) === at ? without(current, reviewId) : current
      );
      if (outcome === 'failed') {
        showErrorMessage(t('mod.reviews.voteFailed'));
      }
    }
  };

  // The minute is spent on the attempt whatever the host answers: a refusal
  // would repeat, so the vote it leaves standing is final.
  const handleUndo = async (reviewId: number) => {
    setCastAt((current) => without(current, reviewId));
    if ((await retractVote(reviewId)) === 'failed') {
      showErrorMessage(t('mod.reviews.undoFailed'));
    }
  };

  const entryFor = (review: ModReview, thread?: ReviewThread) => {
    const vote = findVote(votes ?? [], review.id);
    // The map holds only what is still undoable: the timer above drops an
    // entry as its minute runs out.
    const undoable = vote !== undefined && castAt.has(review.id);
    return (
      <CommentEntry
        key={review.id}
        review={review}
        count={effectiveVotes(review, vote, now)}
        voted={vote !== undefined}
        undoable={undoable}
        voting={voting}
        onVote={undoable ? handleUndo : handleVote}
        onReply={
          posting && thread
            ? () => setComposing({ parentId: thread.review.id })
            : undefined
        }
        reply={!thread}
        collapsed={thread ? collapsedIds.has(review.id) : undefined}
        onToggleCollapsed={thread ? () => toggleCollapsed(review.id) : undefined}
      />
    );
  };

  // With nothing to read and something to say, the form is what there is.
  const composingNow =
    composing === undefined && posting && threads.length === 0 ? {} : composing;

  const form = (parentId?: number) => (
    <ModReviewForm
      modId={modId}
      parentId={parentId}
      modVersion={posting?.modVersion}
      onCancel={() => setComposing(null)}
      onDraftChange={onDraftChange}
    />
  );

  return (
    <>
      {threads.length > 1 && (
        <SortCorner>
          <SortControl>
            <span id={sortLabelId}>{t('mod.reviews.sort.label')}</span>
            {/* The empty title says not to draw the tooltip antd would take
                from the label, which repeats what is already read. */}
            <Segmented
              size="small"
              aria-labelledby={sortLabelId}
              data-testid="mod-reviews-sort"
              value={order}
              options={[
                { value: 'top', label: t('mod.reviews.sort.top'), title: '' },
                { value: 'newest', label: t('mod.reviews.sort.newest'), title: '' },
              ]}
              onChange={(value) => setOrder(value === 'newest' ? 'newest' : 'top')}
            />
          </SortControl>
        </SortCorner>
      )}
      {threads.length === 0 ? (
        <Message>{t('mod.reviews.none')}</Message>
      ) : (
        <ThreadList>
          {threads.map((thread) => (
            <Thread key={thread.review.id}>
              {entryFor(thread.review, thread)}
              {/* A fold takes the replies with the review's text. The reply
                  being written under it stays: it is where the user is
                  working, and a fold does not lose a draft. */}
              {!collapsedIds.has(thread.review.id) &&
                thread.replies.map((replyComment) => entryFor(replyComment))}
              {composingNow?.parentId === thread.review.id && form(thread.review.id)}
            </Thread>
          ))}
        </ThreadList>
      )}
      {posting &&
        (composingNow && composingNow.parentId === undefined ? (
          <ComposeRow>{form()}</ComposeRow>
        ) : (
          <WriteBar>
            <Button
              size="small"
              data-testid="mod-review-write"
              onClick={() => setComposing({})}
            >
              {t('mod.reviews.write')}
            </Button>
          </WriteBar>
        ))}
    </>
  );
}

interface Props {
  modId: string;
  modName: string;
  // Present where the modal was opened for a mod installed on the machine,
  // which is what a review is written about; absent, the modal reads only.
  posting?: PostingContext;
  onClose: () => void;
}

/**
 * Every review of a mod with its replies, by votes or by date, each folding to
 * its header line; the one place a vote is cast, and taken back for a minute
 * after, and - opened for an installed mod - where a review or a reply is
 * written. Mounted by its owner while it is up.
 */
export function ModReviewsModal({ modId, modName, posting, onClose }: Props) {
  const { t } = useTranslation();
  const { open, close, afterClose } = useModalClose(onClose);

  // A click beside the dialog is as often a slip as a dismissal; with a post
  // half written it is not taken as one, and the close button is the way out.
  const [draft, setDraft] = useState(false);

  return (
    <Modal
      open={open}
      afterClose={afterClose}
      title={t('mod.reviews.title', { name: modName })}
      width={640}
      centered
      footer={null}
      mask={{ closable: !draft }}
      onCancel={close}
      wrapProps={{ 'data-testid': 'mod-reviews-modal' }}
      styles={{
        container: MODAL_FOOTERLESS_STYLES.container,
        body: {
          ...MODAL_FOOTERLESS_STYLES.body,
          padding: BODY_PADDING,
          maxHeight: '70vh',
          overflow: 'auto',
        },
      }}
    >
      <ModReviewThreads modId={modId} posting={posting} onDraftChange={setDraft} />
    </Modal>
  );
}

export default ModReviewsModal;
