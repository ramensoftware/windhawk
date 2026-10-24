import { Spin, type SpinProps } from 'antd';

const styles: SpinProps['styles'] = {
  // antd lays the spinner and its description out as an inline flex column
  // centered within itself. Block-level, it centers across the full width.
  root: { display: 'flex' },
  description: { fontSize: 32 },
};

// A loading spinner centered in the width it is given, with its description in
// large text.
export function ProgressSpin(props: SpinProps) {
  return <Spin {...props} styles={styles} />;
}
