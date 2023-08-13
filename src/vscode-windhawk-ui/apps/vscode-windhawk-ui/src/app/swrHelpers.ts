export const fetchText = (input: RequestInfo | URL, init?: RequestInit) =>
  fetch(input, init).then((res) => res.text());
