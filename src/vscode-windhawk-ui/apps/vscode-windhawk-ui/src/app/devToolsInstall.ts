// Seam for the "install development tools" modal (mirrors feedback.ts's
// error-reporter seam). The IPC layer is a plain module, not a React component, so
// it reaches the modal - which owns React state - through this register/trigger pair:
// the modal registers its opener once mounted, and a launch entry point that replies
// `uiMissing` calls promptDevToolsInstall() to surface it.

type Prompt = () => void;

let prompt: Prompt | null = null;

export function registerDevToolsInstallPrompt(fn: Prompt | null) {
  prompt = fn;
}

// Open the modal, if one is mounted (a no-op before it mounts, e.g. website mode).
export function promptDevToolsInstall() {
  prompt?.();
}
