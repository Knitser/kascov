"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.extractZipArchive = void 0;
const node_fs_1 = require("node:fs");
const promises_1 = require("node:fs/promises");
const path = __importStar(require("node:path"));
const consumers_1 = require("node:stream/consumers");
const promises_2 = require("node:stream/promises");
const node_util_1 = require("node:util");
const yauzl = __importStar(require("../../vendor/yauzl-patched"));
const openZip = (0, node_util_1.promisify)(yauzl.open);
const openReadStream = (zipfile, entry) => {
    return new Promise((resolve, reject) => {
        zipfile.openReadStream(entry, (err, readStream) => {
            if (err) {
                reject(err);
                return;
            }
            if (!readStream) {
                reject(new Error('Failed to open zip read stream'));
                return;
            }
            resolve(readStream);
        });
    });
};
const getExtractedMode = (entryMode, isDir) => {
    if (entryMode !== 0) {
        return entryMode;
    }
    return isDir ? 0o755 : 0o644;
};
const extractEntry = async ({ entry, zipfile, dir, }) => {
    if (entry.fileName.startsWith('__MACOSX/')) {
        return;
    }
    const dest = path.join(dir, entry.fileName);
    const destDir = path.dirname(dest);
    await (0, promises_1.mkdir)(destDir, { recursive: true });
    const canonicalDestDir = await (0, promises_1.realpath)(destDir);
    const relativeDestDir = path.relative(dir, canonicalDestDir);
    if (relativeDestDir.split(path.sep).includes('..')) {
        throw new Error(`Out of bound path "${canonicalDestDir}" found while processing file ${entry.fileName}`);
    }
    const mode = (entry.externalFileAttributes >> 16) & 0xffff;
    const IFMT = 61440;
    const IFDIR = 16384;
    const IFLNK = 40960;
    const symlinkEntry = (mode & IFMT) === IFLNK;
    let isDir = (mode & IFMT) === IFDIR;
    if (!isDir && entry.fileName.endsWith('/')) {
        isDir = true;
    }
    const madeBy = entry.versionMadeBy >> 8;
    if (!isDir && madeBy === 0 && entry.externalFileAttributes === 16) {
        isDir = true;
    }
    const procMode = getExtractedMode(mode, isDir) & 0o777;
    const destDirectory = isDir ? dest : path.dirname(dest);
    await (0, promises_1.mkdir)(destDirectory, {
        recursive: true,
        mode: isDir ? procMode : undefined,
    });
    if (isDir) {
        return;
    }
    const readStream = await openReadStream(zipfile, entry);
    if (symlinkEntry) {
        const link = (await (0, consumers_1.buffer)(readStream)).toString();
        await (0, promises_1.symlink)(link, dest);
        return;
    }
    await (0, promises_2.pipeline)(readStream, (0, node_fs_1.createWriteStream)(dest, { mode: procMode }));
};
const extractZipArchive = async (archivePath, dir) => {
    if (!path.isAbsolute(dir)) {
        throw new Error('Target directory is expected to be absolute');
    }
    await (0, promises_1.mkdir)(dir, { recursive: true });
    const resolvedDir = await (0, promises_1.realpath)(dir);
    const zipfile = await openZip(archivePath, { lazyEntries: true });
    return new Promise((resolve, reject) => {
        let canceled = false;
        const cancel = (err) => {
            if (canceled) {
                return;
            }
            canceled = true;
            zipfile.close();
            reject(err);
        };
        zipfile.on('error', cancel);
        zipfile.on('close', () => {
            if (!canceled) {
                resolve();
            }
        });
        zipfile.on('entry', (entry) => {
            if (canceled) {
                return;
            }
            extractEntry({ entry, zipfile, dir: resolvedDir })
                .then(() => {
                zipfile.readEntry();
            })
                .catch(cancel);
        });
        zipfile.readEntry();
    });
};
exports.extractZipArchive = extractZipArchive;
