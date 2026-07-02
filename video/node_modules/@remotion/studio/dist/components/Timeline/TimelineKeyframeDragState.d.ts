import React from 'react';
import type { SequenceNodePathInfo } from '../../helpers/get-timeline-sequence-sort-key';
export type TimelineDraggedKeyframe = {
    readonly nodePathInfo: SequenceNodePathInfo;
    readonly frame: number;
};
export declare const getTimelineKeyframeDragKey: ({ nodePathInfo, frame, }: TimelineDraggedKeyframe) => string;
type TimelineKeyframeDragStateContextValue = {
    readonly clearDraggedKeyframes: () => void;
    readonly isKeyframeDragging: (keyframe: TimelineDraggedKeyframe) => boolean;
    readonly setDraggedKeyframes: (keyframes: readonly TimelineDraggedKeyframe[]) => void;
};
export declare const TimelineKeyframeDragStateProvider: React.FC<{
    readonly children: React.ReactNode;
}>;
export declare const useTimelineKeyframeDragState: () => TimelineKeyframeDragStateContextValue;
export {};
