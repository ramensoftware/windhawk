import { Alert, Button } from 'antd';
import { useContext, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { AppUISettingsContext } from '../appUISettings';
import { ChangelogModal } from './ChangelogModal';
import { UpdateModal } from './UpdateModal';

const AboutContainer = styled.div`
  display: flex;
  flex-direction: column;
  height: 100%;

  // Without this the centered content looks too low.
  padding-bottom: 10vh;
`;

const AboutContent = styled.div`
  margin: auto;
  text-align: center;
`;

const ContentSection = styled.div`
  margin-bottom: 1.5em;

  h1,
  h2,
  h3,
  h4,
  h5,
  h6 {
    margin-bottom: 0;
  }
`;

const UpdateNoticeDescription = styled.div`
  display: flex;
  flex-direction: column;
  row-gap: 8px;
`;

const ButtonGroup = styled.div`
  display: flex;
  gap: 8px;
  justify-content: center;
`;

function About() {
  const { t } = useTranslation();
  const [changelogModalOpen, setChangelogModalOpen] = useState(false);
  const [updateModalOpen, setUpdateModalOpen] = useState(false);

  const { updateIsAvailable } = useContext(AppUISettingsContext);

  const currentVersion = (
    process.env['REACT_APP_VERSION'] || 'unknown'
  ).replace(/^(\d+(?:\.\d+)+?)(\.0+)+$/, '$1');

  return (
    <AboutContainer>
      <AboutContent>
        <ContentSection>
          <h1>
            {t('about.title', {
              // version: currentVersion + ' ' + t('about.beta'),
              version: currentVersion,
            })}
          </h1>
          <h3>{t('about.subtitle')}</h3>
          <h3>
            <Trans
              t={t}
              i18nKey="about.credit"
              values={{ author: 'Ramen Software' }}
              components={[<a href="https://ramensoftware.com/">website</a>]}
            />
          </h3>
        </ContentSection>
        {updateIsAvailable && (
          <ContentSection>
            <Alert
              message={<h3>{t('about.update.title')}</h3>}
              description={
                <UpdateNoticeDescription>
                  <div>{t('about.update.subtitle')}</div>
                  <ButtonGroup>
                    <Button onClick={() => setChangelogModalOpen(true)}>
                      {t('about.update.changelogButton')}
                    </Button>
                    <Button
                      type="primary"
                      onClick={() => setUpdateModalOpen(true)}
                    >
                      {t('about.update.updateButton')}
                    </Button>
                  </ButtonGroup>
                </UpdateNoticeDescription>
              }
              type="info"
            />
          </ContentSection>
        )}
        <ContentSection>
          <h1>{t('about.links.title')}</h1>
          <div>
            <div>
              <a href="https://windhawk.net/">{t('about.links.homepage')}</a>
            </div>
            <div>
              <a href="https://github.com/ramensoftware/windhawk/wiki">
                {t('about.links.documentation')}
              </a>
            </div>
          </div>
        </ContentSection>
        <ContentSection>
          <h1>{t('about.builtWith.title')}</h1>
          <div>
            <div>
              <a href="https://github.com/VSCodium/vscodium">VSCodium</a>
              {' - '}
              {t('about.builtWith.vscodium')}
            </div>
            <div>
              <a href="https://github.com/mstorsjo/llvm-mingw">LLVM MinGW</a>
              {' - '}
              {t('about.builtWith.llvmMingw')}
            </div>
            <div>
              <a href="https://github.com/m417z/minhook-detours">MinHook-Detours</a>
              {' - '}
              {t('about.builtWith.minHook')}
            </div>
            <div>{t('about.builtWith.others')}</div>
          </div>
        </ContentSection>
      </AboutContent>
      <ChangelogModal
        open={changelogModalOpen}
        onClose={() => setChangelogModalOpen(false)}
      />
      <UpdateModal
        open={updateModalOpen}
        onClose={() => setUpdateModalOpen(false)}
      />
    </AboutContainer>
  );
}

export default About;
