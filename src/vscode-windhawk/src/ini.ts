import * as fs from 'fs';
import * as fsExt from 'fs-ext';
import * as ini from 'ini-win';

export type iniValue = {
	[key: string]: {
		[key: string]: string
	}
};

export function fromFile(filePath: string) {
	const fd = fs.openSync(filePath, 'r');
	fsExt.flockSync(fd, 'sh');
	const buffer = fs.readFileSync(fd);
	fsExt.flockSync(fd, 'un');
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
	const fd = fs.openSync(filePath, 'w');
	fsExt.flockSync(fd, 'ex');
	fs.writeFileSync(fd, '\uFEFF' + ini.stringify(value), 'utf16le');
	fsExt.flockSync(fd, 'un');
	fs.closeSync(fd);
}
