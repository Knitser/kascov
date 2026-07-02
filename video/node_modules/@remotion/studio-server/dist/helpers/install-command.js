"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getInstallCommand = void 0;
const getInstallCommand = ({ manager, packages, version, additionalArgs, }) => {
    const pkgList = packages.map((p) => (version ? `${p}@${version}` : p));
    const commands = {
        npm: [
            'i',
            '--save-exact',
            '--no-fund',
            '--no-audit',
            ...additionalArgs,
            ...pkgList,
        ],
        pnpm: ['i', ...additionalArgs, ...pkgList],
        yarn: ['add', '--exact', ...additionalArgs, ...pkgList],
        bun: ['i', ...additionalArgs, ...pkgList],
    };
    return commands[manager];
};
exports.getInstallCommand = getInstallCommand;
