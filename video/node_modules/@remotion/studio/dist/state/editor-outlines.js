"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.EditorShowOutlinesContext = exports.loadEditorShowOutlinesOption = exports.persistEditorShowOutlinesOption = void 0;
const react_1 = require("react");
const key = 'remotion.editorShowOutlines';
const persistEditorShowOutlinesOption = (option) => {
    localStorage.setItem(key, String(option));
};
exports.persistEditorShowOutlinesOption = persistEditorShowOutlinesOption;
const loadEditorShowOutlinesOption = () => {
    const item = localStorage.getItem(key);
    return item !== 'false';
};
exports.loadEditorShowOutlinesOption = loadEditorShowOutlinesOption;
exports.EditorShowOutlinesContext = (0, react_1.createContext)({
    editorShowOutlines: true,
    setEditorShowOutlines: () => undefined,
});
