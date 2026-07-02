"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.applyEffectResponseToPropStatuses = void 0;
const applyEffectResponseToPropStatuses = ({ previous, response, }) => {
    if (!previous.canUpdate) {
        return previous;
    }
    const targetIndex = previous.effects.findIndex((effect) => effect.effectIndex === response.effectIndex);
    if (targetIndex === -1) {
        return previous;
    }
    const effects = [...previous.effects];
    effects[targetIndex] = response;
    return {
        ...previous,
        effects,
    };
};
exports.applyEffectResponseToPropStatuses = applyEffectResponseToPropStatuses;
