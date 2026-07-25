import { Button } from 'antd';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

const AboutContainer = styled.div`
  display: flex;
  flex-direction: column;
  flex: 1;
`;

const AboutContent = styled.div`
  margin: auto;
  text-align: center;
`;

const ContentSection = styled.div`
  margin-bottom: 1.5em;

  h1, h2, h3, h4, h5, h6 {
    margin-bottom: 0;
  }
`;

const CenteredContentLowerPadding = styled.div`
  // Without this the centered content looks too low.
  height: 10%;
`;

function Download() {
  const { t } = useTranslation();

  useEffect(() => {
    document.title = `${t('general.actions.download')} - Windhawk`;
  }, [t]);

  return (
    <AboutContainer>
      <AboutContent>
        <ContentSection>
          <h1>{t('website.download.title')}</h1>
          <div>{t('website.download.newVersionAvailable')}</div>
          <div><a href="https://ramensoftware.com/downloads/windhawk_setup.exe?changelog">{t('website.download.whatsNew')}</a></div>
        </ContentSection>
        <ContentSection>
          <Button
            type='primary'
            size='large'
            href='https://ramensoftware.com/downloads/windhawk_setup.exe'
          >
            {t('general.actions.download')}
          </Button>
        </ContentSection>
      </AboutContent>
      <CenteredContentLowerPadding />
    </AboutContainer>
  );
}

export default Download;
