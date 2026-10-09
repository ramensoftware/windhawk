import LinkifiedText from '@app/components/LinkifiedText';
import { useTranslation } from 'react-i18next';

// Who took a mod off and why, for a tooltip: the catalog, and the reason it
// gives, which is plain text with its web addresses made links.
function NukedReason({ reason }: { reason: string }) {
  const { t } = useTranslation();

  return (
    <>
      <div>{t('mod.nuked.title')}</div>
      <div>
        <LinkifiedText text={reason} />
      </div>
    </>
  );
}

export default NukedReason;
