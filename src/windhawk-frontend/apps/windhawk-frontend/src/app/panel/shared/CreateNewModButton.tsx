import { faPen } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button } from 'antd';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { createNewMod } from '@app/webviewIPC';
import DevModeAction from './DevModeAction';

const ButtonContainer = styled.div`
  position: fixed;
  bottom: 0;
  inset-inline-start: 0;
  inset-inline-end: 0;
  margin: 0 auto;
  width: 100%;
  max-width: var(--whui-max-width);
  z-index: 100; /* Monaco editor uses two-digit z-index values */

  /* Stay out of the way while the settings tab is expanded to fullscreen. */
  body.windhawk-settings-fullscreen & {
    display: none;
  }
`;

const CreateButton = styled(Button)`
  position: absolute !important;
  inset-inline-end: 32px;
  bottom: 20px;
  background-color: var(--whui-background-color) !important;
  box-shadow: 0 3px 6px rgb(100 100 100 / 16%), 0 1px 2px rgb(100 100 100 / 23%);
`;

const CreateButtonIcon = styled(FontAwesomeIcon)`
  /* The spinner that stands in for this icon while the editor opens is 1em wide,
     and Font Awesome pads its icons out to 1.25em; matching them keeps the button
     the width it had, instead of moving its label as the two swap. */
  --fa-width: 1em;
  margin-inline-end: 8px;
`;

function CreateNewModButton() {
  const { t } = useTranslation();

  return (
    <ButtonContainer>
      <DevModeAction
        popconfirmPlacement="top"
        onClick={() => createNewMod()}
        renderButton={({ onClick, loading }) => (
          <CreateButton
            shape="round"
            data-testid="create-new-mod"
            onClick={onClick}
            loading={loading}
            // The icon goes in the slot antd swaps for its spinner rather than
            // among the children: given a slot to swap, antd puts the spinner in
            // place at once, where an icon it does not know about leaves it
            // animating one in from no width at all.
            icon={<CreateButtonIcon icon={faPen} />}
          >
            {t('createNewModButton.title')}
          </CreateButton>
        )}
      />
    </ButtonContainer>
  );
}

export default CreateNewModButton;
