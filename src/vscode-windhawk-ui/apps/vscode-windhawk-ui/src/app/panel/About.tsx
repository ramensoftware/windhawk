import { Alert, Button } from 'antd';
import { useContext, useMemo } from 'react';
import styled from 'styled-components';
import { AppUISettingsContext } from '../appUISettings';
import { Trans, useTranslation } from 'react-i18next';

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

function About() {
  const { t } = useTranslation();

  const { updateIsAvailable } = useContext(AppUISettingsContext);

  const currentVersion = (
    process.env['REACT_APP_VERSION'] || 'unknown'
  ).replace(/^(\d+(?:\.\d+)+?)(\.0+)+$/, '$1');

  const updateUrl = useMemo(() => {
    // https://stackoverflow.com/a/52171480
    const tinySimpleHash = (s: string) => {
      let h = 9;
      for (let i = 0; i < s.length;) {
        h = Math.imul(h ^ s.charCodeAt(i++), 9 ** 9);
      }
      return h ^ h >>> 9;
    };

    // Hash the current version, showing it in plain text can be confusing as
    // users might interpret it as the new version.
    return (
      'https://windhawk.net/download?q=' +
      (tinySimpleHash(currentVersion) + 0x80000000).toString(36)
    );
  }, [currentVersion]);

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
                  <Button
                    type="primary"
                    href={updateUrl}
                  >
                    {t('about.update.button')}
                  </Button>
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
              <a href="https://github.com/TsudaKageyu/minhook">MinHook</a>
              {' - '}
              {t('about.builtWith.minHook')}
            </div>
            <div>{t('about.builtWith.others')}</div>
          </div>
        </ContentSection>
      </AboutContent>
    </AboutContainer>
  );
}

export default About;
