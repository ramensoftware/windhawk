import { RowMeta, RowMetaTitle } from '@app/components/RowList';
import styled from 'styled-components';

// One setting of a form: its name and an account of it, over the control that
// sets it.
export const SettingRow = styled.div`
  padding: 12px 0;
`;

// The name and the account read as one block, set apart from the control by
// the gap under them.
export const SettingRowMeta = styled(RowMeta)`
  margin-bottom: 8px;

  ${RowMetaTitle} {
    font-size: 16px;
    line-height: 1.5;
  }
`;
