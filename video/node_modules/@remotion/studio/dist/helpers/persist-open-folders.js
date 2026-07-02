"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ExpandedFoldersContext = exports.loadExpandedFolders = exports.persistExpandedFolders = exports.openFolderKey = void 0;
const react_1 = require("react");
const persist_boolean_map_1 = require("./persist-boolean-map");
const openFolderKey = ({ folderName, parentName, }) => {
    return [parentName !== null && parentName !== void 0 ? parentName : 'no-parent', folderName].join('/');
};
exports.openFolderKey = openFolderKey;
const sessionStorageKey = (type) => type === 'compositions'
    ? 'remotion.expandedFolders'
    : 'remotion.expandedAssetFolders';
const persistExpandedFolders = (type, state) => {
    (0, persist_boolean_map_1.persistBooleanMap)(sessionStorageKey(type), state);
};
exports.persistExpandedFolders = persistExpandedFolders;
const loadExpandedFolders = (type) => {
    return (0, persist_boolean_map_1.loadPersistedBooleanMap)(sessionStorageKey(type));
};
exports.loadExpandedFolders = loadExpandedFolders;
exports.ExpandedFoldersContext = (0, react_1.createContext)({
    toggleFolder: () => { },
    foldersExpanded: {},
    setFoldersExpanded: () => { },
});
