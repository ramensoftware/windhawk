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
  left: 0;
  right: 0;
  margin: 0 auto;
  width: 100%;
  max-width: var(--app-max-width);
`;

const CreateButton = styled(Button)`
  position: absolute;
  right: 20px;
  bottom: 20px;
  background-color: var(--app-background-color) !important;
  box-shadow: 0 3px 6px rgb(100 100 100 / 16%), 0 1px 2px rgb(100 100 100 / 23%);
`;

const CreateButtonIcon = styled(FontAwesomeIcon)`
  margin-right: 8px;
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
