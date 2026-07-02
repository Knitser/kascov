"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createEffect = exports.disabledEffectField = void 0;
// Framework-level field that every effect inherits. Mirrors `hidden` for
// sequences: rendered as the eye toggle on the timeline effect row and saved
// to source via `/api/save-effect-props`. `getEffectFieldsToShow` filters it
// out of the regular field list so the toggle is the only control.
exports.disabledEffectField = {
    type: 'boolean',
    default: false,
    description: 'Disabled',
};
// Defines an effect and returns a factory that produces effect descriptors.
// The returned function is the public API effect authors expose to users:
//
//   export const blur = createEffect<BlurParams, BlurState>({ ... });
//   // usage: blur({ radius: 4, disabled: false })
//
// The descriptor erases `P` and `S` to `unknown` so descriptors of different
// effects can be freely composed inside `EffectsProp` arrays. Without that
// erasure the descriptor would be contravariant in `P` (via `apply`'s
// argument) and concrete descriptors could not flow into `unknown` slots.
const createEffect = (definition) => {
    var _a;
    // Wrap `calculateKey` to fold the framework-level `disabled` flag into the
    // memoization key. Without this, toggling `disabled` via code/drag overrides
    // would not invalidate the cached `EffectDefinitionAndStack`.
    const { calculateKey: userCalculateKey, validateParams } = definition;
    const widened = {
        ...definition,
        documentationLink: (_a = definition.documentationLink) !== null && _a !== void 0 ? _a : null,
        calculateKey: (params) => {
            var _a;
            const disabled = (_a = params.disabled) !== null && _a !== void 0 ? _a : false;
            return `${userCalculateKey(params)}-disabled-${disabled}`;
        },
        schema: {
            disabled: exports.disabledEffectField,
            ...definition.schema,
        },
    };
    const factory = (params = {}) => {
        validateParams(params);
        return {
            definition: widened,
            params,
            effectKey: widened.calculateKey(params),
            memoized: false,
        };
    };
    return factory;
};
exports.createEffect = createEffect;
