"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.parseCommandLine = void 0;
const config_1 = require("./config");
const parsed_cli_1 = require("./parsed-cli");
const parseCommandLine = () => {
    if (parsed_cli_1.parsedCli.png) {
        throw new Error('The --png flag has been removed. Use --sequence --image-format=png from now on.');
    }
    if (parsed_cli_1.parsedCli['license-key'] &&
        parsed_cli_1.parsedCli['license-key'].startsWith('rm_pub_')) {
        config_1.Config.setPublicLicenseKey(parsed_cli_1.parsedCli['license-key']);
    }
};
exports.parseCommandLine = parseCommandLine;
