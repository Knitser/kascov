"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.useScrollIntoViewOnSelected = exports.loopIndex = exports.QUICK_SWITCHER_RESULT_LABEL_FONT_SIZE = void 0;
const react_1 = require("react");
exports.QUICK_SWITCHER_RESULT_LABEL_FONT_SIZE = 15;
const loopIndex = (index, length) => {
    if (index < 0) {
        index += length;
    }
    return index % length;
};
exports.loopIndex = loopIndex;
const useScrollIntoViewOnSelected = (ref, selected) => {
    (0, react_1.useEffect)(() => {
        var _a;
        if (selected) {
            (_a = ref.current) === null || _a === void 0 ? void 0 : _a.scrollIntoView({
                block: 'nearest',
                inline: 'center',
            });
        }
    }, [ref, selected]);
};
exports.useScrollIntoViewOnSelected = useScrollIntoViewOnSelected;
