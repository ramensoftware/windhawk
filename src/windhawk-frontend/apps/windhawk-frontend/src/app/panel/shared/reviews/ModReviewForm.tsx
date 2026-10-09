import { Button } from 'antd';
import { type InputRef } from 'antd/lib/input';
import { type TextAreaRef } from 'antd/lib/input/TextArea';
import {
  type FormEvent,
  type ReactNode,
  useEffect,
  useId,
  useRef,
  useState,
} from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import {
  InputWithContextMenu,
  TextAreaWithContextMenu,
} from '@app/components/InputWithContextMenu';
import { readReviewAuthor, writeReviewAuthor } from './reviewAuthorStorage';
import { ReviewPostError } from './reviewPostError';
import {
  CONTENT_MAX,
  EMAIL_MAX,
  NAME_MAX,
  type ReviewPostField,
  normalizeReviewPost,
  validateReviewPost,
} from './modReviews';
import { postModReview } from './postModReview';

const MOD_ISSUES_URL = 'https://github.com/ramensoftware/windhawk-mods/issues';

const FormBlock = styled.form`
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 12px;
  border: 1px solid var(--whui-border);
  border-radius: 2px;
`;

const FieldRow = styled.div`
  display: flex;
  flex-wrap: wrap;
  gap: 12px;

  > * {
    flex: 1 1 200px;
  }
`;

const FieldBlock = styled.div`
  display: flex;
  flex-direction: column;
  gap: 4px;
`;

const FieldLabel = styled.label`
  font-size: 12px;
  color: var(--whui-text-secondary);
`;

const Note = styled.div`
  font-size: 12px;
  color: var(--whui-text-muted);
`;

const Problem = styled(Note)`
  color: #ff4d4f;
`;

// The line under a field: its rule or its hint, and at the end what else the
// field says of itself.
const NoteRow = styled(Note)`
  display: flex;
  align-items: baseline;
  gap: 12px;
`;

const NoteAside = styled.span`
  margin-inline-start: auto;
  white-space: nowrap;
`;

const FormActions = styled.div`
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
`;

const VersionNote = styled(Note)`
  margin-inline-start: auto;
`;

const PostedLine = styled.div`
  padding: 12px;
  border: 1px solid var(--whui-border);
  border-radius: 2px;
  color: var(--whui-text-secondary);
`;

function Field({
  id,
  label,
  hint,
  error,
  aside,
  children,
}: {
  id: string;
  label: string;
  hint?: string;
  error?: string;
  aside?: ReactNode;
  children: ReactNode;
}) {
  const note = error ? <Problem role="alert">{error}</Problem> : hint;
  return (
    <FieldBlock>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      {children}
      {(note || aside) && (
        <NoteRow>
          {note}
          {aside && <NoteAside>{aside}</NoteAside>}
        </NoteRow>
      )}
    </FieldBlock>
  );
}

interface Props {
  modId: string;
  // The review this is a reply to; absent for a review of its own.
  parentId?: number;
  // The installed version the post is stamped with, when known.
  modVersion?: string;
  onCancel: () => void;
  // Told whether the form holds something typed that closing would lose.
  onDraftChange?: (draft: boolean) => void;
}

/**
 * A review or a reply on its way to the server: name and email, the text, and
 * the version it is about. The post's rules are applied from the first attempt
 * to post on, so a post the server would refuse is stopped before any request,
 * at the first field breaking a rule: focused, with the rule under it. A
 * refusal the server explains anyway is drawn in its words. Once the post
 * lands the form gives way to a line saying so, and nothing of the post is
 * shown until it is approved.
 */
