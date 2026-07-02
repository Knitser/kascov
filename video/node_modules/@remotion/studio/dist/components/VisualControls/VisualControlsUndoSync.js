"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.VisualControlsUndoSync = void 0;
const react_1 = require("react");
const client_id_1 = require("../../helpers/client-id");
const VisualControls_1 = require("../../visual-controls/VisualControls");
const VisualControlsUndoSync = () => {
    const { subscribeToEvent } = (0, react_1.useContext)(client_id_1.StudioServerConnectionCtx);
    const { updateValue } = (0, react_1.useContext)(VisualControls_1.SetVisualControlsContext);
    (0, react_1.useEffect)(() => {
        const unsub = subscribeToEvent('visual-control-values-changed', (event) => {
            if (event.type !== 'visual-control-values-changed') {
                return;
            }
            for (const entry of event.values) {
                updateValue(entry.id, entry.isUndefined ? undefined : entry.value);
            }
        });
        return () => unsub();
    }, [subscribeToEvent, updateValue]);
    return null;
};
exports.VisualControlsUndoSync = VisualControlsUndoSync;
