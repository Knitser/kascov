"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.useEmitVideoFrame = void 0;
const react_1 = require("react");
const useEmitVideoFrame = ({ ref, onVideoFrame, }) => {
    (0, react_1.useEffect)(() => {
        const { current } = ref;
        if (!current) {
            return;
        }
        if (!onVideoFrame) {
            return;
        }
        let handle = 0;
        const callback = (_now, metadata) => {
            if (!ref.current) {
                return;
            }
            onVideoFrame(ref.current, _now, metadata);
            handle = ref.current.requestVideoFrameCallback(callback);
        };
        onVideoFrame(current);
        if (!current.requestVideoFrameCallback) {
            return;
        }
        handle = current.requestVideoFrameCallback(callback);
        return () => {
            if (handle) {
                current.cancelVideoFrameCallback(handle);
            }
        };
    }, [onVideoFrame, ref]);
};
exports.useEmitVideoFrame = useEmitVideoFrame;
