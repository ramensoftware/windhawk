import { faThumbsUp } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, Popover, Spin } from 'antd';
import { type ReactNode, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import ModReviewsModal, {
  ReviewContent,
  formatReviewDate,
  VersionChip,
  VoteCount,
} from './ModReviewsModal';
import { findVote, effectiveVotes, topReview } from './modReviews';
import { useModReviews } from './useModReviews';
import { useModReviewVotes } from './useModReviewVotes';

// A pointer crossing a grid of cards is not asking about each of them: the
// popover, and the fetch it makes on its first open, wait for it to settle.
const OPEN_DELAY_SECONDS = 0.3;

const PopoverBody = styled.div`
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 320px;
  max-width: 80vw;
`;

const TitleLine = styled.div`
  font-weight: 600;
`;

const Muted = styled.div`
  color: var(--whui-text-muted);
  font-style: italic;
`;

const ReviewHeader = styled.div`
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  column-gap: 8px;
`;

const ReviewAuthor = styled.span`
  font-weight: 600;
`;

const ReviewDate = styled.span`
  color: var(--whui-text-muted);
  font-size: 12px;
`;

const ClampedContent = styled(ReviewContent)`
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 4;
  overflow: hidden;
`;

const ReviewFooter = styled.div`
  display: flex;
  align-items: center;
  justify-content: space-between;
`;

const ShowMore = styled(Button)`
  padding: 0;
  height: auto;
`;

const nowSeconds = () => Math.floor(Date.now() / 1000);

// Mounted on the popover's first open, which is when its reads go out.
function ModReviewsPopoverContent({
  modId,
  reviews,
  onShowMore,
}: {
  modId: string;
  reviews: number;
  onShowMore: () => void;
}) {
  const { t, i18n } = useTranslation();

  // A count of zero is the whole answer: nothing is fetched for it.
  const key = reviews > 0 ? modId : null;
  const { document, error, isLoading } = useModReviews(key);
  const { votes } = useModReviewVotes(key);
  const [now] = useState(nowSeconds);

  const top = document ? topReview(document, votes ?? [], now) : null;
  const topVote = top ? findVote(votes ?? [], top.id) : undefined;

  return (
    <PopoverBody data-testid="mod-reviews-popover">
      <TitleLine>
        {t('mod.reviews', {
          count: reviews,
          formattedCount: reviews.toLocaleString(),
        })}
      </TitleLine>
      {reviews === 0 ? (
        <Muted>{t('mod.reviews.none')}</Muted>
      ) : error ? (
        <Muted>{t('mod.reviews.loadFailed')}</Muted>
      ) : isLoading || !document ? (
        <Spin size="small" />
      ) : !top ? (
        <Muted>{t('mod.reviews.none')}</Muted>
      ) : (
        <>
          <ReviewHeader>
            <ReviewAuthor>{top.authorName}</ReviewAuthor>
            {top.modVersion && <VersionChip>{top.modVersion}</VersionChip>}
            <ReviewDate>{formatReviewDate(top.timestamp, i18n.language)}</ReviewDate>
          </ReviewHeader>
          <ClampedContent content={top.content} />
          <ReviewFooter>
            <VoteCount
              $voted={topVote !== undefined}
              title={
                topVote !== undefined
                  ? t('mod.reviews.voted')
                  : t('mod.reviews.vote')
              }
            >
              <FontAwesomeIcon icon={faThumbsUp} />
              {effectiveVotes(top, topVote, now)}
            </VoteCount>
            <ShowMore
              type="link"
              size="small"
              data-testid="mod-reviews-show-more"
              onClick={onShowMore}
            >
              {t('mod.reviews.showMore')}
            </ShowMore>
          </ReviewFooter>
        </>
      )}
    </PopoverBody>
  );
}

interface Props {
  modId: string;
  modName: string;
  // The catalog's count, which is what the card draws before any document is
  // fetched.
  reviews: number;
  children: ReactNode;
}

/**
 * The card's hover on its reviews count: how many, the top one, and the way
 * to all of them. Read-only - a hover surface that writes is a misclick
 * surface - so the vote is the modal's, one click away. The modal's open state
 * is held here, the card having nothing else to hold it.
 */
export function ModReviewsPopover({ modId, modName, reviews, children }: Props) {
  const [popoverOpen, setPopoverOpen] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);

  return (
    <>
      <Popover
        open={popoverOpen}
        // Kept shut while the modal is up: the pointer entering the popover
        // schedules an open (the hover delay) that closing it on "Show more"
        // does not cancel.
        onOpenChange={(open) => setPopoverOpen(open && !modalOpen)}
        mouseEnterDelay={OPEN_DELAY_SECONDS}
        placement="bottom"
        content={
          <ModReviewsPopoverContent
            modId={modId}
            reviews={reviews}
            onShowMore={() => {
              setPopoverOpen(false);
              setModalOpen(true);
            }}
          />
        }
      >
        {children}
      </Popover>
      {modalOpen && (
        <ModReviewsModal
          modId={modId}
          modName={modName}
          onClose={() => setModalOpen(false)}
        />
      )}
    </>
  );
}

export default ModReviewsPopover;
