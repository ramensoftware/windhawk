import { Button, Modal, Spin } from 'antd';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { type ModOperationContext } from './useCancelModOperation';

const ProgressSpin = styled(Spin)`
  display: block;
  margin-inline-start: auto;
  margin-inline-end: auto;
  font-size: 32px;
`;

interface Props {
  // Which of the two operations is running, and for an install whether it is a
  // first install or an update - which only its context says.
  installModPending?: boolean;
  installModContext?: ModOperationContext;
  compileModPending?: boolean;
  // Omitted where there is nothing to cancel against (the website build, which
  // starts no operation of its own); the modal then has no footer. It answers
  // whether the cancel was taken up, and a cancel that was not leaves the button
  // live: the operation it named is still running, so there is still something to
  // ask for.
  onCancel?: () => Promise<boolean>;
}

/**
 * The modal both mods browsers put up while a mod installs, updates or compiles.
 * It is not closable and offers nothing but the cancel: the operation rewrites the
 * mod being shown, so the screen waits for it either way.
 *
 * Keep it mounted across operations: it is up for as long as one of the two is
 * pending, and it takes itself down when neither is, which is what lets antd
 * animate it out. The requested-cancel flag is put back on the way in, so an
 * operation never arrives asked about.
 *
 * The button goes down on the click, so one operation is asked about once, and
 * comes back up if that ask is answered as not taken up - the operation is then
 * still running with nobody signaled, which leaves something to ask for.
 */
export function ModOperationModal({
  installModPending,
  installModContext,
  compileModPending,
  onCancel,
}: Props) {
  const { t } = useTranslation();
  const [cancelRequested, setCancelRequested] = useState(false);

  const open = !!(installModPending || compileModPending);

  // Held rather than derived, because the operation the label names is over by the
  // time the modal animates out: reading the pending flags there would blank it
  // mid-fade. It is refreshed only while up, so the last operation's word is what
  // fades with it.
  const [tip, setTip] = useState('');
  const pendingTip = installModPending
    ? installModContext?.updating
      ? t('general.status.updating')
      : t('general.status.installing')
    : compileModPending
      ? t('general.status.compiling')
      : null;
  if (pendingTip !== null && pendingTip !== tip) {
    setTip(pendingTip);
  }

  const [wasOpen, setWasOpen] = useState(false);
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) {
      setCancelRequested(false);
    }
  }

  const requestCancel = async () => {
    if (!onCancel) {
      return;
    }

    setCancelRequested(true);
    if (!(await onCancel())) {
      setCancelRequested(false);
    }
  };

  const footer = onCancel
    ? [
        <Button
          key="cancel"
          danger
          disabled={cancelRequested}
          data-testid="mod-operation-cancel"
          onClick={requestCancel}
        >
          {cancelRequested
            ? t('general.status.canceling')
            : t('general.actions.cancel')}
        </Button>,
      ]
    : null;

  return (
    // Nothing of the operation is left behind the closed modal: the body goes with
    // the animation that takes it off the screen, rather than staying in the
    // document as a hidden spinner naming an operation that has ended.
    <Modal open={open} destroyOnClose closable={false} footer={footer}>
      {/* The tip keeps naming the operation after a cancel is asked for, because
          that is what is still running; the button carries the cancel's state. */}
      <ProgressSpin size="large" tip={tip} />
    </Modal>
  );
}

export default ModOperationModal;
