"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.parseSpringEasingConfig = exports.DEFAULT_SPRING_EASING = void 0;
exports.DEFAULT_SPRING_EASING = {
    type: 'spring',
    allowTail: null,
    damping: 10,
    durationRestThreshold: null,
    mass: 1,
    overshootClamping: false,
    stiffness: 100,
};
const isAstNode = (value) => {
    return (typeof value === 'object' &&
        value !== null &&
        typeof value.type === 'string');
};
const getNumericValue = (node) => {
    if (node.type === 'NumericLiteral') {
        return typeof node.value === 'number' ? node.value : null;
    }
    if (node.type === 'UnaryExpression' &&
        (node.operator === '-' || node.operator === '+') &&
        isAstNode(node.argument) &&
        node.argument.type === 'NumericLiteral' &&
        typeof node.argument.value === 'number') {
        return node.operator === '-' ? -node.argument.value : node.argument.value;
    }
    if (node.type === 'TSAsExpression' && isAstNode(node.expression)) {
        return getNumericValue(node.expression);
    }
    return null;
};
const getBooleanValue = (node) => {
    if (node.type === 'BooleanLiteral') {
        return typeof node.value === 'boolean' ? node.value : null;
    }
    if (node.type === 'TSAsExpression' && isAstNode(node.expression)) {
        return getBooleanValue(node.expression);
    }
    return null;
};
const getObjectPropertyName = (prop) => {
    if (prop.computed === true || !isAstNode(prop.key)) {
        return null;
    }
    if (prop.key.type === 'Identifier') {
        return typeof prop.key.name === 'string' ? prop.key.name : null;
    }
    if (prop.key.type === 'StringLiteral') {
        return typeof prop.key.value === 'string' ? prop.key.value : null;
    }
    return null;
};
const parseSpringEasingConfig = (node) => {
    if (node === undefined) {
        return { ...exports.DEFAULT_SPRING_EASING };
    }
    if (!isAstNode(node)) {
        return null;
    }
    if (node.type === 'TSAsExpression') {
        return (0, exports.parseSpringEasingConfig)(node.expression);
    }
    if (node.type !== 'ObjectExpression' || !Array.isArray(node.properties)) {
        return null;
    }
    const spring = { ...exports.DEFAULT_SPRING_EASING };
    for (const prop of node.properties) {
        if (!isAstNode(prop) || prop.type !== 'ObjectProperty') {
            return null;
        }
        const key = getObjectPropertyName(prop);
        if (!key || !isAstNode(prop.value)) {
            return null;
        }
        if (key === 'damping' ||
            key === 'mass' ||
            key === 'stiffness' ||
            key === 'durationRestThreshold') {
            const numericValue = getNumericValue(prop.value);
            if (numericValue === null ||
                !Number.isFinite(numericValue) ||
                numericValue <= 0) {
                return null;
            }
            spring[key] = numericValue;
            continue;
        }
        if (key === 'overshootClamping' || key === 'allowTail') {
            const booleanValue = getBooleanValue(prop.value);
            if (booleanValue === null) {
                return null;
            }
            spring[key] = booleanValue;
            continue;
        }
        return null;
    }
    return spring;
};
exports.parseSpringEasingConfig = parseSpringEasingConfig;