function ModReviewForm({
  modId,
  parentId,
  modVersion,
  onCancel,
  onDraftChange,
}: Props) {
  const { t } = useTranslation();
  const idBase = useId();

  // A string the two kinds of post differ on sits under the kind's block; the
  // rest at the form's level.
  const kind = parentId === undefined ? 'review' : 'reply';
  const tForm = (key: string, options?: Record<string, unknown>) =>
    t([`mod.reviews.form.${kind}.${key}`, `mod.reviews.form.${key}`], options);

  const [author] = useState(() => readReviewAuthor());
  const [name, setName] = useState(author?.name ?? '');
  const [email, setEmail] = useState(author?.email ?? '');
  const [content, setContent] = useState('');
  const [submitAttempted, setSubmitAttempted] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [posted, setPosted] = useState(false);

  // Something typed beyond what the form opened with, until the post lands.
  const draft =
    !posted &&
    (content !== '' ||
      name !== (author?.name ?? '') ||
      email !== (author?.email ?? ''));
  useEffect(() => {
    if (!draft) {
      return;
    }
    onDraftChange?.(true);
    return () => onDraftChange?.(false);
  }, [draft, onDraftChange]);

  const fields = normalizeReviewPost({
    modId,
    authorName: name,
    authorEmail: email,
    content,
    parentId,
    modVersion,
  });
  const problem = validateReviewPost(fields);

  // A field's rule is shown under it once the user has tried to post, and from
  // then on as they type: a form nobody has tried to post does not fill with
  // demands as it is being filled in.
  const problemFor = (field: ReviewPostField) =>
    problem?.field === field && submitAttempted
      ? tForm(problem.messageKey)
      : undefined;

  const nameRef = useRef<InputRef>(null);
  const emailRef = useRef<InputRef>(null);
  const contentRef = useRef<TextAreaRef>(null);
  const focusField = (field: Exclude<ReviewPostField, 'modVersion'>) => {
    switch (field) {
      case 'authorName':
        nameRef.current?.focus();
        break;
      case 'authorEmail':
        emailRef.current?.focus();
        break;
      case 'content':
        contentRef.current?.focus();
        break;
    }
  };

  // Opened, the form scrolls itself into view, whole where it fits: what opened
  // it is not always beside it.
  const formRef = useRef<HTMLFormElement>(null);
  useEffect(() => {
    formRef.current?.scrollIntoView({ block: 'nearest' });
  }, []);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitAttempted(true);
    if (problem) {
      // The version is not the user's to fix, so its rule reads as the post
      // failing rather than as a field to correct.
      if (problem.field === 'modVersion') {
        setFailure(tForm(problem.messageKey));
      } else {
        setFailure(null);
        focusField(problem.field);
      }
      return;
    }

    setSubmitting(true);
    setFailure(null);
    try {
      await postModReview(fields);
      writeReviewAuthor({ name: fields.authorName, email: fields.authorEmail });
      setPosted(true);
    } catch (error) {
      setFailure(
        error instanceof ReviewPostError ? error.message : tForm('failed')
      );
    } finally {
      setSubmitting(false);
    }
  };

  if (posted) {
    return (
      <PostedLine data-testid="mod-review-posted">
        {tForm('posted')}
      </PostedLine>
    );
  }

  const nameId = `${idBase}-name`;
  const emailId = `${idBase}-email`;
  const contentId = `${idBase}-content`;

  return (
    <FormBlock
      ref={formRef}
      data-testid="mod-review-form"
      noValidate
      onSubmit={handleSubmit}
    >
      {kind === 'review' && (
        <Note data-testid="mod-review-form-purpose">
          <Trans
            t={t}
            i18nKey="mod.reviews.form.review.purpose"
            components={[<a href={MOD_ISSUES_URL}>GitHub</a>]}
          />
        </Note>
      )}
      <FieldRow>
        <Field id={nameId} label={tForm('name')} error={problemFor('authorName')}>
          <InputWithContextMenu
            ref={nameRef}
            id={nameId}
            value={name}
            maxLength={NAME_MAX}
            autoFocus={!author}
            onChange={(event) => setName(event.target.value)}
          />
        </Field>
        <Field
          id={emailId}
          label={tForm('email')}
          hint={tForm('emailHint')}
          error={problemFor('authorEmail')}
        >
          <InputWithContextMenu
            ref={emailRef}
            id={emailId}
            type="email"
            value={email}
            maxLength={EMAIL_MAX}
            onChange={(event) => setEmail(event.target.value)}
          />
        </Field>
      </FieldRow>
      <Field
        id={contentId}
        label={tForm('content')}
        hint={tForm('contentHint')}
        error={problemFor('content')}
        aside={`${[...content].length} / ${CONTENT_MAX}`}
      >
        <TextAreaWithContextMenu
          ref={contentRef}
          id={contentId}
          value={content}
          rows={4}
          maxLength={CONTENT_MAX}
          autoFocus={!!author}
          onChange={(event) => setContent(event.target.value)}
        />
      </Field>
      {failure && (
        <Problem role="alert" data-testid="mod-review-form-failure">
          {failure}
        </Problem>
      )}
      <FormActions>
        <Button
          type="primary"
          size="small"
          htmlType="submit"
          loading={submitting}
          data-testid="mod-review-form-submit"
        >
          {tForm('submit')}
        </Button>
        <Button
          size="small"
          disabled={submitting}
          data-testid="mod-review-form-cancel"
          onClick={onCancel}
        >
          {t('general.actions.cancel')}
        </Button>
        {modVersion && (
          <VersionNote>{tForm('version', { version: modVersion })}</VersionNote>
        )}
      </FormActions>
    </FormBlock>
  );
}

export default ModReviewForm;
