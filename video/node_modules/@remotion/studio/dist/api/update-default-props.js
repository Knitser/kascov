"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.updateDefaultProps = void 0;
const save_default_props_1 = require("./save-default-props");
/**
 * @deprecated Use `saveDefaultProps()` instead. This function is now an alias for `saveDefaultProps()`.
 * @description Updates and saves the defaultProps for a composition back to the root file.
 * @see [Documentation](https://www.remotion.dev/docs/studio/update-default-props)
 */
exports.updateDefaultProps = save_default_props_1.saveDefaultProps;
