"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getNavigatorItemPlayheadFrame = exports.getKeyframeEasingNavigatorItems = void 0;
const getKeyframeEasingNavigatorItems = ({ includeEasings, keyframes, nodePathInfo, }) => {
    return keyframes.flatMap((keyframe, index) => {
        const keyframeItem = {
            type: 'keyframe',
            selection: {
                type: 'keyframe',
                nodePathInfo,
                frame: keyframe.frame,
            },
        };
        const nextKeyframe = keyframes[index + 1];
        if (!includeEasings || !nextKeyframe) {
            return [keyframeItem];
        }
        return [
            keyframeItem,
            {
                type: 'easing',
                selection: {
                    type: 'easing',
                    nodePathInfo,
                    fromFrame: keyframe.frame,
                    toFrame: nextKeyframe.frame,
                    segmentIndex: index,
                },
            },
        ];
    });
};
exports.getKeyframeEasingNavigatorItems = getKeyframeEasingNavigatorItems;
const getNavigatorItemPlayheadFrame = (item) => {
    if (item.type === 'keyframe') {
        return item.selection.frame;
    }
    return Math.round((item.selection.fromFrame + item.selection.toFrame) / 2);
};
exports.getNavigatorItemPlayheadFrame = getNavigatorItemPlayheadFrame;
