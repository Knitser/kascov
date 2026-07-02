import React from 'react';
import { type TimelineSelection } from '../Timeline/TimelineSelection';
export declare const KeyframeInspector: React.FC<{
    readonly selection: Extract<TimelineSelection, {
        type: 'keyframe';
    }>;
}>;
