import useModalClose from '@app/panel/shared/useModalClose';
import { testIdProps } from '@app/utils';
import { Button, Modal, Result, Spin } from 'antd';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

import type { UpdatableMod } from './ModUpdateList';
import type { ModUpdateStatus } from './updateRun';
import VersionChange from './VersionChange';

const ProgressSpin = styled(Spin)`
  display: block;
  margin-inline-start: auto;
  margin-inline-end: auto;
  font-size: 32px;
  padding: 24px 0;
`;

interface Props {
  mod: UpdatableMod;
  // The version the update moves the mod to. In hand whenever this is up, the run
  // being started from the source that named it.
  version?: string;
  // What became of the mod, or null while its install is still going.
  status: ModUpdateStatus | null;
  cancelPending: boolean;
  onCancel: () => void;
  onClose: () => void;
}

/**
 * One mod's update, over the wizard's list: the spinner while it installs, and
 * what became of it once the host has answered.
 *
 * The list stays where it is behind this. A mod read up on and then updated is one
 * decision, and the next mod is that same decision again, so a single-mod run
 * reports here rather than taking the dialog through the phases a batch run puts
 * it through and ending on a summary that is the end of the sitting.
 */
export function ModUpdateRunModal({
  mod,
  version,
  status,
  cancelPending,
  onCancel,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const { open, close, afterClose } = useModalClose(onClose);

  const running = status === null;

  const footer = running
    ? [
        <Button
          key="cancel"
          danger
          disabled={cancelPending}
          data-testid="mod-update-run-cancel"
          onClick={onCancel}
        >
          {cancelPending
            ? t('general.status.canceling')
            : t('general.actions.cancel')}
        </Button>,
      ]
    : [
        <Button
          key="close"
          type="primary"
          data-testid="mod-update-run-close"
          onClick={close}
        >
          {t('general.actions.close')}
        </Button>,
      ];

  return (
    <Modal
      open={open}
      afterClose={afterClose}
      title={mod.name}
      width={480}
      centered
      // The install rewrites the mod the list behind is showing and this is the
      // only account of how it went, so neither is clicked away.
      onCancel={running ? undefined : close}
      closable={!running}
      maskClosable={false}
      // The state on the dialog as well as in it, so a test reads what the run
      // came to without matching the sentence it is reported in.
      wrapProps={{
        ...testIdProps('mod-update-run-modal'),
        'data-status': status ?? 'running',
      }}
      footer={footer}
    >
      {running ? (
        <ProgressSpin size="large" tip={t('general.status.updating')} />
      ) : status === 'updated' ? (
        <Result
          status="success"
          title={t('modUpdates.doneTitle')}
          // The move it made, which is what the user weighed the update on.
          subTitle={
            mod.installedVersion && version ? (
              <VersionChange from={mod.installedVersion} to={version} />
            ) : undefined
          }
        />
      ) : status === 'failed' ? (
        // Null details is all a failed install carries - neither host attaches a
        // reason, having sent it to the compiler output window - so the title is
        // the whole report.
        <Result status="error" title={t('modUpdates.failedTitle')} />
      ) : (
        <Result status="warning" title={t('modUpdates.abortedTitle')} />
      )}
    </Modal>
  );
}

export default ModUpdateRunModal;
