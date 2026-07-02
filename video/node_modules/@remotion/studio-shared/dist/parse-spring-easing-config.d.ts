export type SpringKeyframeEasing = {
    readonly type: 'spring';
    allowTail: boolean | null;
    damping: number;
    durationRestThreshold: number | null;
    mass: number;
    overshootClamping: boolean;
    stiffness: number;
};
export declare const DEFAULT_SPRING_EASING: SpringKeyframeEasing;
export declare const parseSpringEasingConfig: (node: unknown) => SpringKeyframeEasing | null;
