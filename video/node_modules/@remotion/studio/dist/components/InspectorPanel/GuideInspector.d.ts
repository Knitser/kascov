import React from 'react';
import type { TimelineSelection } from '../Timeline/TimelineSelection';
export declare const GuideInspector: React.FC<{
    readonly selection: Extract<TimelineSelection, {
        type: 'guide';
    }>;
}>;
