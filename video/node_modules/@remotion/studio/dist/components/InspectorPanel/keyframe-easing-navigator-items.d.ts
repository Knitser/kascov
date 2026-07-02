import type { SequenceNodePathInfo } from '../../helpers/get-timeline-sequence-sort-key';
import type { TimelineSelection } from '../Timeline/TimelineSelection';
export type KeyframeNavigatorItem = {
    readonly selection: Extract<TimelineSelection, {
        type: 'keyframe';
    }>;
    readonly type: 'keyframe';
};
export type EasingNavigatorItem = {
    readonly selection: Extract<TimelineSelection, {
        type: 'easing';
    }>;
    readonly type: 'easing';
};
export type NavigatorItem = KeyframeNavigatorItem | EasingNavigatorItem;
export type NavigatorKeyframe = {
    readonly frame: number;
};
export declare const getKeyframeEasingNavigatorItems: ({ includeEasings, keyframes, nodePathInfo, }: {
    readonly includeEasings: boolean;
    readonly keyframes: readonly NavigatorKeyframe[];
    readonly nodePathInfo: SequenceNodePathInfo;
}) => NavigatorItem[];
export declare const getNavigatorItemPlayheadFrame: (item: NavigatorItem) => number;
