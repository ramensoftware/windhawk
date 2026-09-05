"use strict";
// The webview IPC contract: the postMessage protocol between the Windhawk React
// webview and its hosts (the VSCode extension and the Tauri native shell). Both
// TypeScript hosts import the message payloads from here, and the Rust host
// mirrors them with typed serde structs proven against the shared fixture corpus
// in ./fixtures. The envelope those payloads travel in is the exception - see the
// note above webviewIPCMessageType.
//
// The webview is the superset consumer (it sees every host), so this file models
// the union of all hosts' messages; a given host implements the subset it needs.
Object.defineProperty(exports, "__esModule", { value: true });
exports.ALL_VERSIONS = exports.WEBVIEW_IPC_CONTRACT_VERSION = void 0;
exports.parseSuppression = parseSuppression;
exports.formatSuppression = formatSuppression;
exports.suppressesUpdateOffer = suppressesUpdateOffer;
exports.isValidSuppression = isValidSuppression;
/**
 * The contract version, asserted on the getInitialAppSettings handshake so a host
 * shipped against a different contract fails loudly instead of mis-handling a
 * message. Kept in lockstep with contract-version.json (a package test asserts
 * equality; the Rust host reads that JSON to check its own constant).
 */
exports.WEBVIEW_IPC_CONTRACT_VERSION = '1.13.0';
/**
 * The 'suppress every offer' value of updatesDisabledForVersion, named so a
 * consumer building one does not spell the sentinel itself.
 */
exports.ALL_VERSIONS = '*';
/**
 * Decode a stored updatesDisabledForVersion. `null` is 'suppresses nothing',
 * which covers '', a bare '=' (a pin on the empty version, which no offer can
 * be), and every other value outside the grammar.
 */
function parseSuppression(stored) {
    if (stored === exports.ALL_VERSIONS) {
        return { kind: 'all' };
    }
    if (stored.startsWith('=') && stored.length > 1) {
        return { kind: 'pinned', version: stored.slice(1) };
    }
    return null;
}
/**
 * Encode a suppression as the value to store, the inverse of parseSuppression
 * (`parseSuppression(formatSuppression(s))` is `s` for every `s`). A writer
 * composes the union it already switches on rather than building a '=' prefix,
 * so the grammar has one implementation on the write side too; the result is
 * valid by construction, which is what isValidSuppression is left to check for
 * values that came from somewhere else. There is no encoding of 'updates are
 * on': that is the empty string, and a writer that means it says so.
 */
function formatSuppression(suppression) {
    switch (suppression.kind) {
        case 'all':
            return exports.ALL_VERSIONS;
        case 'pinned':
            return `=${suppression.version}`;
    }
}
/**
 * Whether a stored updatesDisabledForVersion suppresses an offer of `latest`.
 * The pin arm is equality, matching the host's own `latest !== installed`
 * update test: the suppression releases as soon as the offered version is
 * anything other than the pinned one.
 */
function suppressesUpdateOffer(stored, latest) {
    const suppression = parseSuppression(stored);
    if (suppression === null) {
        return false;
    }
    switch (suppression.kind) {
        case 'all':
            return true;
        case 'pinned':
            return suppression.version === latest;
    }
}
// Whether a value is one a WRITER may store: '' (updates on) or a value the
// parser recognizes. The parser accepts anything and suppresses nothing for
// what it does not recognize; this is the other half of that split, so a
/**
 * writer cannot store a value that can never match - a '1.2.3' with the '='
 * forgotten would otherwise be stored, reported as a success, and honored by
 * nothing. The host enforces the same predicate on updateModConfig.
 */
function isValidSuppression(value) {
    return value === '' || parseSuppression(value) !== null;
}
