import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

const Footer = styled.footer`
  padding: 10px 0;
  text-align: center;
  color: rgba(255, 255, 255, 0.45);
`;

function AppFooter() {
  const { t } = useTranslation();

  return (
    <Footer>
      {t('website.footer.copyright')} &copy; <a href="https://ramensoftware.com/">Ramen Software</a>
    </Footer>
  );
}

export default AppFooter;
