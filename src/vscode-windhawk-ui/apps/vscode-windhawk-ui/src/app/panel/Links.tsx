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

  h1,
  h2,
  h3,
  h4,
  h5,
  h6 {
    margin-bottom: 0;
  }
`;

const CenteredContentLowerPadding = styled.div`
  // Without this the centered content looks too low.
  height: 10%;
`;

function Links() {
  const { t } = useTranslation();

  useEffect(() => {
    document.title = `${t('website.appHeader.links')} - Windhawk`;
  }, [t]);

  return (
    <AboutContainer>
      <AboutContent>
        <ContentSection>
          <h1>{t('website.links.news')}</h1>
          <div>
            <div>
              <a href="https://ramensoftware.com/tag/windhawk">
                {t('website.links.windhawkNews')}
              </a>
            </div>
            <div>
              <a href="https://ramensoftware.com/downloads/windhawk_setup.exe?changelog">
                {t('website.links.windhawkChangelog')}
              </a>
            </div>
          </div>
        </ContentSection>
        <ContentSection>
          <h1>{t('website.links.community')}</h1>
          <div>
            <div>
              <a href="https://discord.com/servers/windhawk-923944342991818753">{t('website.links.discord')}</a>
            </div>
            <div>
              <a href="https://github.com/ramensoftware/windhawk/discussions">
                {t('website.links.windhawkDiscussions')}
              </a>
            </div>
            <div>
              <a href="https://github.com/ramensoftware/windhawk-mods/discussions">
                {t('website.links.modsDiscussions')}
              </a>
            </div>
          </div>
        </ContentSection>
        <ContentSection>
          <h1>{t('website.links.documentation')}</h1>
          <div>
            <div>
              <a href="https://github.com/ramensoftware/windhawk/wiki/creating-a-new-mod">
                {t('website.links.creatingNewMod')}
              </a>
            </div>
            <div>
              <a href="https://github.com/ramensoftware/windhawk/wiki">
                {t('website.links.otherTopics')}
              </a>
            </div>
          </div>
        </ContentSection>
        <ContentSection>
          <h1>{t('website.links.feedback')}</h1>
          <div>
            <div>
              <a href="https://github.com/ramensoftware/windhawk/issues">
                {t('website.links.reportIssue')}
              </a>
            </div>
            <div>
              <a href="https://ramensoftware.com/contact">
                {t('website.links.contactRamenSoftware')}
              </a>
            </div>
          </div>
        </ContentSection>
      </AboutContent>
      <CenteredContentLowerPadding />
    </AboutContainer>
  );
}

export default Links;
