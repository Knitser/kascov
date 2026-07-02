"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.skillsCommand = exports.printSkillsHelp = void 0;
const node_child_process_1 = require("node:child_process");
const chalk_1 = require("./chalk");
const log_1 = require("./log");
const printSkillsHelp = (logLevel) => {
    log_1.Log.info({ indent: false, logLevel }, chalk_1.chalk.blue('remotion skills'));
    log_1.Log.info({ indent: false, logLevel }, 'Install or update skills from remotion-dev/skills.');
    log_1.Log.info({ indent: false, logLevel });
    log_1.Log.info({ indent: false, logLevel }, 'Available subcommands:');
    log_1.Log.info({ indent: false, logLevel });
    log_1.Log.info({ indent: false, logLevel }, chalk_1.chalk.blue('remotion skills add'));
    log_1.Log.info({ indent: false, logLevel }, 'Install skills from remotion-dev/skills.');
    log_1.Log.info({ indent: false, logLevel });
    log_1.Log.info({ indent: false, logLevel }, chalk_1.chalk.blue('remotion skills update'));
    log_1.Log.info({ indent: false, logLevel }, 'Update skills from remotion-dev/skills.');
};
exports.printSkillsHelp = printSkillsHelp;
const skillsCommand = (args, logLevel) => {
    const subcommand = args[0];
    const restArgs = args.slice(1);
    if (!subcommand || !['add', 'update'].includes(subcommand)) {
        (0, exports.printSkillsHelp)(logLevel);
        return;
    }
    const command = process.platform === 'win32' ? 'npx.cmd' : 'npx';
    const fullArgs = [
        '-y',
        '--loglevel=error',
        'skills@1.2.0',
        subcommand,
        'remotion-dev/skills',
        ...restArgs,
    ];
    const child = (0, node_child_process_1.spawn)(command, fullArgs, {
        stdio: 'inherit',
    });
    return new Promise((resolve, reject) => {
        child.on('exit', (code) => {
            if (code === 0) {
                resolve();
            }
            else {
                reject(new Error(`The skills command failed with exit code ${code}`));
            }
        });
        child.on('error', (err) => {
            reject(err);
        });
    });
};
exports.skillsCommand = skillsCommand;
