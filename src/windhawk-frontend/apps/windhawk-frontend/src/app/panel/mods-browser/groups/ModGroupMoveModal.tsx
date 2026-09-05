import { InputWithContextMenu } from '@app/components/InputWithContextMenu';
import useModalClose from '@app/panel/shared/useModalClose';
import { testIdProps } from '@app/utils';
import { Button, Modal, Radio, Space, Typography } from 'antd';
import { type InputRef } from 'antd/lib/input';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import {
  groupNameTaken,
  type ModGroup,
  type ModGroupDestination,
} from './modGroups';

// The New group choice and the name it asks for, on one line. The gap between
// them is the radio's own margin.
const NewGroupChoice = styled.div`
  display: flex;
  align-items: center;
`;

// Room for a group name and no more. Run to the dialog's width the field would
// read as what the dialog is about, where it is one detail of one choice among
// four.
const NewGroupName = styled(InputWithContextMenu)`
  width: 200px;
`;

// Where the reason a name is refused is said. Standing whether or not there is
// one, so the dialog is the same height either way.
const NameError = styled.div`
  min-height: 22px;
  margin-top: 8px;
`;

// What the radio holds, against the destination each choice reports. A group's
// own id cannot collide with either, since every generated id carries a prefix.
const NONE_CHOICE = 'none';
const NEW_CHOICE = 'new';

// The group every one of the mods is already in, if they share one. Otherwise
// nothing is preselected: there is no destination the move is obviously about,
// and picking one for the user would be picking one of several.
function commonGroupId(groups: ModGroup[], modIds: string[]): string | null {
  if (modIds.length === 0) {
    return null;
  }

  const members = new Set(modIds);
  const holding = groups.filter((group) =>
    group.modIds.some((modId) => members.has(modId))
  );

  return holding.length === 1 &&
    modIds.every((modId) => holding[0].modIds.includes(modId))
    ? holding[0].id
    : null;
}

interface Props {
  // The mods being moved, for the title's count and the initial destination.
  modIds: string[];
  groups: ModGroup[];
  onMove: (destination: ModGroupDestination) => void;
  // Reported once this dialog is off the screen, whether it was dismissed or
  // handed over, so it animates out either way.
  onClose: () => void;
}

/** Where a selection is sent: an existing group, a new one, or none. */
export function ModGroupMoveModal({ modIds, groups, onMove, onClose }: Props) {
  const { t } = useTranslation();

  const { open, close, afterClose } = useModalClose(onClose);
  const [choice, setChoice] = useState<string | null>(() =>
    commonGroupId(groups, modIds)
  );
  const [newName, setNewName] = useState('');
  const newNameRef = useRef<InputRef>(null);

  // The field stands whether or not it is the choice in force, so the line does
  // not change height as the choice moves onto it - which means it cannot be
  // focused by arriving. Choosing to make a group is asking to name one, so the
  // caret goes there as the choice is made.
  useEffect(() => {
    if (choice === NEW_CHOICE) {
      newNameRef.current?.focus();
    }
  }, [choice]);

  const trimmedNewName = newName.trim();
  const newNameTaken =
    trimmedNewName !== '' && groupNameTaken(groups, trimmedNewName);
  const canMove =
    choice !== null &&
    (choice !== NEW_CHOICE || (trimmedNewName !== '' && !newNameTaken));

  const handleMove = () => {
    if (!canMove) {
      return;
    }

    close();
    onMove(
      choice === NONE_CHOICE
        ? { type: 'none' }
        : choice === NEW_CHOICE
          ? { type: 'new', name: trimmedNewName }
          : { type: 'existing', groupId: choice }
    );
  };

  return (
    <Modal
      open={open}
      afterClose={afterClose}
      title={t('modGroups.moveTitle', { count: modIds.length })}
      onCancel={close}
      maskClosable={false}
      wrapProps={testIdProps('mod-group-move-modal')}
      footer={[
        <Button key="cancel" data-testid="mod-group-move-cancel" onClick={close}>
          {t('general.actions.cancel')}
        </Button>,
        <Button
          key="move"
          type="primary"
          disabled={!canMove}
          data-testid="mod-group-move-ok"
          onClick={handleMove}
        >
          {t('modGroups.moveOk')}
        </Button>,
      ]}
    >
      <Radio.Group value={choice} onChange={(e) => setChoice(e.target.value)}>
        <Space direction="vertical">
          <Radio value={NONE_CHOICE} data-testid="mod-group-move-none">
            {t('modGroups.noGroup')}
          </Radio>
          {groups.map((group) => (
            <Radio key={group.id} value={group.id} data-testid="mod-group-move-existing">
              {group.name}
            </Radio>
          ))}
          <NewGroupChoice>
            <Radio value={NEW_CHOICE} data-testid="mod-group-move-new">
              {t('modGroups.newGroup')}
            </Radio>
            <NewGroupName
              ref={newNameRef}
              disabled={choice !== NEW_CHOICE}
              status={newNameTaken ? 'error' : undefined}
              value={newName}
              placeholder={t('modGroups.groupName') as string}
              aria-label={t('modGroups.groupName') as string}
              data-testid="mod-group-move-new-name"
              onChange={(e) => setNewName(e.target.value)}
              onPressEnter={handleMove}
            />
          </NewGroupChoice>
        </Space>
      </Radio.Group>
      <NameError>
        {newNameTaken && (
          <Typography.Text type="danger" data-testid="mod-group-name-taken">
            {t('modGroups.nameTaken')}
          </Typography.Text>
        )}
      </NameError>
    </Modal>
  );
}

export default ModGroupMoveModal;
