import React from 'react';
import type { SequenceNodePathInfo } from '../../helpers/get-timeline-sequence-sort-key';
import { type TimelineSelection } from '../Timeline/TimelineSelection';
import { type NavigatorKeyframe } from './keyframe-easing-navigator-items';
export declare const KeyframeEasingNavigator: React.FC<{
    readonly currentSelection: Extract<TimelineSelection, {
        type: 'keyframe';
    } | {
        type: 'easing';
    }>;
    readonly includeEasings: boolean;
    readonly keyframes: readonly NavigatorKeyframe[];
    readonly nodePathInfo: SequenceNodePathInfo;
}>;
