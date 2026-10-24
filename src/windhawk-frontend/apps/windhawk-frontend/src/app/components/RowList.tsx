import { Listy, type ListyProps } from 'antd';
import { Children, type ReactElement, type ReactNode } from 'react';
import styled from 'styled-components';

const DIVIDED_CLASS = 'windhawk-row-list-divided';

// Doubled to outrank antd's .ant-listy .ant-listy-item, its hover included.
const StyledListy = styled(Listy)`
  && > .ant-listy-item,
  && > .ant-listy-item:hover {
    padding: 0;
    background-color: transparent;
  }

  &&:not(.${DIVIDED_CLASS}) > .ant-listy-item,
  && > .ant-listy-item:last-child {
    border-bottom: none;
  }

  &&.${DIVIDED_CLASS} > .ant-listy-item {
    padding: 8px 16px;
  }
`;

export interface RowListProps<T> extends ListyProps<T> {
  // Rows set apart by antd's rule and padded the way a small list pads them,
  // for entries read down one after another.
  divided?: boolean;
}

/**
 * antd's Listy as a column of rows that lay themselves out: a row is drawn in
 * no padding, rule or hover of the list's, unless the list is `divided`.
 */
export function RowList<T>({ divided = false, className, ...props }: RowListProps<T>) {
  return (
    <StyledListy
      {...(props as ListyProps<unknown>)}
      className={[divided && DIVIDED_CLASS, className].filter(Boolean).join(' ')}
    />
  );
}

/**
 * A RowList of the rows written out as its children. A child left out by a
 * condition is no row at all.
 */
export function RowStack({ children, className }: { children: ReactNode; className?: string }) {
  const rows = Children.toArray(children) as ReactElement[];
  return (
    <RowList
      className={className}
      items={rows}
      rowKey={(row) => row.key ?? ''}
      itemRender={(row) => row}
    />
  );
}

export const RowMetaTitle = styled.h4`
  margin: 0;
  color: var(--whui-text);
  font-size: inherit;
  font-weight: 500;
  line-height: inherit;
`;

export const RowMetaDescription = styled.div`
  color: var(--whui-text-muted);
`;

/**
 * What a row says of itself ahead of anything else it holds: its name, and an
 * account of it. The class names are what a list sets the parts by from outside
 * and what a test finds them by.
 */
export function RowMeta({
  title,
  description,
  className,
}: {
  title?: ReactNode;
  description?: ReactNode;
  className?: string;
}) {
  return (
    <div className={['windhawk-row-meta', className].filter(Boolean).join(' ')}>
      {title && <RowMetaTitle className="windhawk-row-meta-title">{title}</RowMetaTitle>}
      {description && (
        <RowMetaDescription className="windhawk-row-meta-description">
          {description}
        </RowMetaDescription>
      )}
    </div>
  );
}

// A row of a divided list: its meta, and whatever stands at the row's end.
export const DividedRow = styled.div`
  display: flex;
  align-items: center;
  justify-content: space-between;
`;

export const DividedRowMeta = styled(RowMeta)`
  flex: 1;
  min-width: 0;

  ${RowMetaTitle} {
    margin-bottom: 4px;
  }
`;
