/**
 * The one request the reviews feature makes with a body: a review or a reply,
 * posted from the webview (or the website) straight to the update server's
 * moderated endpoint. It lands as pending and appears in the mod's document
 * once approved and deployed; nothing here echoes it locally.
 *
 * The request carries no credentials: the endpoint is unauthenticated, and a
 * credential-free request is what lets the server answer the cross-origin
 * preflight with a plain allow-origin header for the app's origins.
 */

import { ReviewPostError } from './reviewPostError';
import { type ReviewPostFields } from './modReviews';
/// #if HAS_MOCKS
import backendApi from '@app/backendApi';
import { activeMockData } from '@app/mocking/mockScenarios';
/// #endif

declare const WEBPACK_HAS_MOCKS: boolean;

// Beside the reader's URL in useModReviews: the two hosts this feature talks
// to, one for the static document and one for the post.
export const MOD_REVIEW_POST_URL = 'https://update.windhawk.net/reviews/post.php';

export type ReviewPostReply = { id: number };

async function postModReviewToServer(
  fields: ReviewPostFields
): Promise<ReviewPostReply> {
  const response = await fetch(MOD_REVIEW_POST_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'omit',
    body: JSON.stringify(fields),
  });
  if (response.ok) {
    const reply = (await response.json()) as ReviewPostReply;
    return { id: reply.id };
  }

  // A refusal the server explained is reported in its words; anything else, as
  // the status it came with.
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    body = null;
  }
  const error =
    body && typeof body === 'object'
      ? (body as { error?: unknown }).error
      : undefined;
  if (typeof error === 'string') {
    throw new ReviewPostError(response.status, error);
  }
  throw new Error(`Request failed with status ${response.status}`);
}

/// #if HAS_MOCKS
// With no host to speak of a server either, the post is answered from the mock
// registry, as the commands are - unless the registry says the request goes out
// for real.
function postModReviewWithMock(fields: ReviewPostFields): Promise<ReviewPostReply> {
  if (backendApi || activeMockData.commentsOverNetwork) {
    return postModReviewToServer(fields);
  }
  return activeMockData.postModReview(fields);
}
/// #endif

export const postModReview: (fields: ReviewPostFields) => Promise<ReviewPostReply> =
  WEBPACK_HAS_MOCKS ? postModReviewWithMock : postModReviewToServer;
