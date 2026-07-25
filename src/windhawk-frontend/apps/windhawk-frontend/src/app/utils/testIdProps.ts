/**
 * A `data-testid` attribute in the shape of a props object, for the places where
 * it cannot be written as a JSX attribute: props that a component forwards to a
 * DOM node but types as a component's own props (antd's `okButtonProps` and
 * friends), which reject the unknown attribute.
 *
 * Spread it into such an object - `{ danger: true, ...testIdProps('x') }` - so
 * the attribute rides along to the rendered element.
 */
export function testIdProps(id: string) {
  return { 'data-testid': id };
}
