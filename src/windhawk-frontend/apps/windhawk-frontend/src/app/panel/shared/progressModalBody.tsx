import { Progress, Result } from 'antd';
import type { ReactNode } from 'react';
import styled from 'styled-components';

export const ModalContent = styled.div`
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 16px 0;
`;

const StatusMessage = styled.div`
  text-align: center;
  font-size: 16px;
`;

const Note = styled.div`
  text-align: center;
  color: var(--whui-text-muted);
  font-size: 14px;
`;

type ProgressModalBodyProps = {
  // A download/launch failure: render the error Result instead of the progress body.
  failed: boolean;
  failedTitle: ReactNode;
  errorMessage: ReactNode;
  // The current step label ("Downloading..." / "Installing..."), empty when neither.
  statusMessage: ReactNode;
  showProgress: boolean;
  downloadProgress: number;
  // Shown under the bar once the installer is running.
  note?: ReactNode;
};

// The download -> install -> failure body shared by the app-update and
// dev-tools-install modals: a progress bar while downloading, a note while the
// installer runs, and an error Result on failure. Callers wrap it in ModalContent and
// own any state that is specific to them (e.g. the dev-tools "prompt" step).
export function ProgressModalBody({
  failed,
  failedTitle,
  errorMessage,
  statusMessage,
  showProgress,
  downloadProgress,
  note,
}: ProgressModalBodyProps) {
  if (failed) {
    return (
      <Result status="error" title={failedTitle} subTitle={errorMessage} />
    );
  }

  return (
    <>
      <StatusMessage>{statusMessage}</StatusMessage>
      {showProgress && <Progress percent={downloadProgress} status="active" />}
      {note && <Note>{note}</Note>}
    </>
  );
}
