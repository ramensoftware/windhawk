import { InputWithContextMenu } from '@app/components/InputWithContextMenu';
import useModalClose from '@app/panel/shared/useModalClose';
import { testIdProps } from '@app/utils';
import { Button, Modal, Typography } from 'antd';
import { type InputRef } from 'antd/lib/input';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { groupNameTaken, type ModGroup } from './modGroups';

// Where the reason a name is refused is said. Standing whether or not there is
// one, so the dialog is the same height either way.
const NameError = styled.div`
  min-height: 22px;
  margin-top: 8px;
`;

interface Props {
  group: ModGroup;
  // Every group, for the names this one may not take.
  groups: ModGroup[];
  onRename: (name: string) => void;
  // Reported once this dialog is off the screen, whether it was dismissed or
  // handed over, so it animates out either way.
  onClose: () => void;
}

/** One field and an OK: what a group is called. */
export function ModGroupRenameModal({
  group,
  groups,
  onRename,
  onClose,
}: Props) {
  const { t } = useTranslation();

  const { open, close, afterClose } = useModalClose(onClose);
  const [name, setName] = useState(group.name);
  const nameRef = useRef<InputRef>(null);

  // The dialog is one field, so the field is where it opens, with the name it
  // already holds selected: a rename is usually a replacement, and the one that
  // is not still has the caret in the text. Told rather than left to autoFocus,
  // which the dialog puts focus over on the way in.
  useEffect(() => {
    nameRef.current?.focus({ cursor: 'all' });
  }, []);

  const trimmedName = name.trim();
  const nameTaken =
    trimmedName !== '' && groupNameTaken(groups, trimmedName, group.id);
  const canRename = trimmedName !== '' && !nameTaken;

  const handleRename = () => {
    if (!canRename) {
      return;
    }

    close();
    onRename(trimmedName);
  };

  return (
    <Modal
      open={open}
      afterClose={afterClose}
      title={t('modGroups.renameTitle')}
      onCancel={close}
      maskClosable={false}
      wrapProps={testIdProps('mod-group-rename-modal')}
      footer={[
        <Button key="cancel" data-testid="mod-group-rename-cancel" onClick={close}>
          {t('general.actions.cancel')}
        </Button>,
        <Button
          key="rename"
          type="primary"
          disabled={!canRename}
          data-testid="mod-group-rename-ok"
          onClick={handleRename}
        >
          {t('modGroups.renameOk')}
        </Button>,
      ]}
    >
      <InputWithContextMenu
        ref={nameRef}
        status={nameTaken ? 'error' : undefined}
        value={name}
        placeholder={t('modGroups.groupName') as string}
        aria-label={t('modGroups.groupName') as string}
        data-testid="mod-group-rename-name"
        onChange={(e) => setName(e.target.value)}
        onPressEnter={handleRename}
      />
      <NameError>
        {nameTaken && (
          <Typography.Text type="danger" data-testid="mod-group-name-taken">
            {t('modGroups.nameTaken')}
          </Typography.Text>
        )}
      </NameError>
    </Modal>
  );
}

export default ModGroupRenameModal;
