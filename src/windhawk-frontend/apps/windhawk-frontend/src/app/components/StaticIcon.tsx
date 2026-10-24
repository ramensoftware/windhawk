import { type AbstractElement, icon, type IconDefinition } from '@fortawesome/fontawesome-svg-core';
import { createElement, type ReactElement } from 'react';

// Each icon's <svg>, made the first time the icon is drawn.
const elements = new WeakMap<IconDefinition, ReactElement>();

/**
 * A Font Awesome icon, drawn as the very <svg> FontAwesomeIcon draws for it when
 * given nothing but the icon. That one builds the element over again for every
 * icon on the page, every time it renders; this one builds it once an icon.
 */
export function StaticIcon({ icon: definition }: { icon: IconDefinition }) {
  let element = elements.get(definition);
  if (!element) {
    element = toReactElement(icon(definition).abstract[0]);
    elements.set(definition, element);
  }
  return element;
}

// The attribute names React takes in place of the DOM's: `className`, and the
// rest camel-cased, bar the `aria-` and `data-` ones React takes as they are.
function propName(attribute: string): string {
  if (attribute === 'class') {
    return 'className';
  }
  if (attribute.startsWith('aria-') || attribute.startsWith('data-')) {
    return attribute;
  }
  return attribute.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase());
}

function toReactElement(node: AbstractElement): ReactElement {
  const props: Record<string, unknown> = {};
  for (const [attribute, value] of Object.entries(node.attributes ?? {})) {
    props[propName(attribute)] = value;
  }
  return createElement(node.tag, props, ...(node.children ?? []).map(toReactElement));
}
