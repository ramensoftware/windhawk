import * as fs from 'fs';
import * as ini from 'ini-win';

// fs-ext (flock) is loaded lazily, at first lock, instead of at module
// load: its native build is ABI-locked to the Electron runtime, and the
// modules that host the portable INI backends (appSettings, modConfig)
// also host the registry ones, so merely importing them must not demand
// the native module. Under the production Electron runtime this changes
// nothing. Under the plain Node that runs npm test, registry-mode code
// paths work without fs-ext at all, and the INI tests prime this require
// with a Node-ABI build from .native-cache (src/test/fsExtNodeLoader.ts).
type FsExt = typeof import('fs-ext');
let fsExtModule: FsExt | undefined;
function fsExt(): FsExt {
	// eslint-disable-next-line @typescript-eslint/no-require-imports
	fsExtModule ??= require('fs-ext') as FsExt;
	return fsExtModule;
}

export type iniValue = {
	[key: string]: {
		[key: string]: string
	}
};

export function fromFile(filePath: string) {
	const fd = fs.openSync(filePath, 'r');
	fsExt().flockSync(fd, 'sh');
	const buffer = fs.readFileSync(fd);
	fsExt().flockSync(fd, 'un');
	fs.closeSync(fd);

	let contents: string;
	if (buffer[0] === 0xFF && buffer[1] === 0xFE) {
		contents = buffer.slice(2).toString('utf16le');
	} else {
		contents = buffer.toString('utf8');
	}

	const parsed = ini.parse(contents);

	const result: iniValue = {};
	for (const [sectionName, section] of Object.entries(parsed)) {
		for (const [key, value] of Object.entries(section)) {
			if (typeof value === 'string') {
				result[sectionName] = result[sectionName] || {};
				result[sectionName][key] = value;
			}
		}
	}

	return result;
}

export function fromFileOrDefault(filePath: string, defaultValue: iniValue = {}) {
	try {
		return fromFile(filePath);
	} catch (e) {
		// Ignore if file doesn't exist.
		if (e.code !== 'ENOENT') {
			throw e;
		}
		return defaultValue;
	}
}

export function toFile(filePath: string, value: iniValue) {
	// Open without O_TRUNC, then truncate only after the exclusive lock is held.
	// Opening with 'w' truncates at open time - before flock - which would let a
	// concurrent reader (holding a shared lock) observe an emptied file, defeating
	// the lock.
	const fd = fs.openSync(filePath, fs.constants.O_WRONLY | fs.constants.O_CREAT);
	try {
		fsExt().flockSync(fd, 'ex');
		fs.ftruncateSync(fd, 0);
		fs.writeFileSync(fd, '\uFEFF' + ini.stringify(value), 'utf16le');
		fsExt().flockSync(fd, 'un');
	} finally {
		// closeSync also releases the lock, so the fd never leaks even if the
		// write throws before the explicit unlock above.
		fs.closeSync(fd);
	}
}
