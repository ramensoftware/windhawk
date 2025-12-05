import { faPen } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button } from 'antd';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { createNewMod } from '../webviewIPC';
import DevModeAction from './DevModeAction';

const ButtonContainer = styled.div`
  position: fixed;
  bottom: 0;
  inset-inline-start: 0;
  inset-inline-end: 0;
  margin: 0 auto;
  width: 100%;
  max-width: var(--app-max-width);
  z-index: 100; /* Monaco editor uses two-digit z-index values */
`;

const CreateButton = styled(Button)`
  position: absolute;
  inset-inline-end: 32px;
  bottom: 20px;
  background-color: var(--app-background-color) !important;
  box-shadow: 0 3px 6px rgb(100 100 100 / 16%), 0 1px 2px rgb(100 100 100 / 23%);
`;

const CreateButtonIcon = styled(FontAwesomeIcon)`
  margin-inline-end: 8px;
`;

function CreateNewModButton() {
  const { t } = useTranslation();

  return (
    <ButtonContainer>
      <DevModeAction
        popconfirmPlacement="top"
        onClick={() => createNewMod()}
        renderButton={(onClick) => (
          <CreateButton shape="round" onClick={onClick}>
            <CreateButtonIcon icon={faPen} /> {t('createNewModButton.title')}
          </CreateButton>
        )}
      />
    </ButtonContainer>
  );
}

export default CreateNewModButton;
