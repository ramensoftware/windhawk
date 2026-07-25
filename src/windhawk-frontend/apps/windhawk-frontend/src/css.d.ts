// Side-effect CSS imports are resolved by webpack loaders at build time. Declare
// them so TypeScript can resolve the bare imports (e.g. import './App.css').
declare module '*.css';
